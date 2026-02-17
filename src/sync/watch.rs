use std::path::Path;
use std::sync::mpsc::RecvTimeoutError;
use std::time::Duration;

use bytesize::ByteSize;
use notify::RecursiveMode;
use notify_debouncer_full::{new_debouncer, DebounceEventResult};

use crate::error::FluxError;
use crate::transfer::filter::TransferFilter;

use super::engine::{compute_sync_plan, execute_sync_plan};

/// Watch the source directory for changes and re-sync to dest on each
/// batch of debounced filesystem events.
///
/// Runs an initial sync immediately, then enters an event loop that
/// re-computes the sync plan and executes it whenever changes are detected.
/// The loop uses `recv_timeout` to allow natural Ctrl+C termination.
pub fn watch_and_sync(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
    quiet: bool,
    verify: bool,
    force: bool,
) -> Result<(), FluxError> {
    let (tx, rx) = std::sync::mpsc::channel();

    // Create debouncer with 2-second timeout
    let mut debouncer = new_debouncer(
        Duration::from_secs(2),
        None,
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    )
    .map_err(|e| FluxError::SyncError(format!("Failed to create file watcher: {}", e)))?;

    // Start watching source directory recursively
    debouncer
        .watch(source, RecursiveMode::Recursive)
        .map_err(|e| FluxError::SyncError(format!("Failed to watch '{}': {}", source.display(), e)))?;

    eprintln!(
        "Watching {} for changes... (press Ctrl+C to stop)",
        source.display()
    );

    // Initial sync
    run_sync_cycle(source, dest, filter, delete_orphans, quiet, verify, force)?;

    // Event loop: recv_timeout allows natural Ctrl+C handling
    loop {
        match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(Ok(_events)) => {
                let timestamp = chrono::Local::now().format("%H:%M:%S");
                eprintln!("[{}] Changes detected, syncing...", timestamp);
                run_sync_cycle(source, dest, filter, delete_orphans, quiet, verify, force)?;
            }
            Ok(Err(errors)) => {
                for e in errors {
                    tracing::warn!("Watch error: {}", e);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                // No events, continue loop (allows Ctrl+C)
                continue;
            }
            Err(RecvTimeoutError::Disconnected) => {
                // Watcher dropped or channel closed
                break;
            }
        }
    }

    Ok(())
}

/// Run a single sync cycle: compute plan, execute if changes found.
fn run_sync_cycle(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
    quiet: bool,
    verify: bool,
    force: bool,
) -> Result<(), FluxError> {
    let plan = compute_sync_plan(source, dest, filter, delete_orphans, force)?;

    if !plan.has_changes() {
        if !quiet {
            eprintln!("Already in sync. Nothing to do.");
        }
        return Ok(());
    }

    let result = execute_sync_plan(&plan, quiet, verify)?;

    if !quiet {
        eprintln!(
            "Sync complete: {} copied, {} updated, {} deleted, {} skipped ({})",
            result.files_copied,
            result.files_updated,
            result.files_deleted,
            result.files_skipped,
            ByteSize(result.bytes_transferred),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_watch_debouncer_creation_smoke() {
        // Smoke test: create a debouncer and immediately drop it.
        // Verifies that notify-debouncer-full integration works at a basic level.
        let (tx, _rx) = std::sync::mpsc::channel();
        let debouncer = new_debouncer(
            Duration::from_secs(2),
            None,
            move |result: DebounceEventResult| {
                let _ = tx.send(result);
            },
        );
        assert!(debouncer.is_ok(), "Debouncer creation should succeed");
        // Drop immediately -- no panic expected
        drop(debouncer);
    }

    #[test]
    fn test_watch_start_watching_directory() {
        // Verify we can start watching a real directory without errors.
        let dir = TempDir::new().unwrap();
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut debouncer = new_debouncer(
            Duration::from_secs(2),
            None,
            move |result: DebounceEventResult| {
                let _ = tx.send(result);
            },
        )
        .unwrap();

        let result = debouncer.watch(dir.path(), RecursiveMode::Recursive);
        assert!(result.is_ok(), "Watching a valid directory should succeed");
        drop(debouncer);
    }

    #[test]
    fn test_run_sync_cycle_no_changes() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        let filter = TransferFilter::new(&[], &[]).unwrap();
        // Both empty -- should report no changes
        let result = run_sync_cycle(&source, &dest, &filter, false, true, false, false);
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_sync_cycle_copies_files() {
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        std::fs::write(source.join("hello.txt"), "world").unwrap();

        let filter = TransferFilter::new(&[], &[]).unwrap();
        let result = run_sync_cycle(&source, &dest, &filter, false, true, false, false);
        assert!(result.is_ok());
        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "world"
        );
    }
}
