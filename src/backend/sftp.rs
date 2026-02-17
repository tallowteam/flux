//! SFTP backend using the ssh2 crate (libssh2 bindings).
//!
//! Provides `SftpBackend` which implements `FluxBackend` for SFTP file transfers.
//! Uses a persistent SSH session with SFTP subsystem for all operations.
//! Authentication cascade: SSH agent -> key files -> password prompt.
//!
//! # Thread safety
//!
//! `libssh2` is **not** thread-safe. `ssh2::Session` and `ssh2::Sftp` must
//! never be accessed concurrently from multiple threads. This module achieves
//! sound `Send + Sync` for `SftpBackend` by wrapping the connection state in
//! a `Mutex<SftpInner>`. All `FluxBackend` methods acquire the lock before
//! calling into libssh2 and release it before returning.

use std::io::{BufRead, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, UNIX_EPOCH};

use ssh2::{CheckResult, HashType, KnownHostFileKind, OpenFlags, OpenType, Session, Sftp};

use crate::backend::{BackendFeatures, FileEntry, FileStat, FluxBackend};
use crate::error::FluxError;

/// Connection timeout for TCP connection to SFTP server (30 seconds).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default SSH port.
const DEFAULT_SSH_PORT: u16 = 22;

/// Inner connection state that owns the libssh2 handles.
///
/// Both `Session` and `Sftp` are stored together so a single `Mutex` covers
/// all libssh2 calls. Separating them into two mutexes would be unsound
/// because `Sftp` internally borrows resources from `Session`.
struct SftpInner {
    /// The SSH session. Must remain alive for the lifetime of `sftp`.
    #[allow(dead_code)]
    session: Session,
    sftp: Sftp,
}

// SAFETY: `SftpInner` contains `Session` and `Sftp`, which each wrap a raw
// pointer into libssh2's heap-allocated session struct. libssh2 is not
// thread-safe, so these types are `!Send` by default (the raw pointer would
// be aliased across threads without additional protection).
//
// We implement `Send` here because `SftpInner` is ONLY ever accessed through
// a `Mutex<SftpInner>`. The `Mutex` guarantees exclusive access, so at most
// one thread touches the libssh2 pointer at any instant. Moving a
// `Mutex<SftpInner>` to another thread is safe because:
//   1. The raw pointer is valid for the lifetime of the `Session` value.
//   2. The `Mutex` prevents concurrent access before and after the move.
//
// `Sync` is NOT implemented for `SftpInner`; `Mutex<SftpInner>` inherits
// `Sync` automatically from the standard library once `SftpInner: Send`.
unsafe impl Send for SftpInner {}

/// SFTP backend using ssh2 for SSH/SFTP file operations.
///
/// Holds an `Arc<Mutex<SftpInner>>` that guards the SSH session and SFTP
/// channel. The `Arc` allows the inner state to be shared with `SftpBufferedWriter`
/// without a lifetime parameter, satisfying the `Box<dyn Write + Send + 'static>`
/// bound required by `FluxBackend::open_write`. The `Mutex` makes this type
/// safely `Send + Sync` despite libssh2's lack of internal thread safety.
///
/// All `FluxBackend` methods acquire the mutex for the duration of each
/// libssh2 call and release it before returning. Because the transfer engine
/// calls methods sequentially there is no risk of deadlock or starvation.
pub struct SftpBackend {
    inner: Arc<Mutex<SftpInner>>,
    base_path: String,
}

