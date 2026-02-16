pub mod checksum;
pub mod chunk;
pub mod compress;
pub mod copy;
pub mod filter;
pub mod parallel;
pub mod resume;
pub mod throttle;

use std::path::{Path, PathBuf};

use indicatif::ProgressBar;
use walkdir::WalkDir;

use crate::cli::args::CpArgs;
use crate::error::FluxError;
use crate::progress::bar::{create_directory_progress, create_file_progress};

use self::copy::copy_file_with_progress;
use self::filter::TransferFilter;

/// Aggregated result of a directory copy operation.
///
/// Tracks successful file copies and collects per-file errors so that
/// individual failures don't abort the entire directory copy.
pub struct TransferResult {
    pub files_copied: u64,
    pub bytes_copied: u64,
    pub errors: Vec<(PathBuf, FluxError)>,
}

impl TransferResult {
    pub fn new() -> Self {
        Self {
            files_copied: 0,
            bytes_copied: 0,
            errors: Vec::new(),
        }
    }

    pub fn add_success(&mut self, bytes: u64) {
        self.files_copied += 1;
        self.bytes_copied += bytes;
    }

    pub fn add_error(&mut self, path: PathBuf, err: FluxError) {
        self.errors.push((path, err));
    }
}

/// Execute a copy command based on parsed CLI arguments.
///
/// Validates inputs, creates a TransferFilter from --exclude/--include args,
/// then dispatches to single-file or directory copy.
pub fn execute_copy(args: CpArgs, quiet: bool) -> Result<(), FluxError> {
    let source = &args.source;
    let dest = &args.dest;

    // Build the filter from CLI patterns
    let filter = TransferFilter::new(&args.exclude, &args.include)?;

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
        // For single file: check if filter excludes it
        if !filter.should_transfer(source) {
            tracing::info!(
                "Skipped {} (excluded by filter)",
                source.display()
            );
            return Ok(());
        }

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

        let bytes = copy_file_with_progress(source, &final_dest, &progress)?;

        tracing::info!("Copied {} bytes", bytes);

        Ok(())
    } else if source_meta.is_dir() {
        // Directory copy with filtering
        let result = copy_directory(source, dest, &filter, quiet)?;

        tracing::info!(
            "Copied {} file(s), {} bytes",
            result.files_copied,
            result.bytes_copied
        );

        if !result.errors.is_empty() {
            // Report errors to stderr
            if !quiet {
                eprintln!(
                    "Completed with {} error(s):",
                    result.errors.len()
                );
                for (path, err) in &result.errors {
                    eprintln!("  {}: {}", path.display(), err);
                }
            }
            // Return an error summarizing the failures
            return Err(FluxError::Io {
                source: std::io::Error::new(
                    std::io::ErrorKind::Other,
                    format!(
                        "{} file(s) failed to copy",
                        result.errors.len()
                    ),
                ),
            });
        }

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

/// Copy a directory recursively with filtering and progress.
///
/// Trailing slash semantics (rsync convention):
/// - Source path ends with `/` or `\`: copy CONTENTS of source into dest
/// - Source path has no trailing separator: copy source directory itself into dest
///   (creates dest/source_dirname/)
///
/// Individual file errors are collected in TransferResult, not fatal.
/// Progress bar tracks file count (not bytes).
fn copy_directory(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    quiet: bool,
) -> Result<TransferResult, FluxError> {
    // Detect trailing slash before normalizing the path
    let source_str = source.to_string_lossy();
    let has_trailing_slash = source_str.ends_with('/') || source_str.ends_with('\\');

    // Normalize: remove trailing separator for walkdir (it needs a clean path)
    let source_clean = if has_trailing_slash {
        let trimmed = source_str.trim_end_matches(|c| c == '/' || c == '\\');
        PathBuf::from(trimmed)
    } else {
        source.to_path_buf()
    };

    // Determine the base destination path
    let dest_base = if has_trailing_slash {
        // Trailing slash: copy contents directly into dest
        dest.to_path_buf()
    } else {
        // No trailing slash: copy directory itself into dest
        // e.g., source="mydir", dest="/tmp/out" -> "/tmp/out/mydir/"
        if let Some(dir_name) = source_clean.file_name() {
            dest.join(dir_name)
        } else {
            dest.to_path_buf()
        }
    };

    // Validate: dest must not be inside source to avoid infinite recursion
    if let (Ok(canon_src), Ok(canon_dst)) = (
        std::fs::canonicalize(&source_clean),
        canonicalize_best_effort(&dest_base),
    ) {
        if canon_dst.starts_with(&canon_src) {
            return Err(FluxError::DestinationIsSubdirectory {
                src: source_clean.clone(),
                dst: dest_base.clone(),
            });
        }
    }

    // First pass: count files for progress bar total
    let file_count = WalkDir::new(&source_clean)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| filter.should_transfer(e.path()))
        .count() as u64;

    let progress = create_directory_progress(file_count, quiet);
    let mut result = TransferResult::new();

    // Second pass: actual copy
    for entry in WalkDir::new(&source_clean)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => {
                // walkdir error (e.g., permission denied on directory)
                let path = err
                    .path()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| source_clean.clone());
                result.add_error(path, FluxError::from(err));
                continue;
            }
        };

        let relative = entry.path().strip_prefix(&source_clean)?;

        // Skip the root entry itself (relative path is empty)
        if relative.as_os_str().is_empty() {
            continue;
        }

        let dest_path = dest_base.join(relative);

        if entry.file_type().is_dir() {
            // Create directory structure in destination
            if let Err(e) = std::fs::create_dir_all(&dest_path) {
                result.add_error(
                    entry.path().to_path_buf(),
                    FluxError::Io { source: e },
                );
            }
        } else if entry.file_type().is_file() {
            if !filter.should_transfer(entry.path()) {
                continue;
            }

            // Ensure parent directory exists
            if let Some(parent) = dest_path.parent() {
                if !parent.exists() {
                    if let Err(e) = std::fs::create_dir_all(parent) {
                        result.add_error(
                            entry.path().to_path_buf(),
                            FluxError::Io { source: e },
                        );
                        progress.inc(1);
                        continue;
                    }
                }
            }

            // Use a hidden progress bar for per-file copy (directory progress tracks file count)
            let file_progress = ProgressBar::hidden();

            match copy_file_with_progress(entry.path(), &dest_path, &file_progress) {
                Ok(bytes) => {
                    result.add_success(bytes);
                }
                Err(e) => {
                    result.add_error(entry.path().to_path_buf(), e);
                }
            }
            progress.inc(1);
        }
    }

    progress.finish_with_message("done");
    Ok(result)
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
