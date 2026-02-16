# Phase 3: Network Protocols - Research

**Researched:** 2026-02-16
**Domain:** Network protocol backends (SFTP, SMB, WebDAV) for Rust CLI file transfer tool
**Confidence:** MEDIUM

## Summary

Phase 3 adds network file transfer support to Flux by implementing the `FluxBackend` trait for three protocols: SFTP, SMB, and WebDAV. The current codebase has a synchronous `FluxBackend` trait that returns `Box<dyn std::io::Read + Send>` and `Box<dyn std::io::Write + Send>`, with a `LocalBackend` implementation. The core architectural decision is how to bridge these network protocols (some inherently async, some with C library dependencies) into the existing sync trait surface.

The Rust ecosystem provides workable libraries for all three protocols: `ssh2` (v0.9.5, sync, libssh2-based) for SFTP, `pavao` (v0.2.16, sync, libsmbclient-based) for SMB on non-Windows, Windows native UNC path support via `std::fs` for SMB on Windows, and `reqwest_dav` (v0.3.2, async, reqwest-based) for WebDAV. The SMB story is the most complex due to platform differences and library maturity gaps.

**Primary recommendation:** Keep the `FluxBackend` trait synchronous. Use `ssh2` for SFTP (natively sync), platform-conditional SMB support (Windows UNC paths via `std::fs`/`sambrs`, cross-platform via `pavao` with `vendored` feature), and bridge `reqwest_dav` async calls into sync via `tokio::runtime::Runtime::block_on`. Add a URL/path parser module that auto-detects protocol from path format.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PROT-02 | User can transfer to/from SMB/CIFS network shares | SMB backend using platform-conditional approach: Windows UNC paths natively via `std::fs` (+ optional `sambrs` for authenticated connections), Linux/macOS via `pavao` wrapping libsmbclient. Both provide sync Read/Write on files. |
| PROT-03 | User can transfer to/from SFTP servers | SFTP backend using `ssh2` crate (v0.9.5). `ssh2::Sftp` provides `stat()`, `readdir()`, `mkdir()`, `open()`, `create()` methods. `ssh2::File` implements `std::io::Read + Write + Send + Sync`, directly compatible with `FluxBackend` trait. |
| PROT-04 | User can transfer to/from WebDAV endpoints | WebDAV backend using `reqwest_dav` (v0.3.2). Async client bridged to sync via `tokio::runtime::Runtime::block_on`. Provides `get()`, `put()`, `list()`, `mkcol()` operations. File data transferred as `reqwest::Body` / `Response` bytes. |
| PROT-05 | Tool auto-detects protocol from path format | Path parser module using the `url` crate for URI parsing. Detection rules: `\\server\share` or `smb://` -> SMB, `sftp://` -> SFTP, `https://` or `http://` with WebDAV heuristic -> WebDAV, everything else -> local. No `--protocol` flag needed. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `ssh2` | 0.9.5 | SFTP client via libssh2 bindings | 181K downloads/month, 188 dependents, maintained by Rust ecosystem maintainers, sync API with `Read`/`Write` trait impls on File |
| `pavao` | 0.2.16 | SMB 2/3 client via libsmbclient bindings | Most mature Rust SMB client with type-safe API, `SmbFile` implements `Read`/`Write`/`Seek`, supports vendored libsmbclient |
| `reqwest_dav` | 0.3.2 | WebDAV client (async, reqwest-based) | 56K downloads/month, actively maintained (Feb 2026), supports Basic/Digest auth, Get/Put/List/Mkcol operations |
| `url` | latest | URL/URI parsing for protocol detection | WHATWG URL Standard implementation, handles scheme detection, UNC paths, widely used |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `sambrs` | latest | Windows SMB share connection helper | Windows only: wraps `WNetAddConnection2A` to mount SMB shares, then use `std::fs` |
| `reqwest` | 0.12+ | HTTP client (dependency of `reqwest_dav`) | Already pulled in transitively; also useful for WebDAV raw operations if needed |
| `ssh2-config` | 0.6.2 | SSH config file parser | Reading `~/.ssh/config` for host aliases, port overrides, key paths |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `ssh2` (libssh2 bindings) | `russh` + `russh-sftp` (pure Rust) | `russh` is async-only, would require full async migration; `ssh2` is sync and directly compatible with current trait |
| `pavao` (libsmbclient) | `smb` crate (pure Rust, v0.11.1) | Pure Rust SMB is appealing but has only 2.4K downloads/month, 66 GitHub stars, 13 open issues, uses `ReadAt`/`WriteAt` not `Read`/`Write` traits, and is async-only via tokio |
| `reqwest_dav` (async) | `rustydav` (sync) | `rustydav` last updated Oct 2021, 456 downloads/month, appears abandoned; `reqwest_dav` is actively maintained |
| Per-protocol crates | `remotefs` ecosystem | `remotefs` provides a unified trait (`RemoteFs`) for SSH, SMB, WebDAV but has its own trait surface that doesn't map cleanly to `FluxBackend` |