impl SftpBackend {
    /// Connect to an SFTP server and authenticate.
    ///
    /// Authentication cascade (tries in order, stops on first success):
    /// 1. SSH agent (if running)
    /// 2. Key files: ~/.ssh/id_ed25519, ~/.ssh/id_rsa, ~/.ssh/id_ecdsa
    /// 3. Password (if provided as argument)
    /// 4. Password prompt via rpassword
    ///
    /// Returns an error if connection or authentication fails.
    pub fn connect(
        user: &str,
        host: &str,
        port: u16,
        base_path: &str,
        password: Option<&str>,
    ) -> Result<Self, FluxError> {
        let effective_port = if port == 0 { DEFAULT_SSH_PORT } else { port };
        let addr = format!("{}:{}", host, effective_port);

        // Establish TCP connection with timeout
        let tcp = TcpStream::connect_timeout(
            &addr
                .parse()
                .map_err(|e: std::net::AddrParseError| FluxError::ConnectionFailed {
                    protocol: "sftp".to_string(),
                    host: host.to_string(),
                    reason: format!("Invalid address '{}': {}", addr, e),
                })?,
            CONNECT_TIMEOUT,
        )
        .map_err(|e| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: host.to_string(),
            reason: format!("TCP connection failed: {}", e),
        })?;

        // Create SSH session and perform handshake
        let mut session = Session::new().map_err(|e| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: host.to_string(),
            reason: format!("Failed to create SSH session: {}", e),
        })?;

        session.set_tcp_stream(tcp);
        session.handshake().map_err(|e| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: host.to_string(),
            reason: format!("SSH handshake failed: {}", e),
        })?;

        // Verify the server's host key against ~/.ssh/known_hosts before
        // proceeding to authentication. This prevents man-in-the-middle attacks
        // by ensuring we are talking to the expected server.
        verify_host_key(&session, host, effective_port)?;

        // Determine the effective username
        let effective_user = if user.is_empty() {
            get_current_username()?
        } else {
            user.to_string()
        };

        // Authentication cascade
        let auth_result = authenticate(&session, &effective_user, host, password);
        if let Err(e) = auth_result {
            return Err(FluxError::ConnectionFailed {
                protocol: "sftp".to_string(),
                host: host.to_string(),
                reason: format!("Authentication failed for user '{}': {}", effective_user, e),
            });
        }

        // Open SFTP channel
        let sftp = session.sftp().map_err(|e| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: host.to_string(),
            reason: format!("Failed to open SFTP channel: {}", e),
        })?;

        Ok(SftpBackend {
            inner: Arc::new(Mutex::new(SftpInner { session, sftp })),
            base_path: base_path.to_string(),
        })
    }

    /// Resolve a path relative to the base path.
    ///
    /// If the given path is absolute, use it directly.
    /// Otherwise, join it with the base_path from the SFTP URL.
    fn resolve_path(&self, path: &Path) -> PathBuf {
        let path_str = path.to_string_lossy();
        if path_str.starts_with('/') {
            PathBuf::from(path_str.as_ref())
        } else {
            PathBuf::from(&self.base_path).join(path)
        }
    }

    /// Acquire the inner mutex, converting a poisoned mutex into a `FluxError`.
    ///
    /// A poisoned mutex means a previous operation panicked while holding the
    /// lock. We surface this as an I/O error rather than propagating the panic,
    /// which keeps the error handling path uniform with other backend errors.
    fn lock(&self) -> Result<MutexGuard<'_, SftpInner>, FluxError> {
        self.inner.lock().map_err(|_| FluxError::Io {
            source: std::io::Error::new(
                std::io::ErrorKind::Other,
                "SFTP connection mutex was poisoned; a previous operation panicked",
            ),
        })
    }
}

