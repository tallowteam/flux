//! Conflict resolution logic for file transfers.
//!
//! Provides conflict strategy application (overwrite/skip/rename/ask)
//! and unique filename generation for the rename strategy.

use std::io::IsTerminal;
use std::path::{Path, PathBuf};

use crate::config::types::ConflictStrategy;
use crate::error::FluxError;

/// Resolve a file conflict at the destination path.
///
/// Returns `Some(path)` with the final destination to use, or `None` to skip the file.
///
/// - If dest does not exist: always returns `Some(dest)`.
/// - Overwrite: returns `Some(dest)` (caller will overwrite).
/// - Skip: prints message to stderr, returns `None`.
/// - Rename: generates a unique name (file_1.txt, file_2.txt, etc.), returns `Some(renamed)`.
/// - Ask: prompts user interactively if stdin is a TTY; falls back to Skip if not a TTY.
pub fn resolve_conflict(
    dest: &Path,
    strategy: ConflictStrategy,
) -> Result<Option<PathBuf>, FluxError> {
    if !dest.exists() {
        return Ok(Some(dest.to_path_buf()));
    }

    match strategy {
        ConflictStrategy::Overwrite => Ok(Some(dest.to_path_buf())),
        ConflictStrategy::Skip => {
            eprintln!("Skipped (exists): {}", dest.display());
            Ok(None)
        }
        ConflictStrategy::Rename => {
            let renamed = find_unique_name(dest);
            eprintln!("Renamed: {} -> {}", dest.display(), renamed.display());
            Ok(Some(renamed))
        }
        ConflictStrategy::Ask => {
            if std::io::stdin().is_terminal() {
                eprint!(
                    "{} exists. (o)verwrite / (s)kip / (r)ename? ",
                    dest.display()
                );
                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .map_err(|e| FluxError::Io { source: e })?;
                match input.trim().to_lowercase().as_str() {
                    "o" | "overwrite" => Ok(Some(dest.to_path_buf())),
                    "r" | "rename" => {
                        let renamed = find_unique_name(dest);
                        eprintln!("Renamed: {} -> {}", dest.display(), renamed.display());
                        Ok(Some(renamed))
                    }
                    _ => {
                        // Default to skip on "s", "skip", or any other input
                        eprintln!("Skipped: {}", dest.display());
                        Ok(None)
                    }
                }
            } else {
                // Non-TTY stdin: fall back to Skip to avoid blocking
                eprintln!("Skipped (exists, non-interactive): {}", dest.display());
                Ok(None)
            }
        }
    }
}

/// Generate a unique file name by appending a numeric suffix.
///
/// Given `file.txt`, tries `file_1.txt`, `file_2.txt`, ... up to `file_9999.txt`.
/// If all are taken, falls back to appending a Unix timestamp.
pub fn find_unique_name(path: &Path) -> PathBuf {
    let stem = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    let parent = path.parent().unwrap_or_else(|| Path::new("."));

    for i in 1..=9999 {
        let candidate = parent.join(format!("{}_{}{}", stem, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }

    // Fallback: append Unix timestamp
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    parent.join(format!("{}_{}{}", stem, ts, ext))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_nonexistent_dest_returns_some() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("new_file.txt");
        let result = resolve_conflict(&dest, ConflictStrategy::Overwrite).unwrap();
        assert_eq!(result, Some(dest));
    }

    #[test]
    fn resolve_overwrite_returns_same_path() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("existing.txt");
        fs::write(&dest, "data").unwrap();
        let result = resolve_conflict(&dest, ConflictStrategy::Overwrite).unwrap();
        assert_eq!(result, Some(dest));
    }

    #[test]
    fn resolve_skip_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("existing.txt");
        fs::write(&dest, "data").unwrap();
        let result = resolve_conflict(&dest, ConflictStrategy::Skip).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn resolve_rename_returns_unique_name() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("file.txt");
        fs::write(&dest, "data").unwrap();
        let result = resolve_conflict(&dest, ConflictStrategy::Rename).unwrap();
        assert!(result.is_some());
        let renamed = result.unwrap();
        assert_ne!(renamed, dest);
        assert!(renamed.to_string_lossy().contains("file_1.txt"));
    }

    #[test]
    fn find_unique_name_generates_sequential_names() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("test.txt");
        // base doesn't exist, so _1 is first candidate
        let name = find_unique_name(&base);
        assert_eq!(name, dir.path().join("test_1.txt"));
    }

    #[test]
    fn find_unique_name_skips_existing() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("test.txt");
        // Create _1 and _2, so _3 should be returned
        fs::write(dir.path().join("test_1.txt"), "a").unwrap();
        fs::write(dir.path().join("test_2.txt"), "b").unwrap();
        let name = find_unique_name(&base);
        assert_eq!(name, dir.path().join("test_3.txt"));
    }

    #[test]
    fn find_unique_name_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().join("myfile");
        let name = find_unique_name(&base);
        assert_eq!(name, dir.path().join("myfile_1"));
    }

    #[test]
    fn resolve_ask_falls_back_to_skip_in_tests() {
        // In tests, stdin is not a TTY, so Ask should fall back to Skip
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("existing.txt");
        fs::write(&dest, "data").unwrap();
        let result = resolve_conflict(&dest, ConflictStrategy::Ask).unwrap();
        assert_eq!(result, None);
    }
}