**Installation:**
```toml
[dependencies]
# SFTP
ssh2 = { version = "0.9", features = ["vendored-openssl"] }

# SMB (cross-platform)
pavao = { version = "0.2", features = ["vendored"], optional = true }

# SMB (Windows native connection helper)
[target.'cfg(windows)'.dependencies]
sambrs = "0.1"

# WebDAV
reqwest_dav = "0.3"
reqwest = { version = "0.12", features = ["blocking"] }

# URL parsing
url = "2"

# Tokio already in Cargo.toml: tokio = { version = "1", features = ["full"] }
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── backend/
│   ├── mod.rs           # FluxBackend trait + FileStat/FileEntry/BackendFeatures
│   ├── local.rs         # LocalBackend (existing)
│   ├── sftp.rs          # SftpBackend (new)
│   ├── smb.rs           # SmbBackend (new)
│   └── webdav.rs        # WebDavBackend (new)
├── protocol/
│   ├── mod.rs           # Protocol detection + FluxPath type
│   ├── parser.rs        # URL/path parsing, protocol auto-detection
│   └── auth.rs          # Credential handling (passwords, keys, etc.)
├── cli/
│   ├── args.rs          # Updated: source/dest become String, not PathBuf
│   └── mod.rs
├── transfer/
│   └── mod.rs           # Updated: resolve backend from FluxPath before copy
└── ...
```

### Pattern 1: Protocol Auto-Detection via Path Parsing
**What:** Parse source and destination strings to determine which `FluxBackend` implementation to use.
**When to use:** Every time a path is provided by the user (CLI args).
**Example:**
```rust
// Source: custom implementation
use url::Url;

pub enum Protocol {
    Local,
    Sftp { user: String, host: String, port: u16, path: String },
    Smb { server: String, share: String, path: String },
    WebDav { url: String, auth: Option<Auth> },
}

pub fn detect_protocol(input: &str) -> Protocol {
    // UNC path: \\server\share\path or //server/share/path
    if input.starts_with("\\\\") || input.starts_with("//") {
        return parse_unc_path(input);
    }
    // URL-style: sftp://, smb://, https://, http://
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "sftp" => return parse_sftp_url(&url),
            "smb"  => return parse_smb_url(&url),
            "https" | "http" => return Protocol::WebDav { url: input.to_string(), auth: None },
            _ => {}
        }
    }
    Protocol::Local
}
```

### Pattern 2: Backend Factory from Protocol
**What:** Create the appropriate `Box<dyn FluxBackend>` from a detected `Protocol`.
**When to use:** After protocol detection, before transfer begins.
**Example:**
```rust
pub fn create_backend(protocol: &Protocol) -> Result<Box<dyn FluxBackend>, FluxError> {
    match protocol {
        Protocol::Local => Ok(Box::new(LocalBackend::new())),
        Protocol::Sftp { user, host, port, path } => {
            Ok(Box::new(SftpBackend::connect(user, host, *port)?))
        }
        Protocol::Smb { server, share, path } => {
            Ok(Box::new(SmbBackend::connect(server, share)?))
        }
        Protocol::WebDav { url, auth } => {
            Ok(Box::new(WebDavBackend::new(url, auth.clone())?))
        }
    }
}
```

