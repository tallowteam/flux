use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FluxError {
    #[error("Source not found: {}", path.display())]
    SourceNotFound { path: PathBuf },

    #[error("Destination not writable: {}", path.display())]
    DestinationNotWritable { path: PathBuf },

    #[error("Permission denied: {}", path.display())]
    PermissionDenied { path: PathBuf },

    #[error("Source is a directory, use -r flag for recursive copy")]
    IsDirectory { path: PathBuf },

    #[error("Invalid glob pattern '{pattern}': {reason}")]
    InvalidPattern { pattern: String, reason: String },

    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Destination is inside source directory: {} -> {}", src.display(), dst.display())]
    DestinationIsSubdirectory { src: PathBuf, dst: PathBuf },

    #[error("Checksum mismatch for {}: expected {expected}, got {actual}", path.display())]
    ChecksumMismatch {
        path: PathBuf,
        expected: String,
        actual: String,
    },

    #[error("Resume error: {0}")]
    ResumeError(String),

    #[error("Compression error: {0}")]
    CompressionError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Connection failed to {protocol}://{host}: {reason}")]
    ConnectionFailed {
        protocol: String,
        host: String,
        reason: String,
    },

    #[error("Alias error: {0}")]
    AliasError(String),

    #[error("Queue error: {0}")]
    QueueError(String),
}

impl FluxError {
    /// Returns a user-friendly suggestion for how to fix the error.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            FluxError::SourceNotFound { .. } => {
                Some("Check the path exists and spelling is correct.")
            }
            FluxError::PermissionDenied { .. } => {
                Some("Try running with elevated privileges, or check file permissions.")
            }
            FluxError::IsDirectory { .. } => {
                Some("Use 'flux cp -r <source> <dest>' for directory copies.")
            }
            FluxError::DestinationNotWritable { .. } => {
                Some("Check that the destination directory exists and you have write permission.")
            }
            FluxError::InvalidPattern { .. } => {
                Some("Check glob syntax. Examples: '*.log', '**/*.tmp', 'build/'")
            }
            FluxError::DestinationIsSubdirectory { .. } => {
                Some("Choose a destination outside the source directory.")
            }
            FluxError::ChecksumMismatch { .. } => {
                Some("The file may be corrupted. Try re-transferring.")
            }
            FluxError::ResumeError(_) => {
                Some("Delete the .flux-resume.json manifest file and restart the transfer.")
            }
            FluxError::ProtocolError(_) => {
                Some("Check the URL format. Examples: sftp://user@host/path, \\\\server\\share, https://server/webdav/")
            }
            FluxError::ConnectionFailed { .. } => {
                Some("Check that the host is reachable and the port is correct.")
            }
            FluxError::AliasError(_) => {
                Some("Check alias name with `flux alias`.")
            }
            FluxError::QueueError(_) => {
                Some("Check queue status with `flux queue`.")
            }
            _ => None,
        }
    }
}

impl From<globset::Error> for FluxError {
    fn from(err: globset::Error) -> Self {
        FluxError::InvalidPattern {
            pattern: err.glob().map(|g| g.to_string()).unwrap_or_default(),
            reason: err.kind().to_string(),
        }
    }
}

impl From<walkdir::Error> for FluxError {
    fn from(err: walkdir::Error) -> Self {
        if let Some(path) = err.path() {
            // walkdir errors can be permission errors or I/O errors
            if let Some(inner) = err.io_error() {
                if inner.kind() == std::io::ErrorKind::PermissionDenied {
                    return FluxError::PermissionDenied {
                        path: path.to_path_buf(),
                    };
                }
            }
        }
        // Fall back to I/O error
        FluxError::Io {
            source: err.into_io_error().unwrap_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::Other, "walkdir error")
            }),
        }
    }
}

impl From<serde_json::Error> for FluxError {
    fn from(err: serde_json::Error) -> Self {
        FluxError::Config(err.to_string())
    }
}

impl From<toml::ser::Error> for FluxError {
    fn from(err: toml::ser::Error) -> Self {
        FluxError::Config(format!("TOML serialization error: {}", err))
    }
}

impl From<std::path::StripPrefixError> for FluxError {
    fn from(err: std::path::StripPrefixError) -> Self {
        FluxError::Io {
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, err),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn source_not_found_display_and_suggestion() {
        let err = FluxError::SourceNotFound {
            path: PathBuf::from("/tmp/missing.txt"),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Source not found"));
        assert!(msg.contains("missing.txt"));
        assert_eq!(
            err.suggestion(),
            Some("Check the path exists and spelling is correct.")
        );
    }

    #[test]
    fn permission_denied_suggestion() {
        let err = FluxError::PermissionDenied {
            path: PathBuf::from("/root/secret"),
        };
        assert_eq!(
            err.suggestion(),
            Some("Try running with elevated privileges, or check file permissions.")
        );
    }

    #[test]
    fn is_directory_display_and_suggestion() {
        let err = FluxError::IsDirectory {
            path: PathBuf::from("/tmp/mydir"),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("use -r flag"));
        assert_eq!(
            err.suggestion(),
            Some("Use 'flux cp -r <source> <dest>' for directory copies.")
        );
    }

    #[test]
    fn destination_not_writable_suggestion() {
        let err = FluxError::DestinationNotWritable {
            path: PathBuf::from("/readonly/dir"),
        };
        assert_eq!(
            err.suggestion(),
            Some("Check that the destination directory exists and you have write permission.")
        );
    }

    #[test]
    fn invalid_pattern_suggestion() {
        let err = FluxError::InvalidPattern {
            pattern: "[invalid".to_string(),
            reason: "unclosed bracket".to_string(),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("[invalid"));
        assert!(msg.contains("unclosed bracket"));
        assert_eq!(
            err.suggestion(),
            Some("Check glob syntax. Examples: '*.log', '**/*.tmp', 'build/'")
        );
    }

    #[test]
    fn destination_is_subdirectory_suggestion() {
        let err = FluxError::DestinationIsSubdirectory {
            src: PathBuf::from("/home/user/project"),
            dst: PathBuf::from("/home/user/project/backup"),
        };
        let msg = format!("{}", err);
        assert!(msg.contains("Destination is inside source directory"));
        assert_eq!(
            err.suggestion(),
            Some("Choose a destination outside the source directory.")
        );
    }

    #[test]
    fn io_error_no_suggestion() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file gone");
        let err: FluxError = io_err.into();
        assert!(err.suggestion().is_none());
    }

    #[test]
    fn config_error_no_suggestion() {
        let err = FluxError::Config("bad toml".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Configuration error"));
        assert!(msg.contains("bad toml"));
        assert!(err.suggestion().is_none());
    }

    #[test]
    fn from_strip_prefix_error() {
        let path = PathBuf::from("/a/b");
        let result = path.strip_prefix("/c/d");
        assert!(result.is_err());
        let err: FluxError = result.unwrap_err().into();
        match err {
            FluxError::Io { .. } => {} // expected
            other => panic!("Expected Io variant, got: {:?}", other),
        }
    }
}
