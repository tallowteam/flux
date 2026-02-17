//! SMB/CIFS network share backend.
//!
//! Platform-conditional implementation:
//! - **Windows:** Uses native UNC paths via `std::fs`. Windows natively supports
//!   `\\server\share\path` access through the OS SMB client, so SmbBackend
//!   constructs UNC paths and delegates to standard filesystem operations.
//! - **Non-Windows:** Returns a clear error message directing users to build
//!   with the `smb` feature flag (requires libsmbclient).

use std::path::{Path, PathBuf};

use crate::backend::{BackendFeatures, FileEntry, FileStat, FluxBackend};
use crate::error::FluxError;

/// SMB/CIFS backend for accessing network shares.
///
/// On Windows, this backend uses UNC paths (`\\server\share\path`) which are
/// natively supported by the Windows OS. All operations delegate to `std::fs`.
///
/// On non-Windows platforms, SMB support requires the `smb` feature flag
/// and libsmbclient. Without it, all operations return a `ProtocolError`.
#[derive(Debug)]
pub struct SmbBackend {
    /// The base UNC path to the share root, e.g. `\\server\share`.
    #[cfg(windows)]
    base_unc: PathBuf,

    /// Placeholder field for non-Windows (struct must have at least one field
    /// or be constructed differently per platform).
    #[cfg(not(windows))]
    _phantom: (),
}

// ─── Windows implementation ────────────────────────────────────────────

#[cfg(windows)]
impl SmbBackend {
    /// Connect to an SMB share on Windows using native UNC path access.
    ///
    /// Constructs the UNC path `\\server\share` and relies on the Windows OS
    /// to handle authentication (using the current user's session or cached
    /// credentials).
    ///
    /// # Arguments
    /// * `server` - The SMB server hostname or IP address.
    /// * `share` - The share name on the server.
    pub fn connect(server: &str, share: &str) -> Result<Self, FluxError> {
        if server.is_empty() {
            return Err(FluxError::ProtocolError(
                "SMB server name cannot be empty".to_string(),
            ));
        }
        if share.is_empty() {
            return Err(FluxError::ProtocolError(
                "SMB share name cannot be empty".to_string(),
            ));
        }

        let base_unc = PathBuf::from(format!("\\\\{}\\{}", server, share));
        Ok(SmbBackend { base_unc })
    }

    /// Resolve a relative path against the base UNC path.
    ///
    /// Joins `self.base_unc` with the provided relative path, producing
    /// a full UNC path like `\\server\share\subdir\file.txt`.
    fn resolve(&self, path: &Path) -> PathBuf {
        if path.as_os_str().is_empty() {
            self.base_unc.clone()
        } else {
            self.base_unc.join(path)
        }
    }
}

/// Buffer size for BufReader/BufWriter: 256KB (matching LocalBackend).
#[cfg(windows)]
const BUF_SIZE: usize = 256 * 1024;

