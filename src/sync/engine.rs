use std::path::Path;
use std::time::Duration;

use indicatif::ProgressBar;
use walkdir::WalkDir;

use crate::error::FluxError;
use crate::progress::bar::create_directory_progress;
use crate::transfer::checksum::hash_file;
use crate::transfer::copy::copy_file_with_progress;
use crate::transfer::filter::TransferFilter;

use super::plan::{SyncAction, SyncPlan, SyncResult};

/// Decision for a single file comparison.
#[derive(Debug, PartialEq)]
pub enum SyncDecision {
    /// Destination does not exist -- copy the file.
    CopyNew,
    /// File differs (size or mtime) -- update the dest.
    Update,
    /// File is identical -- skip.
    Skip,
}

/// Cross-filesystem mtime tolerance: 2 seconds.
/// FAT32 has 2-second mtime resolution; this avoids false positives
/// when syncing between NTFS and FAT32 or across network mounts.
const MTIME_TOLERANCE: Duration = Duration::from_secs(2);

/// Determine whether a source file needs to be synced to dest.
///
/// Decision logic:
/// 1. If dest doesn't exist -> CopyNew
/// 2. If file sizes differ -> Update
/// 3. If source mtime is newer than dest mtime (by more than 2s tolerance) -> Update
/// 4. Otherwise -> Skip
pub fn needs_sync(src_meta: &std::fs::Metadata, dest_path: &Path) -> SyncDecision {
    let dest_meta = match std::fs::metadata(dest_path) {
        Ok(m) => m,
        Err(_) => return SyncDecision::CopyNew,
    };

    // Different size -> definitely changed
    if src_meta.len() != dest_meta.len() {
        return SyncDecision::Update;
    }

    // Compare modification times with tolerance for cross-filesystem sync
    match (src_meta.modified(), dest_meta.modified()) {
        (Ok(src_mtime), Ok(dest_mtime)) => {
            if let Ok(diff) = src_mtime.duration_since(dest_mtime) {
                if diff > MTIME_TOLERANCE {
                    return SyncDecision::Update;
                }
            }
            SyncDecision::Skip
        }
        _ => SyncDecision::Skip, // Can't compare mtimes, assume same
    }
}

/// Compute a sync plan by diffing source and dest directory trees.
///
/// Phase 1: Walk source tree, compare each file against dest.
/// Phase 2: If delete_orphans, walk dest tree and find files not in source.
/// Safety: refuses to proceed if source is empty and delete_orphans is true
/// (unless force is true).
pub fn compute_sync_plan(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
    force: bool,
) -> Result<SyncPlan, FluxError> {
    let mut actions = Vec::new();

    // Phase 1: Walk source tree, compare against dest
    let mut source_file_count = 0u64;
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        if !filter.should_transfer(entry.path()) {
            continue;
        }

        source_file_count += 1;

        let relative = entry.path().strip_prefix(source)?;
        let dest_path = dest.join(relative);
        let src_meta = entry.metadata()?;

        match needs_sync(&src_meta, &dest_path) {
            SyncDecision::CopyNew => {
                actions.push(SyncAction::CopyNew {
                    src: entry.path().to_path_buf(),
                    dest: dest_path,
                    size: src_meta.len(),
                });
            }
            SyncDecision::Update => {
                let dest_size = std::fs::metadata(&dest_path)
                    .map(|m| m.len())
                    .unwrap_or(0);
                actions.push(SyncAction::UpdateChanged {
                    src: entry.path().to_path_buf(),
                    dest: dest_path,
                    src_size: src_meta.len(),
                    dest_size,
                });
            }
            SyncDecision::Skip => {
                actions.push(SyncAction::Skip {
                    path: entry.path().to_path_buf(),
                    reason: "unchanged",
                });
            }
        }
    }

    // Phase 2: Walk dest tree, find orphans (if --delete)
    if delete_orphans && dest.exists() {
        // Safety check: empty source + delete is dangerous
        if source_file_count == 0 && !force {
            return Err(FluxError::SyncError(
                "Source directory is empty but --delete is set. Use --force to proceed.".to_string(),
            ));
        }

        for entry in WalkDir::new(dest)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let relative = match entry.path().strip_prefix(dest) {
                Ok(r) => r,
                Err(_) => continue,
            };
            let src_path = source.join(relative);

            // Only mark as orphan if not in source AND passes filter
            // (don't delete files that were merely excluded from sync)
            if !src_path.exists() {
                // Check if the file would have been filtered out of the source walk
                // If so, it's not truly an orphan -- it was just excluded
                if filter.should_transfer(&src_path) {
                    actions.push(SyncAction::DeleteOrphan {
                        path: entry.path().to_path_buf(),
                        size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                    });
                }
            }
        }
    }

    Ok(SyncPlan::from_actions(actions))
}