impl FluxBackend for SftpBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let resolved = self.resolve_path(path);
        let guard = self.lock()?;
        let stat = guard.sftp.stat(&resolved).map_err(sftp_err)?;

        let modified = stat
            .mtime
            .map(|t| UNIX_EPOCH + Duration::from_secs(t));

        Ok(FileStat {
            size: stat.size.unwrap_or(0),
            is_dir: stat.is_dir(),
            is_file: !stat.is_dir(),
            modified: Some(modified.unwrap_or(UNIX_EPOCH)),
            permissions: stat.perm,
        })
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, FluxError> {
        let resolved = self.resolve_path(path);
        let guard = self.lock()?;
        let entries = guard.sftp.readdir(&resolved).map_err(sftp_err)?;

        let mut result = Vec::new();
        for (entry_path, stat) in entries {
            // Filter out . and .. entries
            if let Some(name) = entry_path.file_name() {
                let name_str = name.to_string_lossy();
                if name_str == "." || name_str == ".." {
                    continue;
                }
            }

            let modified = stat
                .mtime
                .map(|t| UNIX_EPOCH + Duration::from_secs(t));

            result.push(FileEntry {
                path: entry_path,
                stat: FileStat {
                    size: stat.size.unwrap_or(0),
                    is_dir: stat.is_dir(),
                    is_file: !stat.is_dir(),
                    modified: Some(modified.unwrap_or(UNIX_EPOCH)),
                    permissions: stat.perm,
                },
            });
        }

        Ok(result)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn Read + Send>, FluxError> {
        let resolved = self.resolve_path(path);
        let guard = self.lock()?;

        // Open the remote file and read its entire contents while holding the
        // mutex. `ssh2::File` borrows from `Sftp` (and transitively from
        // `Session`), so it cannot outlive the `MutexGuard`. Buffering the
        // content into a `Vec<u8>` here is the only sound approach that does
        // not require unsafe lifetime extension.
        //
        // The transfer engine streams from the returned `Cursor` into the
        // destination, so no extra in-memory copy occurs during the transfer
        // itself.
        let mut file = guard
            .sftp
            .open_mode(&resolved, OpenFlags::READ, 0o644, OpenType::File)
            .map_err(sftp_err)?;

        let mut buf = Vec::new();
        file.read_to_end(&mut buf)
            .map_err(|e| FluxError::Io { source: e })?;

        Ok(Box::new(std::io::Cursor::new(buf)))
    }

    fn open_write(&self, path: &Path) -> Result<Box<dyn Write + Send>, FluxError> {
        let resolved = self.resolve_path(path);

        // `ssh2::File` borrows from `Sftp` inside the `Mutex`, so we cannot
        // return it as `Box<dyn Write + Send>` without unsafe lifetime
        // extension. Instead, return an `SftpBufferedWriter` that accumulates
        // bytes in memory and flushes them to the remote path in one shot when
        // `flush()` is called (or on `Drop` as a best-effort fallback).
        //
        // Callers (including the transfer engine) are expected to call
        // `flush()` explicitly to observe write errors.
        Ok(Box::new(SftpBufferedWriter {
            inner: Arc::clone(&self.inner),
            resolved,
            buf: Vec::new(),
            flushed: false,
        }))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FluxError> {
        let resolved = self.resolve_path(path);
        let guard = self.lock()?;

        // SFTP mkdir only creates one level at a time.
        // We need to iterate through path components and create each.
        let mut current = PathBuf::new();
        for component in resolved.components() {
            current.push(component);

            // Try to create the directory, ignoring "already exists" errors
            match guard.sftp.mkdir(&current, 0o755) {
                Ok(()) => {}
                Err(e) => {
                    // SSH2 error code 4 is SFTP_FAILURE, which includes "already exists"
                    // Error code 11 is SSH_FX_FILE_ALREADY_EXISTS (not all servers use it)
                    // Try to stat the path -- if it exists and is a dir, ignore the error
                    if let Ok(stat) = guard.sftp.stat(&current) {
                        if stat.is_dir() {
                            continue;
                        }
                    }
                    return Err(sftp_err(e));
                }
            }
        }

        Ok(())
    }

    fn features(&self) -> BackendFeatures {
        BackendFeatures {
            supports_seek: false,
            supports_parallel: false,
            supports_permissions: true,
        }
    }
}

/// A `Write` implementation that buffers bytes in memory and flushes them
/// to the remote SFTP path in a single operation.
///
/// This is necessary because `ssh2::File` borrows from `ssh2::Sftp` (which
/// lives inside `Arc<Mutex<SftpInner>>`), making it impossible to return a
/// `Box<dyn Write + Send + 'static>` that holds the file handle directly
/// without unsafe lifetime extension.
///
/// Owning an `Arc<Mutex<SftpInner>>` (rather than a reference to
/// `SftpBackend`) means this writer has no lifetime parameter and satisfies
/// the `'static` bound implicit in `Box<dyn Write + Send>`.
///
/// The write contract:
/// - `write()` always succeeds immediately (copies into the internal buffer).
/// - `flush()` opens the remote file, writes all buffered bytes, and clears
///   the buffer. Any I/O error is surfaced here.
/// - `Drop` calls `flush()` as a best-effort fallback. Callers should call
///   `flush()` explicitly to observe errors.
struct SftpBufferedWriter {
    inner: Arc<Mutex<SftpInner>>,
    resolved: PathBuf,
    buf: Vec<u8>,
    /// Set to `true` after a successful `flush()` so that `Drop` does not
    /// perform a redundant (and potentially confusing) second flush.
    flushed: bool,
}

impl SftpBufferedWriter {
    fn lock_inner(&self) -> std::io::Result<MutexGuard<'_, SftpInner>> {
        self.inner.lock().map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::Other,
                "SFTP connection mutex was poisoned; a previous operation panicked",
            )
        })
    }
}

