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

use crate::backend::create_backend;
use crate::cli::args::CpArgs;
use crate::error::FluxError;
use crate::progress::bar::{create_directory_progress, create_file_progress};
use crate::protocol::detect_protocol;

use self::checksum::hash_file;
use self::chunk::{auto_chunk_count, chunk_file};
use self::copy::copy_file_with_progress;
use self::filter::TransferFilter;
use self::parallel::parallel_copy_chunked;
use self::resume::TransferManifest;
use self::throttle::parse_bandwidth;

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
/// then dispatches to single-file or directory copy. Detects protocol from
/// source and destination strings -- network protocols return stub errors
/// until Phase 3 Plans 02-04 implement them.
pub fn execute_copy(args: CpArgs, quiet: bool) -> Result<(), FluxError> {
    // Detect protocols from source and destination strings
    let src_protocol = detect_protocol(&args.source);
    let dst_protocol = detect_protocol(&args.dest);

    tracing::debug!("Source protocol: {} ({})", src_protocol.name(), args.source);
    tracing::debug!("Dest protocol: {} ({})", dst_protocol.name(), args.dest);

    // For non-local protocols, validate the backend is available (will error with stub message)
    if !src_protocol.is_local() {
        let _backend = create_backend(&src_protocol)?;
    }
    if !dst_protocol.is_local() {
        let _backend = create_backend(&dst_protocol)?;
    }

    // Extract local paths -- for now, only local-to-local transfers are supported.
    // Once network backends are implemented (Plans 02-04), this will route through
    // the appropriate backend's open_read/open_write.
    let source = src_protocol
        .local_path()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(&args.source));
    let dest = dst_protocol
        .local_path()
        .cloned()
        .unwrap_or_else(|| PathBuf::from(&args.dest));
    let source = &source;
    let dest = &dest;

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

    // Parse and validate bandwidth limit early
    let _bandwidth_limit: Option<u64> = if let Some(ref limit_str) = args.limit {
        let bps = parse_bandwidth(limit_str)?;
        tracing::info!("Bandwidth limit: {} bytes/sec", bps);
        Some(bps)
    } else {
        None
    };

    // Log compression status
    if args.compress {
        tracing::info!("Compression enabled (zstd, most effective for network transfers)");
    }

    // Determine chunk strategy
    // When --limit is set, fall back to single-chunk sequential copy with
    // throttled I/O to avoid complexity of shared token buckets across threads.
    // Phase 3 optimization: shared limiter across parallel threads.
    let chunk_count = if _bandwidth_limit.is_some() {
        1 // Sequential for throttled transfers
    } else if args.chunks > 0 {
        args.chunks
    } else {
        auto_chunk_count(source_meta.len())
    };

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

        // Resume support: load existing manifest if --resume is set
        let mut resume_chunks = if args.resume {
            match TransferManifest::load(&final_dest)? {
                Some(manifest) if manifest.is_compatible(source, size) => {
                    let completed = manifest.completed_count();
                    let total = manifest.chunk_count;
                    let completed_bytes = manifest.completed_bytes();
                    tracing::info!(
                        "Resuming transfer: {}/{} chunks already done ({} bytes)",
                        completed,
                        total,
                        completed_bytes
                    );
                    if !quiet && completed > 0 {
                        eprintln!(
                            "Resuming: {}/{} chunks complete",
                            completed, total
                        );
                    }
                    Some(manifest.chunks)
                }
                Some(_manifest) => {
                    // Incompatible manifest -- source or size changed
                    tracing::warn!(
                        "Existing manifest incompatible (source/size changed), starting fresh"
                    );
                    TransferManifest::cleanup(&final_dest)?;
                    None
                }
                None => None,
            }
        } else {
            None
        };

        if chunk_count > 1 && size > 0 {
            // Parallel chunked copy path
            let progress = create_file_progress(size, quiet);

            let chunks = if let Some(ref mut existing) = resume_chunks {
                // Use resumed chunks -- set progress to reflect completed work
                let completed_bytes: u64 = existing.iter()
                    .filter(|c| c.completed)
                    .map(|c| c.length)
                    .sum();
                progress.set_position(completed_bytes);
                existing
            } else {
                // Fresh chunk plan
                resume_chunks = Some(chunk_file(size, chunk_count));
                resume_chunks.as_mut().unwrap()
            };

            // Save initial manifest if --resume
            if args.resume {
                let manifest = TransferManifest::new(
                    source.clone(),
                    final_dest.clone(),
                    size,
                    chunks.clone(),
                    args.compress,
                );
                manifest.save(&final_dest)?;
            }

            parallel_copy_chunked(source, &final_dest, chunks, &progress)?;
            progress.finish_with_message("done");

            // Save completed manifest and then clean up
            if args.resume {
                TransferManifest::cleanup(&final_dest)?;
            }

            tracing::info!(
                "Copied {} bytes using {} parallel chunks",
                size,
                chunk_count
            );
        } else {
            // Sequential copy path (small files or single chunk)
            let progress = create_file_progress(size, quiet);

            // Save initial manifest if --resume (even for sequential)
            if args.resume && size > 0 {
                let fresh_chunks = resume_chunks.unwrap_or_else(|| chunk_file(size, 1));
                let manifest = TransferManifest::new(
                    source.clone(),
                    final_dest.clone(),
                    size,
                    fresh_chunks,
                    args.compress,
                );
                manifest.save(&final_dest)?;
            }

            if let Some(bps) = _bandwidth_limit {
                // Throttled sequential copy
                use std::io::{BufReader, BufWriter, Read, Write};
                use self::throttle::ThrottledReader;

                let src_file = std::fs::File::open(source).map_err(|e| FluxError::Io { source: e })?;
                let reader = BufReader::with_capacity(256 * 1024, src_file);
                let mut throttled = ThrottledReader::new(reader, bps);

                // Ensure parent dir exists
                if let Some(parent) = final_dest.parent() {
                    if !parent.as_os_str().is_empty() && !parent.exists() {
                        std::fs::create_dir_all(parent)?;
                    }
                }

                let dst_file = std::fs::File::create(&final_dest).map_err(|e| FluxError::Io { source: e })?;
                let mut writer = BufWriter::with_capacity(256 * 1024, dst_file);

                let mut buf = [0u8; 256 * 1024];
                let mut total_bytes = 0u64;
                loop {
                    let n = throttled.read(&mut buf)?;
                    if n == 0 {
                        break;
                    }
                    writer.write_all(&buf[..n])?;
                    total_bytes += n as u64;
                    progress.set_position(total_bytes);
                }
                writer.flush()?;
                progress.finish_with_message("done");
                tracing::info!("Copied {} bytes (throttled to {} B/s)", total_bytes, bps);
            } else {
                let bytes = copy_file_with_progress(source, &final_dest, &progress)?;
                tracing::info!("Copied {} bytes", bytes);
            }

            // Clean up resume manifest on success
            if args.resume {
                TransferManifest::cleanup(&final_dest)?;
            }
        }

        // Post-transfer verification if --verify is set
        if args.verify && source_meta.len() > 0 {
            let source_hash = hash_file(source)?;
            let dest_hash = hash_file(&final_dest)?;

            if source_hash != dest_hash {
                return Err(FluxError::ChecksumMismatch {
                    path: final_dest.clone(),
                    expected: source_hash,
                    actual: dest_hash,
                });
            }

            tracing::info!("Integrity verified (BLAKE3)");
            if !quiet {
                eprintln!("Integrity verified (BLAKE3)");
            }
        }

        Ok(())
    } else if source_meta.is_dir() {
        // Directory copy with filtering and optional chunking/verification
        let result = copy_directory(source, dest, &filter, quiet, chunk_count, args.verify)?;

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
    chunks: usize,
    verify: bool,
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

            // Determine per-file chunk count
            let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
            let file_chunk_count = if chunks > 0 {
                // Use explicit chunk setting, but only if file is non-empty
                // and chunk count > 1 and file is large enough
                let effective = if chunks > 1 { chunks } else { 1 };
                effective
            } else {
                auto_chunk_count(file_size)
            };

            let copy_result = if file_chunk_count > 1 && file_size > 0 {
                // Parallel chunked copy for this file
                let file_progress = ProgressBar::hidden();
                let mut file_chunks = chunk_file(file_size, file_chunk_count);
                parallel_copy_chunked(entry.path(), &dest_path, &mut file_chunks, &file_progress)
                    .map(|_| file_size)
            } else {
                // Sequential copy for small files
                let file_progress = ProgressBar::hidden();
                copy_file_with_progress(entry.path(), &dest_path, &file_progress)
            };

            match copy_result {
                Ok(bytes) => {
                    // Post-transfer verification for this file if --verify
                    if verify && file_size > 0 {
                        match (hash_file(entry.path()), hash_file(&dest_path)) {
                            (Ok(src_hash), Ok(dst_hash)) if src_hash != dst_hash => {
                                result.add_error(
                                    entry.path().to_path_buf(),
                                    FluxError::ChecksumMismatch {
                                        path: dest_path.clone(),
                                        expected: src_hash,
                                        actual: dst_hash,
                                    },
                                );
                            }
                            (Err(e), _) | (_, Err(e)) => {
                                result.add_error(entry.path().to_path_buf(), e);
                            }
                            _ => {
                                // Hashes match, file verified
                                result.add_success(bytes);
                            }
                        }
                    } else {
                        result.add_success(bytes);
                    }
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
