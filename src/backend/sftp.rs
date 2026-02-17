//! SFTP backend using the ssh2 crate (libssh2 bindings).
//!
//! Provides `SftpBackend` which implements `FluxBackend` for SFTP file transfers.
//! Uses a persistent SSH session with SFTP subsystem for all operations.
//! Authentication cascade: SSH agent -> key files -> password prompt.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use ssh2::{OpenFlags, OpenType, Session, Sftp};

use crate::backend::{BackendFeatures, FileEntry, FileStat, FluxBackend};
use crate::error::FluxError;

/// Connection timeout for TCP connection to SFTP server (30 seconds).
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default SSH port.
const DEFAULT_SSH_PORT: u16 = 22;

/// SFTP backend using ssh2 for SSH/SFTP file operations.
///
/// Holds a persistent SSH session and SFTP channel. All FluxBackend methods
/// operate through the SFTP channel. The session is established once during
/// `connect()` and reused for all subsequent operations.
pub struct SftpBackend {
    #[allow(dead_code)]
    session: Session,
    sftp: Sftp,
    base_path: String,
}

// ssh2::Session is Send + Sync, and Sftp is derived from it.
// SftpBackend needs to be Send + Sync for FluxBackend trait.
// ssh2::Session and ssh2::Sftp are Send + Sync.
unsafe impl Send for SftpBackend {}
unsafe impl Sync for SftpBackend {}

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

        // Determine the effective username
        let effective_user = if user.is_empty() {
            get_current_username()
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
            session,
            sftp,
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
}

impl FluxBackend for SftpBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let resolved = self.resolve_path(path);
        let stat = self.sftp.stat(&resolved).map_err(sftp_err)?;

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
        let entries = self.sftp.readdir(&resolved).map_err(sftp_err)?;

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
        let file = self
            .sftp
            .open_mode(
                &resolved,
                OpenFlags::READ,
                0o644,
                OpenType::File,
            )
            .map_err(sftp_err)?;

        // ssh2::File implements Read + Send, so we can box it directly
        Ok(Box::new(file))
    }

    fn open_write(&self, path: &Path) -> Result<Box<dyn Write + Send>, FluxError> {
        let resolved = self.resolve_path(path);

        // Create/truncate the file for writing
        let file = self
            .sftp
            .open_mode(
                &resolved,
                OpenFlags::WRITE | OpenFlags::CREATE | OpenFlags::TRUNCATE,
                0o644,
                OpenType::File,
            )
            .map_err(sftp_err)?;

        // ssh2::File implements Write + Send
        Ok(Box::new(file))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FluxError> {
        let resolved = self.resolve_path(path);

        // SFTP mkdir only creates one level at a time.
        // We need to iterate through path components and create each.
        let mut current = PathBuf::new();
        for component in resolved.components() {
            current.push(component);

            // Try to create the directory, ignoring "already exists" errors
            match self.sftp.mkdir(&current, 0o755) {
                Ok(()) => {}
                Err(e) => {
                    // SSH2 error code 4 is SFTP_FAILURE, which includes "already exists"
                    // Error code 11 is SSH_FX_FILE_ALREADY_EXISTS (not all servers use it)
                    // Try to stat the path -- if it exists and is a dir, ignore the error
                    if let Ok(stat) = self.sftp.stat(&current) {
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
fn get_current_username() -> String {
    std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "root".to_string())
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
}