### Pattern 3: Sync-over-Async Bridge for WebDAV
**What:** Wrap async `reqwest_dav` calls in a sync interface using a stored `tokio::runtime::Runtime`.
**When to use:** WebDavBackend implementation of the sync `FluxBackend` trait.
**Example:**
```rust
// Source: tokio.rs bridging docs
pub struct WebDavBackend {
    rt: tokio::runtime::Runtime,
    client: reqwest_dav::Client,
    base_url: String,
}

impl FluxBackend for WebDavBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        self.rt.block_on(async {
            let items = self.client.list(path.to_str().unwrap(), Depth::Number(0)).await
                .map_err(|e| FluxError::Io { source: io_error_from(e) })?;
            // Convert first item to FileStat
            Ok(items_to_stat(&items))
        })
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn Read + Send>, FluxError> {
        let bytes = self.rt.block_on(async {
            let response = self.client.get(path.to_str().unwrap()).await
                .map_err(|e| FluxError::Io { source: io_error_from(e) })?;
            response.bytes().await
                .map_err(|e| FluxError::Io { source: io_error_from(e) })
        })?;
        Ok(Box::new(std::io::Cursor::new(bytes)))
    }
}
```

### Pattern 4: SFTP Backend with ssh2 (Natively Sync)
**What:** Direct implementation using ssh2's sync API.
**When to use:** SFTP backend - most straightforward mapping.
**Example:**
```rust
// Source: docs.rs/ssh2
pub struct SftpBackend {
    session: ssh2::Session,
    sftp: ssh2::Sftp,
}

impl SftpBackend {
    pub fn connect(user: &str, host: &str, port: u16) -> Result<Self, FluxError> {
        let tcp = std::net::TcpStream::connect((host, port))?;
        let mut session = ssh2::Session::new()?;
        session.set_tcp_stream(tcp);
        session.handshake()?;
        // Try agent auth, then password, then key file
        session.userauth_agent(user)
            .or_else(|_| session.userauth_pubkey_file(
                user, None, &home_dir().join(".ssh/id_rsa"), None
            ))?;
        let sftp = session.sftp()?;
        Ok(SftpBackend { session, sftp })
    }
}

impl FluxBackend for SftpBackend {
    fn open_read(&self, path: &Path) -> Result<Box<dyn Read + Send>, FluxError> {
        let file = self.sftp.open(path)?;
        // ssh2::File implements Read + Send
        Ok(Box::new(file))
    }

    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let stat = self.sftp.stat(path)?;
        Ok(FileStat {
            size: stat.size.unwrap_or(0),
            is_dir: stat.is_dir(),
            is_file: stat.is_file(),
            modified: stat.mtime.map(|t| /* convert */),
            permissions: stat.perm,
        })
    }
}
```

