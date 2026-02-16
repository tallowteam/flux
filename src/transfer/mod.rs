pub mod copy;

use std::path::Path;

use crate::cli::args::CpArgs;
use crate::error::FluxError;
use crate::progress::bar::create_file_progress;

/// Execute a copy command based on parsed CLI arguments.
///
/// Validates inputs, then dispatches to single-file or directory copy.
/// Directory copy is not yet implemented (Plan 03).
pub fn execute_copy(args: CpArgs, quiet: bool) -> Result<(), FluxError> {
    let source = &args.source;
    let dest = &args.dest;

    // Validate: source must exist
    let source_meta = std::fs::metadata(source).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: source.clone(),
        },
        std::io::ErrorKind::PermissionDenied => FluxError::PermissionDenied {
            path: source.clone(),
        },
        _ => FluxError::Io { source: e },
    })?;

    // Validate: if source is a directory, recursive flag must be set
    if source_meta.is_dir() && !args.recursive {
        return Err(FluxError::IsDirectory {
            path: source.clone(),
        });
    }

    // Validate: source != dest (canonicalize to resolve symlinks and relative paths)
    if let (Ok(canon_src), Ok(canon_dst)) = (
        canonicalize_best_effort(source),
        canonicalize_best_effort(dest),
    ) {
        if canon_src == canon_dst {
            return Err(FluxError::Io {
                source: std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "Source and destination are the same file: {}",
                        canon_src.display()
                    ),
                ),
            });
        }
    }

    if source_meta.is_file() {
        // Determine actual destination: if dest is an existing directory,
        // copy into it with the source file name
        let final_dest = if dest.is_dir() {
            if let Some(file_name) = source.file_name() {
                dest.join(file_name)
            } else {
                dest.clone()
            }
        } else {
            dest.clone()
        };

        let size = source_meta.len();
        let progress = create_file_progress(size, quiet);

        let bytes = copy::copy_file_with_progress(source, &final_dest, &progress)?;

        tracing::info!("Copied {} bytes", bytes);

        Ok(())
    } else if source_meta.is_dir() {
        // Directory copy will be implemented in Plan 03
        tracing::info!("Directory copy not yet implemented");
        eprintln!("Directory copy not yet implemented (coming in Plan 03)");
        Ok(())
    } else {
        Err(FluxError::Io {
            source: std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("Source is not a file or directory: {}", source.display()),
            ),
        })
    }
}

/// Attempt to canonicalize a path; if it fails (e.g., path doesn't exist yet),
/// try canonicalizing the parent and appending the file name.
fn canonicalize_best_effort(path: &Path) -> std::io::Result<std::path::PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(_) => {
            // For dest that doesn't exist yet, canonicalize the parent
            if let Some(parent) = path.parent() {
                if let Ok(canon_parent) = std::fs::canonicalize(parent) {
                    if let Some(file_name) = path.file_name() {
                        return Ok(canon_parent.join(file_name));
                    }
                }
            }
            std::fs::canonicalize(path)
        }
    }
}
