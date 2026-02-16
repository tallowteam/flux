use std::path::Path;

use globset::{Glob, GlobSet, GlobSetBuilder};
use walkdir::DirEntry;

use crate::error::FluxError;

/// Glob-based file filter for include/exclude pattern matching during transfers.
///
/// Exclude patterns are checked first — any matching file is skipped.
/// Include patterns (if any) are checked second — file must match at least one.
/// Directories can be pruned early via `is_excluded_dir` in walkdir's `filter_entry`.
pub struct TransferFilter {
    excludes: Option<GlobSet>,
    includes: Option<GlobSet>,
}

impl TransferFilter {
    /// Create a new filter from exclude and include glob patterns.
    ///
    /// Empty pattern lists result in `None` (no filtering for that dimension).
    /// Returns `FluxError::InvalidPattern` if any glob pattern is malformed.
    pub fn new(
        exclude_patterns: &[String],
        include_patterns: &[String],
    ) -> Result<Self, FluxError> {
        let excludes = if exclude_patterns.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in exclude_patterns {
                builder.add(Glob::new(pattern)?);
            }
            Some(builder.build()?)
        };

        let includes = if include_patterns.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in include_patterns {
                builder.add(Glob::new(pattern)?);
            }
            Some(builder.build()?)
        };

        Ok(Self { excludes, includes })
    }

    /// Returns true if the file at `path` should be transferred.
    ///
    /// Logic (applied in order):
    /// 1. If excludes match the path (full path or file name), return false
    /// 2. If includes exist and none match the path (full path or file name), return false
    /// 3. Otherwise return true
    ///
    /// Matching against both full path and file name ensures patterns like
    /// `*.log` work at any depth (matching the file name) while patterns like
    /// `build/**` work against the full path structure.
    pub fn should_transfer(&self, path: &Path) -> bool {
        // Check excludes first
        if let Some(ref excludes) = self.excludes {
            if Self::matches_glob(excludes, path) {
                return false;
            }
        }

        // If includes specified, path must match at least one
        if let Some(ref includes) = self.includes {
            return Self::matches_glob(includes, path);
        }

        true
    }

    /// Returns true if a directory entry should be excluded (pruned from traversal).
    ///
    /// Only checks exclude patterns — include patterns are not used for directory
    /// pruning because a directory might contain files that match an include pattern
    /// even if the directory name itself does not.
    ///
    /// Used with `walkdir::IntoIter::filter_entry` for early directory pruning.
    pub fn is_excluded_dir(&self, entry: &DirEntry) -> bool {
        if !entry.file_type().is_dir() {
            return false;
        }

        if let Some(ref excludes) = self.excludes {
            return Self::matches_glob(excludes, entry.path());
        }

        false
    }

    /// Match a glob set against both the full path and just the file name.
    ///
    /// This enables patterns like `*.log` to match files at any depth
    /// (by matching the file name component), while patterns like `build/**`
    /// match against the full path structure.
    fn matches_glob(glob_set: &GlobSet, path: &Path) -> bool {
        if glob_set.is_match(path) {
            return true;
        }
        // Also try matching just the file name for simple patterns like *.log
        if let Some(file_name) = path.file_name() {
            if glob_set.is_match(Path::new(file_name)) {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn no_patterns_means_everything_transfers() {
        let filter = TransferFilter::new(&[], &[]).unwrap();
        assert!(filter.should_transfer(Path::new("anything.txt")));
        assert!(filter.should_transfer(Path::new("dir/nested/file.rs")));
        assert!(filter.should_transfer(Path::new("secret.log")));
    }

    #[test]
    fn exclude_log_files_skips_log_but_not_txt() {
        let filter = TransferFilter::new(&["*.log".to_string()], &[]).unwrap();
        assert!(!filter.should_transfer(Path::new("debug.log")));
        assert!(!filter.should_transfer(Path::new("sub/error.log")));
        assert!(filter.should_transfer(Path::new("readme.txt")));
        assert!(filter.should_transfer(Path::new("src/main.rs")));
    }

    #[test]
    fn include_rs_only_transfers_rs_files() {
        let filter = TransferFilter::new(&[], &["*.rs".to_string()]).unwrap();
        assert!(filter.should_transfer(Path::new("main.rs")));
        assert!(filter.should_transfer(Path::new("src/lib.rs")));
        assert!(!filter.should_transfer(Path::new("readme.txt")));
        assert!(!filter.should_transfer(Path::new("data.log")));
    }

    #[test]
    fn exclude_and_include_combination() {
        // Exclude *.log, include *.txt — only .txt files pass, .log and others fail
        let filter = TransferFilter::new(
            &["*.log".to_string()],
            &["*.txt".to_string()],
        )
        .unwrap();
        assert!(filter.should_transfer(Path::new("readme.txt")));
        assert!(!filter.should_transfer(Path::new("debug.log")));
        assert!(!filter.should_transfer(Path::new("main.rs"))); // not in includes
    }

    #[test]
    fn directory_pattern_excludes_subtree() {
        let filter = TransferFilter::new(&["build/**".to_string()], &[]).unwrap();
        assert!(!filter.should_transfer(Path::new("build/output.o")));
        assert!(!filter.should_transfer(Path::new("build/sub/artifact.bin")));
        assert!(filter.should_transfer(Path::new("src/main.rs")));
        assert!(filter.should_transfer(Path::new("readme.txt")));
    }

    #[test]
    fn is_excluded_dir_only_checks_directories() {
        let filter = TransferFilter::new(&["target".to_string()], &[]).unwrap();

        // Create a temp directory structure to get a real DirEntry
        let dir = tempfile::tempdir().unwrap();
        let target_dir = dir.path().join("target");
        std::fs::create_dir(&target_dir).unwrap();
        let src_dir = dir.path().join("src");
        std::fs::create_dir(&src_dir).unwrap();

        // Walk the temp dir and test entries
        let entries: Vec<_> = walkdir::WalkDir::new(dir.path())
            .min_depth(1)
            .max_depth(1)
            .into_iter()
            .filter_map(|e| e.ok())
            .collect();

        let target_entry = entries.iter().find(|e| e.file_name() == "target").unwrap();
        let src_entry = entries.iter().find(|e| e.file_name() == "src").unwrap();

        assert!(filter.is_excluded_dir(target_entry));
        assert!(!filter.is_excluded_dir(src_entry));
    }

    #[test]
    fn invalid_pattern_returns_error() {
        let result = TransferFilter::new(&["[invalid".to_string()], &[]);
        assert!(result.is_err());
    }
}
