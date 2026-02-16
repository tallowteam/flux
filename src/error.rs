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

impl From<std::path::StripPrefixError> for FluxError {
    fn from(err: std::path::StripPrefixError) -> Self {
        FluxError::Io {
            source: std::io::Error::new(std::io::ErrorKind::InvalidInput, err),
        }
    }
}
