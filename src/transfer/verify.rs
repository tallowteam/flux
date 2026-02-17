//! Directory verification: compare two locations and report differences.
//!
//! Walks both source and destination trees, compares file sizes and BLAKE3
//! hashes, and produces a structured `VerifyResult` with matched, differing,
//! source-only, and dest-only files.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use bytesize::ByteSize;
use walkdir::WalkDir;

use crate::error::FluxError;
use crate::progress::bar::create_transfer_progress;
use crate::transfer::checksum::hash_file;
use crate::transfer::filter::TransferFilter;

/// Reason two files differ.
#[derive(Debug)]
pub enum DiffReason {
    /// Files have different sizes.
    SizeMismatch { src_size: u64, dst_size: u64 },
    /// Files have same size but different BLAKE3 content hashes.
    ContentMismatch,
}

/// A single file that differs between source and dest.
#[derive(Debug)]
pub struct DiffEntry {
    pub path: PathBuf,
    pub reason: DiffReason,
}

/// Result of a directory verification.
#[derive(Debug)]
pub struct VerifyResult {
    /// Number of files that are identical (same BLAKE3 hash).
    pub matched: u64,
    /// Files that exist in both but differ.
    pub differs: Vec<DiffEntry>,
    /// Files only in source (not in dest).
    pub source_only: Vec<PathBuf>,
    /// Files only in dest (not in source).
    pub dest_only: Vec<PathBuf>,
    /// Files that caused errors during comparison.
    pub errors: Vec<(PathBuf, FluxError)>,
    /// Total bytes checked across all files.
    pub bytes_checked: u64,
}

/// Compare two directories and report differences without copying.
///
/// Algorithm:
/// 1. Walk source tree, collect all relative paths
/// 2. For each source file, check if dest has it:
///    - Missing -> source_only
///    - Present -> compare size, then BLAKE3 hash if sizes match
/// 3. Walk dest tree for files not in source -> dest_only
/// 4. Shows progress bar during verification
pub fn verify_directories(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    quiet: bool,
) -> Result<VerifyResult, FluxError> {
    // Validate inputs
    if !source.exists() {
        return Err(FluxError::SourceNotFound {
            path: source.to_path_buf(),
        });
    }
    if !source.is_dir() {
        return Err(FluxError::SyncError(format!(
            "Source '{}' is not a directory",
            source.display()
        )));
    }
    if !dest.exists() {
        return Err(FluxError::SourceNotFound {
            path: dest.to_path_buf(),
        });
    }
    if !dest.is_dir() {
        return Err(FluxError::SyncError(format!(
            "Destination '{}' is not a directory",
            dest.display()
        )));
    }

    if !quiet {
        eprintln!(
            "Verifying: {} <-> {}",
            source.display(),
            dest.display()
        );
    }

    // First pass: count files and total bytes for progress bar
    let mut total_bytes = 0u64;
    let mut source_files: Vec<(PathBuf, u64)> = Vec::new(); // (relative_path, size)

    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if !filter.should_transfer(entry.path()) {
            continue;
        }
        let relative = match entry.path().strip_prefix(source) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        total_bytes += size;
        source_files.push((relative, size));
    }

    // Also count dest-only bytes (for files we'll need to check existence of)
    let progress = create_transfer_progress(total_bytes, quiet);
    let mut source_relative_set: HashSet<PathBuf> = HashSet::new();

    let mut result = VerifyResult {
        matched: 0,
        differs: Vec::new(),
        source_only: Vec::new(),
        dest_only: Vec::new(),
        errors: Vec::new(),
        bytes_checked: 0,
    };

    // Phase 1: Check each source file against dest
    for (relative, size) in &source_files {
        source_relative_set.insert(relative.clone());

        let src_path = source.join(relative);
        let dst_path = dest.join(relative);

        progress.set_message(
            relative
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
        );

        if !dst_path.exists() {
            result.source_only.push(relative.clone());
            progress.inc(*size);
            result.bytes_checked += size;
            continue;
        }

        // Compare sizes first (fast path)
        let dst_size = match std::fs::metadata(&dst_path) {
            Ok(m) => m.len(),
            Err(e) => {
                result
                    .errors
                    .push((relative.clone(), FluxError::Io { source: e }));
                progress.inc(*size);
                result.bytes_checked += size;
                continue;
            }
        };

        if *size != dst_size {
            result.differs.push(DiffEntry {
                path: relative.clone(),
                reason: DiffReason::SizeMismatch {
                    src_size: *size,
                    dst_size,
                },
            });
            progress.inc(*size);
            result.bytes_checked += size;
            continue;
        }

        // Sizes match -- compare BLAKE3 hashes
        match (hash_file(&src_path), hash_file(&dst_path)) {
            (Ok(src_hash), Ok(dst_hash)) => {
                if src_hash == dst_hash {
                    result.matched += 1;
                } else {
                    result.differs.push(DiffEntry {
                        path: relative.clone(),
                        reason: DiffReason::ContentMismatch,
                    });
                }
            }
            (Err(e), _) => {
                result.errors.push((relative.clone(), e));
            }
            (_, Err(e)) => {
                result.errors.push((relative.clone(), e));
            }
        }

        progress.inc(*size);
        result.bytes_checked += size;
    }

    // Phase 2: Walk dest tree to find dest-only files
    for entry in WalkDir::new(dest)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        if !entry.file_type().is_file() {
            continue;
        }
        if !filter.should_transfer(entry.path()) {
            continue;
        }
        let relative = match entry.path().strip_prefix(dest) {
            Ok(r) => r.to_path_buf(),
            Err(_) => continue,
        };
        if !source_relative_set.contains(&relative) {
            result.dest_only.push(relative);
        }
    }

    progress.finish_and_clear();

    // Print results
    if !quiet {
        print_verify_result(&result);
    }

    Ok(result)
}