/// Execute a sync plan: copy/update/delete files as determined.
///
/// For CopyNew and UpdateChanged: ensures parent dirs exist, copies using
/// existing `copy_file_with_progress`. For DeleteOrphan: removes the file.
/// Skip actions are ignored.
pub fn execute_sync_plan(
    plan: &SyncPlan,
    quiet: bool,
    verify: bool,
) -> Result<SyncResult, FluxError> {
    let actionable = plan.files_to_copy + plan.files_to_update + plan.files_to_delete;
    let progress = create_directory_progress(actionable, quiet);
    let mut result = SyncResult::default();

    for action in &plan.actions {
        match action {
            SyncAction::CopyNew { src, dest, size } => {
                ensure_parent_exists(dest)?;
                let file_progress = ProgressBar::hidden();
                copy_file_with_progress(src, dest, &file_progress)?;

                if verify && *size > 0 {
                    verify_copy(src, dest)?;
                }

                result.files_copied += 1;
                result.bytes_transferred += size;
                progress.inc(1);
            }
            SyncAction::UpdateChanged {
                src,
                dest,
                src_size,
                ..
            } => {
                ensure_parent_exists(dest)?;
                let file_progress = ProgressBar::hidden();
                copy_file_with_progress(src, dest, &file_progress)?;

                if verify && *src_size > 0 {
                    verify_copy(src, dest)?;
                }

                result.files_updated += 1;
                result.bytes_transferred += src_size;
                progress.inc(1);
            }
            SyncAction::DeleteOrphan { path, .. } => {
                std::fs::remove_file(path)?;
                result.files_deleted += 1;
                progress.inc(1);
            }
            SyncAction::Skip { .. } => {
                result.files_skipped += 1;
            }
        }
    }

    progress.finish_with_message("done");
    Ok(result)
}

/// Ensure a file's parent directory exists.
fn ensure_parent_exists(path: &Path) -> Result<(), FluxError> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    Ok(())
}

