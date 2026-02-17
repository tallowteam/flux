pub mod local;
// pub mod sftp;  // Temporarily disabled: ssh2 cannot build on this machine (missing Perl/OpenSSL)
pub mod smb;
pub mod webdav;

use std::path::Path;

use crate::error::FluxError;
use crate::protocol::Protocol;

/// Metadata about a file or directory.
#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub is_dir: bool,
    pub is_file: bool,
    pub modified: Option<std::time::SystemTime>,
    pub permissions: Option<u32>,
}

/// Entry in a directory listing.
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: std::path::PathBuf,
    pub stat: FileStat,
}

/// What capabilities a backend supports.
#[derive(Debug, Clone)]
pub struct BackendFeatures {
    pub supports_seek: bool,
    pub supports_parallel: bool,
    pub supports_permissions: bool,
}

/// Core abstraction for all file backends.
///
/// Phase 1 implements LocalBackend only.
/// Future phases add SftpBackend, SmbBackend, WebDavBackend.
///
/// Synchronous trait -- local file I/O is inherently blocking.
/// Will evolve to async with `#[async_trait]` when network backends arrive (Phase 3).
pub trait FluxBackend: Send + Sync {
    /// Get file/directory metadata.
    fn stat(&self, path: &Path) -> Result<FileStat, crate::error::FluxError>;

    /// List directory contents (non-recursive).
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, crate::error::FluxError>;

    /// Open a file for reading, returns a boxed Read.
    fn open_read(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>, crate::error::FluxError>;

    /// Create/open a file for writing, returns a boxed Write.
    fn open_write(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, crate::error::FluxError>;

    /// Create directory (and parents if needed).
    fn create_dir_all(&self, path: &Path) -> Result<(), crate::error::FluxError>;

    /// Check backend capabilities.
    fn features(&self) -> BackendFeatures;
}

/// Create the appropriate backend for a detected protocol.
///
/// Returns `LocalBackend` for local paths, `SftpBackend` for SFTP,
/// `SmbBackend` for SMB, and `WebDavBackend` for WebDAV.
pub fn create_backend(protocol: &Protocol) -> Result<Box<dyn FluxBackend>, FluxError> {
    match protocol {
        Protocol::Local { .. } => Ok(Box::new(local::LocalBackend::new())),
        // SFTP temporarily disabled: ssh2 cannot build on this machine (missing Perl/OpenSSL)
        Protocol::Sftp { .. } => Err(FluxError::ProtocolError(
            "SFTP backend not available: ssh2 dependency cannot build on this system".to_string(),
        )),
        Protocol::Smb {
            server, share, ..
        } => {
            let backend = smb::SmbBackend::connect(server, share)?;
            Ok(Box::new(backend))
        }
        Protocol::WebDav { url, auth } => {
            let backend = webdav::WebDavBackend::new(url, auth.clone())?;
            Ok(Box::new(backend))
        }
    }
}