/// Print a human-readable verification report to stderr.
fn print_verify_result(result: &VerifyResult) {
    let total_files = result.matched
        + result.differs.len() as u64
        + result.source_only.len() as u64
        + result.dest_only.len() as u64;

    eprintln!(
        "\nVerification complete: {} matched, {} differ, {} source-only, {} dest-only ({} checked)",
        result.matched,
        result.differs.len(),
        result.source_only.len(),
        result.dest_only.len(),
        ByteSize(result.bytes_checked),
    );

    if !result.differs.is_empty() {
        eprintln!("\nDifferences:");
        for entry in &result.differs {
            match &entry.reason {
                DiffReason::SizeMismatch {
                    src_size,
                    dst_size,
                } => {
                    eprintln!(
                        "  DIFFER  {} (size: {} vs {})",
                        entry.path.display(),
                        ByteSize(*src_size),
                        ByteSize(*dst_size),
                    );
                }
                DiffReason::ContentMismatch => {
                    eprintln!(
                        "  DIFFER  {} (content mismatch)",
                        entry.path.display()
                    );
                }
            }
        }
    }

    if !result.source_only.is_empty() {
        eprintln!("\nSource only (not in dest):");
        for path in &result.source_only {
            eprintln!("  {}", path.display());
        }
    }

    if !result.dest_only.is_empty() {
        eprintln!("\nDest only (not in source):");
        for path in &result.dest_only {
            eprintln!("  {}", path.display());
        }
    }

    if !result.errors.is_empty() {
        eprintln!("\nErrors:");
        for (path, err) in &result.errors {
            eprintln!("  {}  {}", path.display(), err);
        }
    }

    // Exit status hint
    if total_files > 0
        && result.differs.is_empty()
        && result.source_only.is_empty()
        && result.dest_only.is_empty()
        && result.errors.is_empty()
    {
        eprintln!("\nAll files match.");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn no_filter() -> TransferFilter {
        TransferFilter::new(&[], &[]).unwrap()
    }

    fn create_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }

    #[test]
    fn identical_directories_all_match() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        create_file(&src, "a.txt", "hello");
        create_file(&dst, "a.txt", "hello");
        create_file(&src, "sub/b.txt", "world");
        create_file(&dst, "sub/b.txt", "world");

        let result = verify_directories(&src, &dst, &no_filter(), true).unwrap();
        assert_eq!(result.matched, 2);
        assert!(result.differs.is_empty());
        assert!(result.source_only.is_empty());
        assert!(result.dest_only.is_empty());
    }

    #[test]
    fn detects_source_only_files() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        create_file(&src, "only-in-src.txt", "data");

        let result = verify_directories(&src, &dst, &no_filter(), true).unwrap();
        assert_eq!(result.source_only.len(), 1);
        assert_eq!(result.matched, 0);
    }

    #[test]
    fn detects_dest_only_files() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        create_file(&dst, "only-in-dst.txt", "data");

        let result = verify_directories(&src, &dst, &no_filter(), true).unwrap();
        assert_eq!(result.dest_only.len(), 1);
        assert_eq!(result.matched, 0);
    }

    #[test]
    fn detects_size_mismatch() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        create_file(&src, "file.txt", "short");
        create_file(&dst, "file.txt", "much longer content");

        let result = verify_directories(&src, &dst, &no_filter(), true).unwrap();
        assert_eq!(result.differs.len(), 1);
        assert!(matches!(
            result.differs[0].reason,
            DiffReason::SizeMismatch { .. }
        ));
    }

    #[test]
    fn detects_content_mismatch() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        // Same size, different content
        create_file(&src, "file.txt", "aaaa");
        create_file(&dst, "file.txt", "bbbb");

        let result = verify_directories(&src, &dst, &no_filter(), true).unwrap();
        assert_eq!(result.differs.len(), 1);
        assert!(matches!(
            result.differs[0].reason,
            DiffReason::ContentMismatch
        ));
    }

    #[test]
    fn respects_filter() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src");
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        create_file(&src, "keep.txt", "hello");
        create_file(&dst, "keep.txt", "hello");
        create_file(&src, "skip.log", "log data");

        let filter = TransferFilter::new(&["*.log".to_string()], &[]).unwrap();
        let result = verify_directories(&src, &dst, &filter, true).unwrap();

        assert_eq!(result.matched, 1);
        assert!(result.source_only.is_empty()); // .log excluded
    }

    #[test]
    fn nonexistent_source_errors() {
        let dir = TempDir::new().unwrap();
        let dst = dir.path().join("dst");
        std::fs::create_dir_all(&dst).unwrap();

        let result = verify_directories(
            &dir.path().join("nonexistent"),
            &dst,
            &no_filter(),
            true,
        );
        assert!(result.is_err());
    }
}