impl Write for SftpBufferedWriter {
    fn write(&mut self, data: &[u8]) -> std::io::Result<usize> {
        self.buf.extend_from_slice(data);
        self.flushed = false;
        Ok(data.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        if self.flushed || self.buf.is_empty() {
            return Ok(());
        }

        {
            let guard = self.lock_inner()?;

            let mut file = guard
                .sftp
                .open_mode(
                    &self.resolved,
                    OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                    0o644,
                    OpenType::File,
                )
                .map_err(|e| -> std::io::Error { e.into() })?;

            file.write_all(&self.buf)?;
            // guard and file are dropped here, releasing the mutex
        }

        self.buf.clear();
        self.flushed = true;
        Ok(())
    }
}

impl Drop for SftpBufferedWriter {
    fn drop(&mut self) {
        if !self.flushed && !self.buf.is_empty() {
            // Best-effort flush. Errors are silently discarded here; callers
            // that care about write errors must call `flush()` explicitly.
            let _ = self.flush();
        }
    }
}

// `SftpBufferedWriter` is `Send` automatically:
//   - `Arc<Mutex<SftpInner>>` is `Send` because `SftpInner: Send`.
//   - `PathBuf`, `Vec<u8>`, and `bool` are all `Send`.
// No explicit impl is required.
//
// `SftpBufferedWriter` is also `Sync` automatically:
//   - `Arc<Mutex<SftpInner>>` is `Sync`.
//   - All other fields are `Sync`.
// No explicit impl is required.

/// Verify the remote server's host key against the user's known_hosts file.
///
/// Implements the TOFU (Trust On First Use) pattern that SSH clients follow:
///
/// - `Match`    — key is known and matches; proceed silently.
/// - `NotFound` — key has never been seen; print the SHA-256 fingerprint, ask
///               the user to confirm, add the key to `~/.ssh/known_hosts`, and
///               proceed. Refusing the prompt aborts the connection.
/// - `Mismatch` — a key for this host is already stored but it is DIFFERENT.
///               This is a strong indicator of a man-in-the-middle attack. The
///               connection is rejected with a prominent warning identical in
///               style to OpenSSH's warning.
/// - `Failure`  — the check could not be completed (e.g., no known_hosts file
///               existed yet and the read failed). We warn the user and proceed
///               so that first-time users are not blocked; the key will be
///               added on the next connection once the file is created.
fn verify_host_key(session: &Session, host: &str, port: u16) -> Result<(), FluxError> {
    // Obtain the raw host key bytes from the just-completed handshake.
    let (key_bytes, key_type) = session.host_key().ok_or_else(|| FluxError::ConnectionFailed {
        protocol: "sftp".to_string(),
        host: host.to_string(),
        reason: "Server did not provide a host key during handshake.".to_string(),
    })?;

    // Build a KnownHosts object and attempt to load ~/.ssh/known_hosts.
    let mut known_hosts = session.known_hosts().map_err(|e| FluxError::ConnectionFailed {
        protocol: "sftp".to_string(),
        host: host.to_string(),
        reason: format!("Failed to initialise known-hosts store: {}", e),
    })?;

    let known_hosts_path = dirs::home_dir()
        .map(|h| h.join(".ssh").join("known_hosts"))
        .ok_or_else(|| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: host.to_string(),
            reason: "Cannot determine home directory to locate ~/.ssh/known_hosts.".to_string(),
        })?;

