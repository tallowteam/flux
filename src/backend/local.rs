use std::io::{BufReader, BufWriter};
use std::path::Path;

use crate::backend::{BackendFeatures, FileEntry, FileStat, FluxBackend};
use crate::error::FluxError;

/// Buffer size for BufReader/BufWriter: 256KB.
const BUF_SIZE: usize = 256 * 1024;

/// Local filesystem backend using std::fs.
pub struct LocalBackend;

impl LocalBackend {
    pub fn new() -> Self {
        LocalBackend
    }
}

impl Default for LocalBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Convert std::fs::Metadata to FileStat.
fn metadata_to_stat(meta: &std::fs::Metadata) -> FileStat {
    let modified = meta.modified().ok();

    #[cfg(unix)]
    let permissions = {
        use std::os::unix::fs::PermissionsExt;
        Some(meta.permissions().mode())
    };

    #[cfg(not(unix))]
    let permissions = None;

    FileStat {
        size: meta.len(),
        is_dir: meta.is_dir(),
        is_file: meta.is_file(),
        modified,
        permissions,
    }
}

/// Map an io::Error to an appropriate FluxError, using the path for context.
fn map_io_error(err: std::io::Error, path: &Path, context: IoContext) -> FluxError {
    match err.kind() {
        std::io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: path.to_path_buf(),
        },
        std::io::ErrorKind::PermissionDenied => match context {
            IoContext::Read | IoContext::Stat | IoContext::ListDir => FluxError::PermissionDenied {
                path: path.to_path_buf(),
            },
            IoContext::Write | IoContext::CreateDir => FluxError::DestinationNotWritable {
                path: path.to_path_buf(),
            },
        },
        _ => FluxError::Io { source: err },
    }
}

/// Context for which operation caused the io::Error.
enum IoContext {
    Read,
    Write,
    Stat,
    ListDir,
    CreateDir,
}

impl FluxBackend for LocalBackend {
    fn stat(&self, path: &Path) -> Result<FileStat, FluxError> {
        let meta = std::fs::metadata(path).map_err(|e| map_io_error(e, path, IoContext::Stat))?;
        Ok(metadata_to_stat(&meta))
    }

    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, FluxError> {
        let read_dir =
            std::fs::read_dir(path).map_err(|e| map_io_error(e, path, IoContext::ListDir))?;

        let mut entries = Vec::new();
        for entry_result in read_dir {
            let entry =
                entry_result.map_err(|e| map_io_error(e, path, IoContext::ListDir))?;
            let entry_path = entry.path();
            let meta = entry
                .metadata()
                .map_err(|e| map_io_error(e, &entry_path, IoContext::Stat))?;

            entries.push(FileEntry {
                path: entry_path,
                stat: metadata_to_stat(&meta),
            });
        }

        Ok(entries)
    }

    fn open_read(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>, FluxError> {
        let file =
            std::fs::File::open(path).map_err(|e| map_io_error(e, path, IoContext::Read))?;
        Ok(Box::new(BufReader::with_capacity(BUF_SIZE, file)))
    }

    fn open_write(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, FluxError> {
        let file =
            std::fs::File::create(path).map_err(|e| map_io_error(e, path, IoContext::Write))?;
        Ok(Box::new(BufWriter::with_capacity(BUF_SIZE, file)))
    }

    fn create_dir_all(&self, path: &Path) -> Result<(), FluxError> {
        std::fs::create_dir_all(path)
            .map_err(|e| map_io_error(e, path, IoContext::CreateDir))?;
        Ok(())
    }

    fn features(&self) -> BackendFeatures {
        BackendFeatures {
            supports_seek: true,
            supports_parallel: true,
            supports_permissions: cfg!(unix),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stat_existing_file() {
        let backend = LocalBackend::new();
        // Use the current source file as a known existing file
        let path = std::path::Path::new(file!());
        // file!() returns a relative path from the project root
        // We need an absolute path, so use the manifest dir
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let abs_path = manifest_dir.join(path);

        let stat = backend.stat(&abs_path).expect("stat should succeed on source file");
        assert!(stat.is_file);
        assert!(!stat.is_dir);
        assert!(stat.size > 0);
        assert!(stat.modified.is_some());
    }

    #[test]
    fn stat_nonexistent_returns_source_not_found() {
        let backend = LocalBackend::new();
        let path = std::path::Path::new("/nonexistent/path/that/does/not/exist.txt");
        let result = backend.stat(path);
        assert!(result.is_err());
        match result.unwrap_err() {
            FluxError::SourceNotFound { .. } => {} // expected
            other => panic!("Expected SourceNotFound, got: {:?}", other),
        }
    }

    #[test]
    fn open_read_existing_file() {
        use std::io::Read;

        let backend = LocalBackend::new();
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let abs_path = manifest_dir.join(file!());

        let mut reader = backend.open_read(&abs_path).expect("open_read should succeed");
        let mut buf = [0u8; 16];
        let bytes_read = reader.read(&mut buf).expect("read should succeed");
        assert!(bytes_read > 0);
    }

    #[test]
    fn open_read_nonexistent_returns_source_not_found() {
        let backend = LocalBackend::new();
        let path = std::path::Path::new("/nonexistent/file.txt");
        let result = backend.open_read(path);
        assert!(result.is_err());
        match result {
            Err(FluxError::SourceNotFound { .. }) => {} // expected
            Err(other) => panic!("Expected SourceNotFound, got: {:?}", other),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }

    #[test]
    fn features_reports_local_capabilities() {
        let backend = LocalBackend::new();
        let features = backend.features();
        assert!(features.supports_seek);
        assert!(features.supports_parallel);
        // permissions support depends on platform
        #[cfg(unix)]
        assert!(features.supports_permissions);
        #[cfg(not(unix))]
        assert!(!features.supports_permissions);
    }

    #[test]
    fn list_dir_on_project_root() {
        let backend = LocalBackend::new();
        let manifest_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let entries = backend.list_dir(manifest_dir).expect("list_dir should succeed");
        // Project root should have at least Cargo.toml and src/
        let names: Vec<String> = entries
            .iter()
            .filter_map(|e| e.path.file_name().map(|n| n.to_string_lossy().to_string()))
            .collect();
        assert!(names.contains(&"Cargo.toml".to_string()));
        assert!(names.contains(&"src".to_string()));
    }
}