### Anti-Patterns to Avoid
- **Making FluxBackend async now:** The entire transfer engine (rayon parallel chunks, progress tracking, resume manifests) is built on sync `std::io::Read`/`Write`. Converting to async would require rewriting 80%+ of Phase 1-2 code. ssh2 is sync, pavao is sync. Only WebDAV needs async bridging, and `block_on` handles this cleanly.
- **Using one SMB library cross-platform:** No single SMB crate works well everywhere. Windows has native OS support for UNC paths; Linux/macOS need libsmbclient. Use platform-conditional compilation (`cfg` attributes).
- **Storing connections globally:** Each backend should own its connection. Connections are not thread-safe in general (pavao's SmbFile is `!Send + !Sync`, ssh2 sessions are `Send + Sync` but not designed for concurrent use from multiple threads).
- **Parallel chunked transfer over network protocols:** The existing `parallel_copy_chunked` uses `read_at`/`write_at` positional I/O on `std::fs::File`. Network protocols do not support positional I/O. Network transfers should fall back to sequential streaming via `open_read`/`open_write`. Set `supports_parallel: false` and `supports_seek: false` in `BackendFeatures`.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SSH/SFTP protocol | Custom SSH handshake/channel management | `ssh2` crate (libssh2) | SSH protocol is enormously complex: key exchange, host key verification, channel multiplexing, SFTP binary protocol |
| SMB protocol | Custom SMB2/3 packet parsing | `pavao` (libsmbclient) or OS native | SMB has dozens of message types, NTLM/Kerberos auth, signing, encryption, dialect negotiation |
| WebDAV protocol | Custom HTTP+XML parsing | `reqwest_dav` | WebDAV is HTTP + custom XML request/response bodies, PROPFIND/PROPPATCH, depth headers, multi-status responses |
| URL parsing | Regex-based path parsing | `url` crate | WHATWG URL Standard handles edge cases: percent-encoding, IPv6 hosts, port normalization, relative references |
| SSH agent integration | Custom agent socket communication | `ssh2::Session::userauth_agent` | SSH agent protocol is OS-specific (Unix socket vs Windows named pipe) |

**Key insight:** All three network protocols are standardized with decades of RFCs and edge cases. The C libraries (libssh2, libsmbclient) have years of security patches and interoperability testing that a hand-rolled solution would lack.

## Common Pitfalls

### Pitfall 1: CpArgs Using PathBuf for Network Paths
**What goes wrong:** `PathBuf` normalizes paths according to OS rules, which corrupts network URIs. `sftp://user@host/path` becomes a local path. UNC paths `\\server\share` may also be mangled.
**Why it happens:** Phase 1 correctly used `PathBuf` for local-only paths. Network paths are URIs, not filesystem paths.
**How to avoid:** Change `CpArgs.source` and `CpArgs.dest` from `PathBuf` to `String`. Parse them in `execute_copy` through the protocol detection module. Local paths get converted to `PathBuf` after detection.
**Warning signs:** Tests with `sftp://` paths fail at argument parsing stage.

### Pitfall 2: Attempting Parallel Chunked Copy Over Network
**What goes wrong:** `parallel_copy_chunked` uses `read_at`/`write_at` (positional I/O), which requires OS-level file handle support. Network streams don't support this.
**Why it happens:** Parallel chunks were designed for local-to-local transfers with random access to both files.
**How to avoid:** Check `backend.features().supports_parallel` before choosing the parallel path. Network backends return `supports_parallel: false`. Fall back to sequential streaming copy (which already exists in the codebase).
**Warning signs:** Runtime panic or error when trying to cast `Box<dyn Read>` to `std::fs::File`.

### Pitfall 3: ssh2 Build Failure on Windows Without OpenSSL
**What goes wrong:** `ssh2` depends on `libssh2-sys` which needs OpenSSL. Without it, build fails with cryptic CMake/linker errors.
**Why it happens:** libssh2 requires an SSL/crypto backend. System OpenSSL is rarely available on Windows.
**How to avoid:** Always enable the `vendored-openssl` feature for `ssh2`: `ssh2 = { version = "0.9", features = ["vendored-openssl"] }`. This statically compiles OpenSSL.
**Warning signs:** Build errors mentioning `openssl`, `cmake`, or `libssh2`.

### Pitfall 4: pavao SmbFile is !Send and !Sync
**What goes wrong:** Cannot pass `SmbFile` across thread boundaries. Breaks `FluxBackend: Send + Sync` requirement since `open_read` returns `Box<dyn Read + Send>`.
**Why it happens:** libsmbclient C library uses thread-local state.
**How to avoid:** Read the entire file into memory (for small files) and return a `Cursor<Vec<u8>>`, or read into a temporary local file and return a handle to that. For large files, consider a channel-based reader that reads on the SMB thread and pipes data through a `std::sync::mpsc` channel wrapped in a `Read` adapter.
**Warning signs:** Compiler error: "SmbFile cannot be sent between threads safely".

### Pitfall 5: WebDAV open_read Loading Entire File Into Memory
**What goes wrong:** `reqwest_dav::Client::get()` returns a `Response`. Calling `.bytes().await` loads the entire file into memory, which fails for large files.
**Why it happens:** The sync `FluxBackend` trait expects a `Box<dyn Read>`, but the response body is async.
**How to avoid:** For the initial implementation, use `Cursor<Vec<u8>>` for small/medium files. For large files, spawn a background tokio task that streams chunks into a `std::sync::mpsc::channel`, and wrap the receiver in a `Read` adapter. Or write to a temp file first.
**Warning signs:** Out-of-memory when transferring large files via WebDAV.

### Pitfall 6: Connection Lifetime and Reuse
**What goes wrong:** Creating a new SSH session or SMB connection for every single file in a directory copy is extremely slow (SSH handshake alone takes 100-500ms).
**Why it happens:** `FluxBackend` is stateless in the current design.
**How to avoid:** Backend instances should hold persistent connections. Create the backend once (with connection), use it for the entire transfer operation. The factory pattern (create backend from Protocol) naturally supports this.
**Warning signs:** Directory copies with many small files are 100x slower over network than expected.

### Pitfall 7: Hardcoded Authentication
**What goes wrong:** Users have to type passwords on every invocation. No support for SSH keys, SSH agent, or credential stores.
**Why it happens:** Shipping the simplest possible auth first.
**How to avoid:** For SFTP: try SSH agent first, then key files (`~/.ssh/id_rsa`, `~/.ssh/id_ed25519`), then password prompt. For SMB: try Windows credential store on Windows, else prompt. For WebDAV: support Basic auth credentials in URL or prompt.
**Warning signs:** Users report "I can't use my SSH key" or "it always asks for a password".

## Code Examples

Verified patterns from official sources:

### SSH2 Session Setup and SFTP File Read
```rust
// Source: docs.rs/ssh2/latest/ssh2/struct.Session.html
use ssh2::Session;
use std::io::Read;
use std::net::TcpStream;

let tcp = TcpStream::connect("host:22").unwrap();
let mut sess = Session::new().unwrap();
sess.set_tcp_stream(tcp);
sess.handshake().unwrap();

// Auth: try agent first, then key file
sess.userauth_agent("username")
    .or_else(|_| sess.userauth_pubkey_file(
        "username",
        None, // public key (auto-derived from private)
        std::path::Path::new("/home/user/.ssh/id_rsa"),
        None, // passphrase
    ))
    .unwrap();

let sftp = sess.sftp().unwrap();

// Read a file
let mut file = sftp.open(std::path::Path::new("/remote/file.txt")).unwrap();
let mut contents = Vec::new();
file.read_to_end(&mut contents).unwrap();

// Stat a file
let stat = sftp.stat(std::path::Path::new("/remote/file.txt")).unwrap();
println!("size: {:?}, is_dir: {}", stat.size, stat.is_dir());

// List directory
let entries = sftp.readdir(std::path::Path::new("/remote/dir")).unwrap();
for (path, stat) in entries {
    println!("{}: size={:?}", path.display(), stat.size);
}

// Write a file
let mut remote_file = sftp.create(std::path::Path::new("/remote/output.txt")).unwrap();
remote_file.write_all(b"Hello from Flux").unwrap();

// Create directory
sftp.mkdir(std::path::Path::new("/remote/newdir"), 0o755).unwrap();
```

### Pavao SMB Client (Linux/macOS)
```rust
// Source: docs.rs/pavao/latest/pavao/struct.SmbClient.html
use pavao::{SmbClient, SmbCredentials, SmbOptions, SmbOpenOptions};
use std::io::{Read, Write};

let client = SmbClient::new(
    SmbCredentials::default()
        .server("smb://server")
        .share("/sharename")
        .username("user")
        .password("pass")
        .workgroup("WORKGROUP"),
    SmbOptions::default().one_share_per_server(true),
).unwrap();

// Read a file (note: SmbFile is !Send, must read fully here)
let mut file = client.open_with(
    "/path/to/file.txt",
    SmbOpenOptions::default().read(true),
).unwrap();
let mut contents = Vec::new();
file.read_to_end(&mut contents).unwrap();

// Stat a file
let metadata = client.stat("/path/to/file.txt").unwrap();

// List directory
let entries = client.list_dir("/path/to/dir").unwrap();
```

### Windows SMB via sambrs + std::fs
```rust
// Source: docs.rs/sambrs
#[cfg(windows)]
{
    use sambrs::SmbShare;

    // Connect the share (maps to a drive letter or UNC access)
    let share = SmbShare::new(r"\\server\share", "user", "pass", None);
    share.connect(false, false).unwrap();

    // Now use standard fs operations with UNC paths
    let contents = std::fs::read(r"\\server\share\file.txt").unwrap();
    let entries = std::fs::read_dir(r"\\server\share\dir").unwrap();
    let metadata = std::fs::metadata(r"\\server\share\file.txt").unwrap();
}
```

### WebDAV via reqwest_dav (Async Bridged to Sync)
```rust
// Source: github.com/niuhuan/reqwest_dav + tokio.rs/bridging
use reqwest_dav::{Auth, ClientBuilder, Depth};
use tokio::runtime::Runtime;

let rt = Runtime::new().unwrap();
let client = ClientBuilder::new()
    .set_host("https://server/webdav/".to_string())
    .set_auth(Auth::Basic("user".to_owned(), "pass".to_owned()))
    .build()
    .unwrap();

// List directory (sync wrapper)
let items = rt.block_on(async {
    client.list("/path/", Depth::Number(1)).await
}).unwrap();

// Download file (sync wrapper)
let response = rt.block_on(async {
    client.get("/path/file.txt").await
}).unwrap();
let bytes = rt.block_on(async {
    response.bytes().await
}).unwrap();

// Upload file (sync wrapper)
let data: Vec<u8> = std::fs::read("local_file.txt").unwrap();
rt.block_on(async {
    client.put("/remote/file.txt", data).await
}).unwrap();

// Create directory
rt.block_on(async {
    client.mkcol("/remote/newdir/").await
}).unwrap();
```

### Protocol Auto-Detection
```rust
// Source: custom implementation pattern using url crate
use url::Url;

pub fn detect_protocol(input: &str) -> Protocol {
    // Check for Windows UNC paths: \\server\share\...
    if input.starts_with("\\\\") {
        let parts: Vec<&str> = input.trim_start_matches("\\\\")
            .splitn(3, '\\')
            .collect();
        if parts.len() >= 2 {
            return Protocol::Smb {
                server: parts[0].to_string(),
                share: parts[1].to_string(),
                path: parts.get(2).unwrap_or(&"").to_string(),
            };
        }
    }

    // Check for Unix-style UNC: //server/share/...
    if input.starts_with("//") && !input.starts_with("///") {
        // Similar parsing for forward-slash UNC
    }

    // Try URL parsing
    if let Ok(url) = Url::parse(input) {
        match url.scheme() {
            "sftp" | "ssh" => {
                return Protocol::Sftp {
                    user: url.username().to_string(),
                    host: url.host_str().unwrap_or("").to_string(),
                    port: url.port().unwrap_or(22),
                    path: url.path().to_string(),
                };
            }
            "smb" => {
                // smb://server/share/path
                return Protocol::Smb {
                    server: url.host_str().unwrap_or("").to_string(),
                    share: /* first path segment */,
                    path: /* remaining path */,
                };
            }
            "https" | "http" => {
                return Protocol::WebDav {
                    url: input.to_string(),
                    auth: None,
                };
            }
            _ => {}
        }
    }

    Protocol::Local
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `async_trait` crate for async trait methods | Native async fn in traits (Rust 1.75+) | Dec 2023 | Less overhead, but dyn dispatch still needs `async_trait`; not relevant since we stay sync |
| Hand-rolling SSH protocol | `ssh2` (libssh2 bindings) or `russh` (pure Rust) | Ongoing | `ssh2` is battle-tested; `russh` is newer but async-only |
| Single SMB library | Platform-conditional (Windows native + pavao) | Current | No single Rust crate handles SMB well cross-platform |
| Sync HTTP clients | Async-first (`reqwest`) with optional blocking feature | 2020+ | `reqwest::blocking` exists but `reqwest_dav` is async-only, requiring bridge |

**Deprecated/outdated:**
- `rustydav`: Last updated Oct 2021, appears abandoned. Use `reqwest_dav` instead.
- `smbc` crate: Older libsmbclient wrapper, superseded by `pavao` which has active maintenance.

## Open Questions

1. **WebDAV large file streaming without buffering entire file in memory**
   - What we know: `reqwest_dav::Client::get()` returns a `Response`. We can call `.bytes().await` but that buffers everything. `reqwest::Response` also has `bytes_stream()` for chunked reading.
   - What's unclear: How to pipe an async byte stream into a sync `Box<dyn Read>` without buffering the whole file. May need a channel-based adapter or temp file approach.
   - Recommendation: Start with full-buffer approach (works for files up to ~100MB). Add streaming for large files as a follow-up if needed. Document the limitation.

2. **SMB on Linux/macOS without libsmbclient installed**
   - What we know: `pavao` has a `vendored` feature that statically builds libsmbclient. However, libsmbclient is "a bloated library with tons of dependencies" (from pavao docs). Build times will increase significantly.
   - What's unclear: Exact build time impact and binary size increase. Whether all pavao dependencies compile on all platforms.
   - Recommendation: Make SMB support a Cargo feature flag (`smb` feature). Users who don't need SMB skip the build cost. Default to enabled on Windows (native), optional on Linux/macOS.

3. **SSH authentication UX: password prompts, key selection, known hosts**
   - What we know: `ssh2` supports agent auth, key file auth, and password auth. It also supports host key checking.
   - What's unclear: What UX to provide for password prompts (stdin? a dialog?), how to handle unknown host keys (trust on first use? strict?), how to find the right key file.
   - Recommendation: For Phase 3, implement a simple auth cascade: SSH agent -> default key files -> password prompt via `rpassword` crate. Known hosts checking can be relaxed (warn but continue). Full host key management deferred to Phase 5 (Security).

4. **WebDAV path semantics: when is https:// WebDAV vs regular HTTPS?**
   - What we know: WebDAV is just HTTP with extra methods (PROPFIND, PUT, MKCOL). Any HTTPS URL could be WebDAV.
   - What's unclear: How to distinguish "user wants to download a web page" from "user wants to access a WebDAV server" when both use `https://`.
   - Recommendation: Treat all `https://` and `http://` destinations as WebDAV. If WebDAV-specific operations fail (PROPFIND returns 405), fall back to plain HTTP GET/PUT. Users can also use `webdav://` or `dav://` scheme prefix for explicit WebDAV.

5. **Credential storage and management**
   - What we know: Users will need to provide credentials for SFTP (key/password), SMB (user/password), WebDAV (user/password).
   - What's unclear: Whether to store credentials, where, and how securely.
   - Recommendation: Phase 3 supports inline credentials (URL-embedded or prompted) only. Credential storage deferred to Phase 4 (configuration) or Phase 5 (security).

## Sources

### Primary (HIGH confidence)
- [ssh2 crate docs](https://docs.rs/ssh2/latest/ssh2/) - Sftp struct methods, File trait impls (Read, Write, Send, Sync)
- [ssh2 on lib.rs](https://lib.rs/crates/ssh2) - Version 0.9.5, 181K downloads/month, release date Feb 2025
- [pavao docs](https://docs.rs/pavao/latest/pavao/) - SmbClient methods, SmbFile traits (Read, Write, !Send, !Sync)
- [pavao on lib.rs](https://lib.rs/crates/pavao) - Version 0.2.16, Dec 2025, requires libsmbclient or vendored
- [reqwest_dav on lib.rs](https://lib.rs/crates/reqwest_dav) - Version 0.3.2, Feb 2026, 56K downloads/month
- [Tokio bridging docs](https://tokio.rs/tokio/topics/bridging) - Runtime::block_on pattern for sync-over-async
- [smb crate on lib.rs](https://lib.rs/crates/smb) - Version 0.11.1, Dec 2025, pure Rust but async-only

### Secondary (MEDIUM confidence)
- [reqwest_dav GitHub](https://github.com/niuhuan/reqwest_dav) - Client methods: get, put, list, mkcol, delete, mv, cp
- [sambrs docs](https://docs.rs/sambrs/latest/sambrs/) - Windows SMB share connection wrapper, WNetAddConnection2A
- [ssh2-rs GitHub](https://github.com/alexcrichton/ssh2-rs) - vendored-openssl feature for Windows builds
- [smb-rs GitHub](https://github.com/afiffon/smb-rs) - 66 stars, 13 open issues, async API with ReadAt/WriteAt

### Tertiary (LOW confidence)
- [rustydav on lib.rs](https://lib.rs/crates/rustydav) - Confirmed abandoned (last update Oct 2021)
- [Greptime blog on bridging](https://greptime.com/blogs/2023-03-09-bridging-async-and-sync-rust) - block_on pitfalls (panics if called inside async context)

## Metadata

**Confidence breakdown:**
- Standard stack: MEDIUM-HIGH - ssh2 and reqwest_dav are well-established; pavao is adequate but the `!Send` constraint on SmbFile requires workarounds; the pure Rust `smb` crate is promising but immature
- Architecture: MEDIUM - Keeping sync trait is the right call given codebase investment, but the pavao `!Send` issue and WebDAV memory buffering need careful implementation
- Pitfalls: HIGH - Well-documented issues from library docs and community reports; the parallel-chunked-over-network pitfall is critical to avoid

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (30 days - these libraries are relatively stable)