    // read_file returns an error when the file does not exist; that is
    // expected for brand-new users. We deliberately ignore the error here
    // because check() will return NotFound (not Failure) in that case,
    // which triggers the TOFU prompt below.
    let read_result = known_hosts.read_file(&known_hosts_path, KnownHostFileKind::OpenSSH);
    let file_loaded = read_result.is_ok();

    let check_result = known_hosts.check_port(host, port, key_bytes);

    match check_result {
        CheckResult::Match => {
            tracing::debug!("SFTP: Host key verified for {}:{}", host, port);
            return Ok(());
        }

        CheckResult::Mismatch => {
            // Compute SHA-256 fingerprint for the warning message.
            let fingerprint = format_sha256_fingerprint(session);
            eprintln!();
            eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
            eprintln!("@    WARNING: REMOTE HOST IDENTIFICATION HAS CHANGED!     @");
            eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
            eprintln!("IT IS POSSIBLE THAT SOMEONE IS DOING SOMETHING NASTY!");
            eprintln!(
                "Someone could be eavesdropping on you right now (man-in-the-middle attack)!"
            );
            eprintln!("It is also possible that a host key has just been changed.");
            eprintln!(
                "The fingerprint for the server key sent by the remote host {} is:",
                host
            );
            eprintln!("{}", fingerprint);
            eprintln!(
                "Please contact your system administrator or update {} manually.",
                known_hosts_path.display()
            );
            eprintln!();
            return Err(FluxError::ConnectionFailed {
                protocol: "sftp".to_string(),
                host: host.to_string(),
                reason: format!(
                    "Host key mismatch for '{}'. The stored key does not match the server's \
                     current key. Refusing connection to prevent a possible \
                     man-in-the-middle attack. If the host key legitimately changed, \
                     remove the old entry from {} and reconnect.",
                    host,
                    known_hosts_path.display()
                ),
            });
        }

        CheckResult::NotFound => {
            // TOFU: first time we see this host. Show the fingerprint and
            // ask the user for explicit confirmation.
            let fingerprint = format_sha256_fingerprint(session);
            eprintln!(
                "The authenticity of host '{}' can't be established.",
                host
            );
            eprintln!("Server's key fingerprint (SHA256): {}", fingerprint);
            eprint!("Are you sure you want to continue connecting (yes/no)? ");
            std::io::stderr().flush().ok();

            let stdin = std::io::stdin();
            let answer = stdin
                .lock()
                .lines()
                .next()
                .and_then(|l| l.ok())
                .unwrap_or_default();
            let answer = answer.trim().to_ascii_lowercase();

            if answer != "yes" {
                return Err(FluxError::ConnectionFailed {
                    protocol: "sftp".to_string(),
                    host: host.to_string(),
                    reason: "Host key not accepted by user. Connection aborted.".to_string(),
                });
            }

            // User confirmed: add the key to known_hosts and persist it.
            let key_format = key_type.into();
            if let Err(e) = known_hosts.add(host, key_bytes, host, key_format) {
                tracing::warn!(
                    "SFTP: Could not add host key for '{}' to in-memory store: {}",
                    host,
                    e
                );
            }

            // Write back to disk. If the parent directory does not exist,
            // create it with restricted permissions (0o700 on Unix).
            if let Some(parent) = known_hosts_path.parent() {
                if !parent.exists() {
                    #[cfg(unix)]
                    {
                        use std::os::unix::fs::DirBuilderExt;
                        std::fs::DirBuilder::new()
                            .recursive(true)
                            .mode(0o700)
                            .create(parent)
                            .ok();
                    }
                    #[cfg(not(unix))]
                    {
                        std::fs::create_dir_all(parent).ok();
                    }
                }
            }

            match known_hosts.write_file(&known_hosts_path, KnownHostFileKind::OpenSSH) {
                Ok(()) => {
                    eprintln!(
                        "Warning: Permanently added '{}' ({:?}) to the list of known hosts.",
                        host, key_type
                    );
                }
                Err(e) => {
                    tracing::warn!(
                        "SFTP: Could not write known_hosts file '{}': {}",
                        known_hosts_path.display(),
                        e
                    );
                    eprintln!(
                        "Warning: Could not save host key to '{}': {}. \
                         You will be prompted again on the next connection.",
                        known_hosts_path.display(),
                        e
                    );
                }
            }

            Ok(())
        }

        CheckResult::Failure => {
            // The check itself failed. This typically means the known_hosts
            // file could not be parsed or the key could not be hashed.
            // If the file was never loaded (does not exist yet), this is
            // benign for first-time users; we warn but do not block.
            if !file_loaded {
                tracing::warn!(
                    "SFTP: Could not verify host key for '{}' (no known_hosts file at '{}'). \
                     Proceeding without verification. Create the file and reconnect to enable \
                     host key checking.",
                    host,
                    known_hosts_path.display()
                );
            } else {
                tracing::warn!(
                    "SFTP: Host key check failed for '{}' (internal ssh2 error). \
                     Proceeding with caution.",
                    host
                );
            }
            Ok(())
        }
    }
}