/// Verify a copy with BLAKE3 checksums.
fn verify_copy(src: &Path, dest: &Path) -> Result<(), FluxError> {
    let src_hash = hash_file(src)?;
    let dest_hash = hash_file(dest)?;
    if src_hash != dest_hash {
        return Err(FluxError::ChecksumMismatch {
            path: dest.to_path_buf(),
            expected: src_hash,
            actual: dest_hash,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transfer::filter::TransferFilter;
    use tempfile::TempDir;

    fn no_filter() -> TransferFilter {
        TransferFilter::new(&[], &[]).unwrap()
    }

    /// Helper: create a file with given content.
    fn create_file(dir: &Path, name: &str, content: &str) {
        let path = dir.join(name);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
    }

    #[test]
    fn test_needs_sync_dest_missing() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("file.txt");
        std::fs::write(&src, "hello").unwrap();
        let src_meta = std::fs::metadata(&src).unwrap();

        let dest = dir.path().join("nonexistent.txt");
        assert_eq!(needs_sync(&src_meta, &dest), SyncDecision::CopyNew);
    }

    #[test]
    fn test_needs_sync_different_size() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");

        std::fs::write(&src, "hello world").unwrap();
        std::fs::write(&dst, "hi").unwrap();

        let src_meta = std::fs::metadata(&src).unwrap();
        assert_eq!(needs_sync(&src_meta, &dst), SyncDecision::Update);
    }

    #[test]
    fn test_needs_sync_same_file() {
        let dir = TempDir::new().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");

        let content = "identical content";
        std::fs::write(&src, content).unwrap();
        // Copy to ensure same content and similar mtime
        std::fs::copy(&src, &dst).unwrap();

        let src_meta = std::fs::metadata(&src).unwrap();
        assert_eq!(needs_sync(&src_meta, &dst), SyncDecision::Skip);
    }

    #[test]
    fn test_compute_sync_plan_new_files() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        create_file(&source, "a.txt", "aaa");
        create_file(&source, "b.txt", "bbb");
        create_file(&source, "sub/c.txt", "ccc");

        let plan = compute_sync_plan(&source, &dest, &no_filter(), false, false).unwrap();

        assert_eq!(plan.files_to_copy, 3);
        assert_eq!(plan.files_to_update, 0);
        assert_eq!(plan.files_to_delete, 0);
        assert_eq!(plan.files_to_skip, 0);
        assert!(plan.has_changes());
    }

    #[test]
    fn test_compute_sync_plan_mixed() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        // New file in source only
        create_file(&source, "new.txt", "new content");

        // Existing file with different size -> update
        create_file(&source, "changed.txt", "longer content here");
        create_file(&dest, "changed.txt", "short");

        // Existing file with same content -> skip
        create_file(&source, "same.txt", "identical");
        // Copy to ensure same size and mtime
        std::fs::copy(source.join("same.txt"), dest.join("same.txt")).unwrap();

        let plan = compute_sync_plan(&source, &dest, &no_filter(), false, false).unwrap();

        assert_eq!(plan.files_to_copy, 1); // new.txt
        assert_eq!(plan.files_to_update, 1); // changed.txt
        assert_eq!(plan.files_to_skip, 1); // same.txt
        assert!(plan.has_changes());
    }

    #[test]
    fn test_compute_sync_plan_delete_orphans() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        // File in both (same content)
        create_file(&source, "keep.txt", "keep me");
        std::fs::copy(source.join("keep.txt"), dest.join("keep.txt")).unwrap();

        // Orphan: only in dest
        create_file(&dest, "orphan.txt", "delete me");

        let plan = compute_sync_plan(&source, &dest, &no_filter(), true, false).unwrap();

        assert_eq!(plan.files_to_delete, 1);
        // Check the orphan action is for the right file
        let delete_actions: Vec<_> = plan
            .actions
            .iter()
            .filter(|a| matches!(a, SyncAction::DeleteOrphan { .. }))
            .collect();
        assert_eq!(delete_actions.len(), 1);
        if let SyncAction::DeleteOrphan { path, .. } = &delete_actions[0] {
            assert!(path.ends_with("orphan.txt"));
        }
    }

    #[test]
    fn test_compute_sync_plan_empty_source_delete_safety() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        // Dest has files but source is empty
        create_file(&dest, "important.txt", "don't delete me");

        let result = compute_sync_plan(&source, &dest, &no_filter(), true, false);
        assert!(result.is_err());
        let err = result.unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("empty"));
        assert!(msg.contains("--force"));
    }

    #[test]
    fn test_compute_sync_plan_empty_source_delete_with_force() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        create_file(&dest, "file.txt", "content");

        // With force=true, should succeed
        let plan = compute_sync_plan(&source, &dest, &no_filter(), true, true).unwrap();
        assert_eq!(plan.files_to_delete, 1);
    }

    #[test]
    fn test_execute_sync_plan_copies_files() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        create_file(&source, "file.txt", "hello sync");

        let plan = compute_sync_plan(&source, &dest, &no_filter(), false, false).unwrap();
        assert_eq!(plan.files_to_copy, 1);

        let result = execute_sync_plan(&plan, true, false).unwrap();
        assert_eq!(result.files_copied, 1);
        assert_eq!(result.bytes_transferred, 10); // "hello sync" = 10 bytes

        // Verify file was actually copied
        let dest_content = std::fs::read_to_string(dest.join("file.txt")).unwrap();
        assert_eq!(dest_content, "hello sync");
    }

    #[test]
    fn test_execute_sync_plan_deletes_orphans() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        // Keep file in both
        create_file(&source, "keep.txt", "keep");
        std::fs::copy(source.join("keep.txt"), dest.join("keep.txt")).unwrap();

        // Orphan in dest
        create_file(&dest, "orphan.txt", "bye");

        let plan = compute_sync_plan(&source, &dest, &no_filter(), true, false).unwrap();
        let result = execute_sync_plan(&plan, true, false).unwrap();

        assert_eq!(result.files_deleted, 1);
        assert!(!dest.join("orphan.txt").exists());
        assert!(dest.join("keep.txt").exists());
    }

    #[test]
    fn test_compute_sync_plan_with_filter() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        create_file(&source, "file.txt", "include me");
        create_file(&source, "file.log", "exclude me");

        let filter = TransferFilter::new(&["*.log".to_string()], &[]).unwrap();
        let plan = compute_sync_plan(&source, &dest, &filter, false, false).unwrap();

        assert_eq!(plan.files_to_copy, 1); // only file.txt
    }
}