#[cfg(windows)]
impl FluxBackend for SmbBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let full_path = self.resolve(path);
        let meta = std::fs::metadata(&full_path).map_err(|e| map_smb_io_error(e, &full_path))?;
        Ok(metadata_to_stat(&meta))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, FluxError> {
        let full_path = self.resolve(path);
        let read_dir =
            std::fs::read_dir(&full_path).map_err(|e| map_smb_io_error(e, &full_path))?;

        let mut entries = Vec::new();
        for entry_result in read_dir {
            let entry = entry_result.map_err(|e| map_smb_io_error(e, &full_path))?;
            let entry_path = entry.path();
            let meta = entry
                .metadata()
                .map_err(|e| map_smb_io_error(e, &entry_path))?;
            entries.push(FileEntry {
                path: entry_path,
                stat: metadata_to_stat(&meta),
            });
        }
        Ok(entries)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>, FluxError> {
        let full_path = self.resolve(path);
        let file =
            std::fs::File::open(&full_path).map_err(|e| map_smb_io_error(e, &full_path))?;
        Ok(Box::new(std::io::BufReader::with_capacity(BUF_SIZE, file)))
    }

    fn open_write(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, FluxError> {
        let full_path = self.resolve(path);
        let file =
            std::fs::File::create(&full_path).map_err(|e| map_smb_io_error(e, &full_path))?;
        Ok(Box::new(std::io::BufWriter::with_capacity(BUF_SIZE, file)))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FluxError> {
        let full_path = self.resolve(path);
        std::fs::create_dir_all(&full_path).map_err(|e| map_smb_io_error(e, &full_path))?;
        Ok(())
    }

    fn features(&self) -> BackendFeatures {
        BackendFeatures {
            // Windows UNC paths are accessed through the OS SMB client which
            // handles caching and streaming. Parallel chunked I/O with
            // positional reads is not reliable over network shares.
            supports_seek: false,
            supports_parallel: false,
            // Windows does not expose Unix-style permission bits
            supports_permissions: false,
        }
    }
}

/// Convert std::fs::Metadata to FileStat (Windows SMB variant).
#[cfg(windows)]
fn metadata_to_stat(meta: &std::fs::Metadata) -> FileStat {
    FileStat {
        size: meta.len(),
        is_dir: meta.is_dir(),
        is_file: meta.is_file(),
        modified: meta.modified().ok(),
        permissions: None, // Windows does not use Unix permission bits
    }
}

/// Map I/O errors to FluxError with SMB-specific context.
#[cfg(windows)]
fn map_smb_io_error(err: std::io::Error, path: &Path) -> FluxError {
    match err.kind() {
        std::io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: path.to_path_buf(),
        },
        std::io::ErrorKind::PermissionDenied => FluxError::PermissionDenied {
            path: path.to_path_buf(),
        },
        _ => FluxError::ProtocolError(format!("SMB error accessing {}: {}", path.display(), err)),
    }
}

// ─── Non-Windows stub implementation ───────────────────────────────────

#[cfg(not(windows))]
impl SmbBackend {
    /// Attempt to connect to an SMB share on non-Windows.
    ///
    /// Always returns an error directing users to build with the `smb` feature
    /// flag or use a Windows host for native SMB support.
    pub fn connect(_server: &str, _share: &str) -> Result<Self, FluxError> {
        Err(FluxError::ProtocolError(
            "SMB support on Linux/macOS requires the 'smb' feature flag. \
             Rebuild with: cargo build --features smb\n\
             Alternatively, mount the SMB share with your OS and use a local path."
                .to_string(),
        ))
    }
}

#[cfg(not(windows))]
impl FluxBackend for SmbBackend {
    fn stat(&self, _path: &Path) -> Result<FileStat, FluxError> {
        Err(FluxError::ProtocolError(
            "SMB not available on this platform".to_string(),
        ))
    }

    fn list_dir(&self, _path: &Path) -> Result<Vec<FileEntry>, FluxError> {
        Err(FluxError::ProtocolError(
            "SMB not available on this platform".to_string(),
        ))
    }

    fn open_read(&self, _path: &Path) -> Result<Box<dyn std::io::Read + Send>, FluxError> {
        Err(FluxError::ProtocolError(
            "SMB not available on this platform".to_string(),
        ))
    }

    fn open_write(&self, _path: &Path) -> Result<Box<dyn std::io::Write + Send>, FluxError> {
        Err(FluxError::ProtocolError(
            "SMB not available on this platform".to_string(),
        ))
    }

    fn create_dir_all(&self, _path: &Path) -> Result<(), FluxError> {
        Err(FluxError::ProtocolError(
            "SMB not available on this platform".to_string(),
        ))
    }

    fn features(&self) -> BackendFeatures {
        BackendFeatures {
            supports_seek: false,
            supports_parallel: false,
            supports_permissions: false,
        }
    }
}

// ─── Unit Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    mod windows_tests {
        use super::*;

        #[test]
        fn connect_creates_unc_path() {
            let backend = SmbBackend::connect("myserver", "myshare").unwrap();
            assert_eq!(backend.base_unc, PathBuf::from("\\\\myserver\\myshare"));
        }

        #[test]
        fn connect_empty_server_returns_error() {
            let result = SmbBackend::connect("", "share");
            assert!(result.is_err());
            match result.unwrap_err() {
                FluxError::ProtocolError(msg) => {
                    assert!(msg.contains("server name cannot be empty"));
                }
                other => panic!("Expected ProtocolError, got {:?}", other),
            }
        }

        #[test]
        fn connect_empty_share_returns_error() {
            let result = SmbBackend::connect("server", "");
            assert!(result.is_err());
            match result.unwrap_err() {
                FluxError::ProtocolError(msg) => {
                    assert!(msg.contains("share name cannot be empty"));
                }
                other => panic!("Expected ProtocolError, got {:?}", other),
            }
        }

        #[test]
        fn resolve_empty_path_returns_base() {
            let backend = SmbBackend::connect("server", "share").unwrap();
            let resolved = backend.resolve(Path::new(""));
            assert_eq!(resolved, PathBuf::from("\\\\server\\share"));
        }

        #[test]
        fn resolve_relative_path_joins_correctly() {
            let backend = SmbBackend::connect("server", "share").unwrap();
            let resolved = backend.resolve(Path::new("subdir\\file.txt"));
            assert_eq!(
                resolved,
                PathBuf::from("\\\\server\\share\\subdir\\file.txt")
            );
        }

        #[test]
        fn resolve_nested_path() {
            let backend = SmbBackend::connect("nas", "documents").unwrap();
            let resolved = backend.resolve(Path::new("projects\\2024\\report.pdf"));
            assert_eq!(
                resolved,
                PathBuf::from("\\\\nas\\documents\\projects\\2024\\report.pdf")
            );
        }

        #[test]
        fn features_reports_no_parallel_no_seek() {
            let backend = SmbBackend::connect("server", "share").unwrap();
            let features = backend.features();
            assert!(!features.supports_seek);
            assert!(!features.supports_parallel);
            assert!(!features.supports_permissions);
        }

        #[test]
        fn stat_nonexistent_unc_path_returns_error() {
            let backend =
                SmbBackend::connect("nonexistent-smb-host-12345", "fakeshare").unwrap();
            let result = backend.stat(Path::new("no-such-file.txt"));
            assert!(result.is_err());
        }

        #[test]
        fn resolve_forward_slash_path_works() {
            // Paths from smb:// URL parsing may use forward slashes
            let backend = SmbBackend::connect("server", "share").unwrap();
            let resolved = backend.resolve(Path::new("docs/readme.txt"));
            // On Windows, PathBuf.join normalizes forward slashes to backslashes
            let resolved_str = resolved.to_string_lossy();
            assert!(
                resolved_str.starts_with("\\\\server\\share"),
                "Should start with UNC base, got: {}",
                resolved_str
            );
            assert!(
                resolved_str.contains("readme.txt"),
                "Should contain filename, got: {}",
                resolved_str
            );
        }

        #[test]
        fn resolve_single_file_name() {
            let backend = SmbBackend::connect("fileserver", "data").unwrap();
            let resolved = backend.resolve(Path::new("report.xlsx"));
            assert_eq!(
                resolved,
                PathBuf::from("\\\\fileserver\\data\\report.xlsx")
            );
        }

        #[test]
        fn connect_with_ip_address() {
            let backend = SmbBackend::connect("192.168.1.100", "share$").unwrap();
            assert_eq!(
                backend.base_unc,
                PathBuf::from("\\\\192.168.1.100\\share$")
            );
        }

        /// Verify that create_backend routes Protocol::Smb to SmbBackend.
        #[test]
        fn create_backend_routes_smb_protocol() {
            use crate::backend::create_backend;
            use crate::protocol::Protocol;

            let protocol = Protocol::Smb {
                server: "testserver".to_string(),
                share: "testshare".to_string(),
                path: "file.txt".to_string(),
            };

            let result = create_backend(&protocol);
            assert!(
                result.is_ok(),
                "create_backend should succeed for Smb protocol, got: {:?}",
                result.err()
            );

            let backend = result.unwrap();
            let features = backend.features();
            assert!(!features.supports_parallel);
            assert!(!features.supports_seek);
            assert!(!features.supports_permissions);
        }
    }

    #[cfg(not(windows))]
    mod non_windows_tests {
        use super::*;

        #[test]
        fn connect_returns_protocol_error() {
            let result = SmbBackend::connect("server", "share");
            assert!(result.is_err());
            match result.unwrap_err() {
                FluxError::ProtocolError(msg) => {
                    assert!(msg.contains("smb"));
                    assert!(msg.contains("feature flag"));
                }
                other => panic!("Expected ProtocolError, got {:?}", other),
            }
        }
    }
}