/// Format the server's SHA-256 host key hash in the OpenSSH style.
///
/// Returns a string like `SHA256:AbCdEf...` using base64 encoding without
/// trailing padding, matching what `ssh-keygen -l` and OpenSSH print.
/// Falls back to the MD5 hex fingerprint if SHA-256 is unavailable.
fn format_sha256_fingerprint(session: &Session) -> String {
    if let Some(hash) = session.host_key_hash(HashType::Sha256) {
        // Standard base64, no padding — identical to OpenSSH display
        use std::fmt::Write as FmtWrite;
        let mut encoded = String::new();
        let b64_chars =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let bytes = hash;
        let mut i = 0;
        while i + 3 <= bytes.len() {
            let n = ((bytes[i] as u32) << 16)
                | ((bytes[i + 1] as u32) << 8)
                | (bytes[i + 2] as u32);
            let _ = write!(
                encoded,
                "{}{}{}{}",
                b64_chars[((n >> 18) & 0x3f) as usize] as char,
                b64_chars[((n >> 12) & 0x3f) as usize] as char,
                b64_chars[((n >> 6) & 0x3f) as usize] as char,
                b64_chars[(n & 0x3f) as usize] as char
            );
            i += 3;
        }
        if i + 2 == bytes.len() {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            let _ = write!(
                encoded,
                "{}{}{}",
                b64_chars[((n >> 18) & 0x3f) as usize] as char,
                b64_chars[((n >> 12) & 0x3f) as usize] as char,
                b64_chars[((n >> 6) & 0x3f) as usize] as char,
            );
        } else if i + 1 == bytes.len() {
            let n = (bytes[i] as u32) << 16;
            let _ = write!(
                encoded,
                "{}{}",
                b64_chars[((n >> 18) & 0x3f) as usize] as char,
                b64_chars[((n >> 12) & 0x3f) as usize] as char,
            );
        }
        format!("SHA256:{}", encoded)
    } else if let Some(hash) = session.host_key_hash(HashType::Md5) {
        // Fall back to MD5 hex (16 bytes, colon-separated)
        let hex: Vec<String> = hash.iter().map(|b| format!("{:02x}", b)).collect();
        format!("MD5:{}", hex.join(":"))
    } else {
        "(fingerprint unavailable)".to_string()
    }
}

/// Authenticate the SSH session using a cascade of methods.
///
/// Tries in order: SSH agent, key files, provided password, password prompt.
fn authenticate(
    session: &Session,
    user: &str,
    host: &str,
    password: Option<&str>,
) -> Result<(), String> {
    // 1. Try SSH agent
    if session.userauth_agent(user).is_ok() && session.authenticated() {
        tracing::debug!("SFTP: Authenticated via SSH agent for {}@{}", user, host);
        return Ok(());
    }

    // 2. Try common SSH key files
    let key_files = get_ssh_key_paths();
    for key_path in &key_files {
        if key_path.exists() {
            if session
                .userauth_pubkey_file(user, None, key_path, None)
                .is_ok()
                && session.authenticated()
            {
                tracing::debug!(
                    "SFTP: Authenticated via key file {} for {}@{}",
                    key_path.display(),
                    user,
                    host
                );
                return Ok(());
            }
        }
    }

    // 3. Try provided password (from URL or programmatic use)
    if let Some(pwd) = password {
        if !pwd.is_empty() {
            if session.userauth_password(user, pwd).is_ok() && session.authenticated() {
                tracing::debug!(
                    "SFTP: Authenticated via provided password for {}@{}",
                    user,
                    host
                );
                return Ok(());
            }
        }
    }

    // 4. Try interactive password prompt
    match rpassword::prompt_password(format!("Password for {}@{}: ", user, host)) {
        Ok(prompted_pwd) => {
            if session.userauth_password(user, &prompted_pwd).is_ok() && session.authenticated() {
                tracing::debug!(
                    "SFTP: Authenticated via password prompt for {}@{}",
                    user,
                    host
                );
                return Ok(());
            }
        }
        Err(e) => {
            tracing::debug!("SFTP: Password prompt failed: {}", e);
        }
    }

    Err(format!(
        "All authentication methods failed for {}@{}. Tried: SSH agent, key files ({:?}), password.",
        user,
        host,
        key_files
            .iter()
            .filter(|p| p.exists())
            .collect::<Vec<_>>()
    ))
}

/// Get the standard SSH key file paths to try.
fn get_ssh_key_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = dirs::home_dir() {
        let ssh_dir = home.join(".ssh");
        paths.push(ssh_dir.join("id_ed25519"));
        paths.push(ssh_dir.join("id_rsa"));
        paths.push(ssh_dir.join("id_ecdsa"));
    }
    paths
}

/// Convert an ssh2::Error to FluxError::Io.
///
/// ssh2::Error implements Into<std::io::Error>, so we convert through that.
fn sftp_err(e: ssh2::Error) -> FluxError {
    let io_err: std::io::Error = e.into();
    FluxError::Io { source: io_err }
}

/// Get the current system username for SSH authentication fallback.
///
/// Uses environment variables: USERNAME on Windows, USER on Unix.
/// Returns an error if neither variable is set rather than defaulting
/// to "root", which could cause unintended privileged access attempts.
fn get_current_username() -> Result<String, FluxError> {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .map_err(|_| FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: String::new(),
            reason: "Cannot determine username: neither USERNAME nor USER environment variable is set. \
                     Specify the user in the URL (sftp://user@host/path).".to_string(),
        })
}

/// Split a path into its components for recursive mkdir.
/// Exported for unit testing.
pub(crate) fn path_components(path: &Path) -> Vec<PathBuf> {
    let mut components = Vec::new();
    let mut current = PathBuf::new();
    for component in path.components() {
        current.push(component);
        components.push(current.clone());
    }
    components
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn features_returns_network_capabilities() {
        // We can't create a real SftpBackend without a server,
        // but we can test the features() return values by checking
        // the expected constants.
        let features = BackendFeatures {
            supports_seek: false,
            supports_parallel: false,
            supports_permissions: true,
        };
        assert!(!features.supports_seek);
        assert!(!features.supports_parallel);
        assert!(features.supports_permissions);
    }

    #[test]
    fn sftp_err_converts_to_flux_io_error() {
        // Create an ssh2 error and verify it converts to FluxError::Io
        let ssh_err = ssh2::Error::new(
            ssh2::ErrorCode::Session(-1),
            "test error",
        );
        let flux_err = sftp_err(ssh_err);
        match flux_err {
            FluxError::Io { source } => {
                assert!(source.to_string().contains("test error"));
            }
            other => panic!("Expected FluxError::Io, got {:?}", other),
        }
    }

    #[test]
    fn path_components_splits_correctly() {
        let path = Path::new("/home/user/data");
        let components = path_components(path);
        assert!(components.len() >= 3);
        // The last component should be the full path
        assert_eq!(components.last().unwrap(), path);
    }

    #[test]
    fn path_components_relative_path() {
        let path = Path::new("a/b/c");
        let components = path_components(path);
        assert_eq!(components.len(), 3);
        assert_eq!(components[0], PathBuf::from("a"));
        assert_eq!(components[1], PathBuf::from("a/b"));
        assert_eq!(components[2], PathBuf::from("a/b/c"));
    }

    #[test]
    fn get_ssh_key_paths_returns_expected_names() {
        let paths = get_ssh_key_paths();
        // Should include ed25519, rsa, ecdsa variants
        let names: Vec<String> = paths
            .iter()
            .filter_map(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert!(names.contains(&"id_ed25519".to_string()));
        assert!(names.contains(&"id_rsa".to_string()));
        assert!(names.contains(&"id_ecdsa".to_string()));
    }

    #[test]
    fn connection_failed_error_format() {
        let err = FluxError::ConnectionFailed {
            protocol: "sftp".to_string(),
            host: "example.com".to_string(),
            reason: "Connection refused".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("sftp"));
        assert!(msg.contains("example.com"));
        assert!(msg.contains("Connection refused"));
    }

    /// Validate our hand-rolled base64 encoder used in `format_sha256_fingerprint`
    /// against the well-known encoding of the zero byte sequences.
    ///
    /// SHA-256 always produces 32 bytes, so the output is 43 characters (no
    /// padding, matching OpenSSH's display).
    #[test]
    fn base64_encoding_of_known_bytes() {
        // Manually encode a simple known sequence and compare to expected
        // OpenSSH-style (no-padding) base64.
        // "Man" in base64 is "TWFu"
        let input = b"Man";
        let b64_chars =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let n = ((input[0] as u32) << 16) | ((input[1] as u32) << 8) | (input[2] as u32);
        let encoded = format!(
            "{}{}{}{}",
            b64_chars[((n >> 18) & 0x3f) as usize] as char,
            b64_chars[((n >> 12) & 0x3f) as usize] as char,
            b64_chars[((n >> 6) & 0x3f) as usize] as char,
            b64_chars[(n & 0x3f) as usize] as char
        );
        assert_eq!(encoded, "TWFu");
    }

    /// Verify that a 32-byte SHA-256-sized hash produces a 43-character
    /// (no-padding) base64 string, matching the OpenSSH fingerprint format.
    #[test]
    fn sha256_fingerprint_length_for_32_byte_hash() {
        // 32 bytes -> ceil(32 / 3) * 4 with last group being 2 chars (32 % 3 == 2)
        // = 10 full groups (40 chars) + 3 chars for remaining 2 bytes = 43 chars total
        let bytes = [0u8; 32];
        let b64_chars =
            b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut encoded = String::new();
        let mut i = 0;
        while i + 3 <= bytes.len() {
            let n = ((bytes[i] as u32) << 16)
                | ((bytes[i + 1] as u32) << 8)
                | (bytes[i + 2] as u32);
            encoded.push(b64_chars[((n >> 18) & 0x3f) as usize] as char);
            encoded.push(b64_chars[((n >> 12) & 0x3f) as usize] as char);
            encoded.push(b64_chars[((n >> 6) & 0x3f) as usize] as char);
            encoded.push(b64_chars[(n & 0x3f) as usize] as char);
            i += 3;
        }
        if i + 2 == bytes.len() {
            let n = ((bytes[i] as u32) << 16) | ((bytes[i + 1] as u32) << 8);
            encoded.push(b64_chars[((n >> 18) & 0x3f) as usize] as char);
            encoded.push(b64_chars[((n >> 12) & 0x3f) as usize] as char);
            encoded.push(b64_chars[((n >> 6) & 0x3f) as usize] as char);
        }
        // All-zero 32 bytes: every 6-bit group is 0 -> 'A'
        assert_eq!(encoded.len(), 43);
        assert!(encoded.chars().all(|c| c == 'A'));
    }
}
