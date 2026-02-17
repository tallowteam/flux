use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

use std::io::{BufRead, BufReader};
use std::process::Stdio;
use std::time::Duration;

fn flux() -> Command {
    Command::cargo_bin("flux").expect("flux binary not found")
}

/// Get the path to the flux binary for spawning processes.
fn flux_bin_path() -> std::path::PathBuf {
    assert_cmd::cargo::cargo_bin("flux")
}

/// Helper: create a file with given content inside a directory.
fn create_file(dir: &std::path::Path, name: &str, content: &str) {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, content).unwrap();
}

#[test]
fn test_sync_basic_copy() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "file1.txt", "content one");
    create_file(&source, "file2.txt", "content two");
    create_file(&source, "sub/file3.txt", "content three");

    flux()
        .args(["sync", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // Verify all 3 files exist in dest with correct content
    assert_eq!(
        std::fs::read_to_string(dest.join("file1.txt")).unwrap(),
        "content one"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("file2.txt")).unwrap(),
        "content two"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("sub/file3.txt")).unwrap(),
        "content three"
    );
}

#[test]
fn test_sync_dry_run_no_changes() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "file.txt", "hello");

    flux()
        .args([
            "sync",
            "--dry-run",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("COPY"));

    // Dest should still be empty
    assert!(!dest.join("file.txt").exists());
}

#[test]
fn test_sync_skips_unchanged() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "same.txt", "identical");
    // Copy to make identical file in dest
    std::fs::copy(source.join("same.txt"), dest.join("same.txt")).unwrap();

    flux()
        .args(["sync", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("Nothing to do").or(predicate::str::contains("sync")));
}

#[test]
fn test_sync_updates_changed() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    // Create files with different sizes (different content length)
    create_file(&source, "file.txt", "updated content that is longer");
    create_file(&dest, "file.txt", "old");

    flux()
        .args(["sync", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // Dest file should now match source
    assert_eq!(
        std::fs::read_to_string(dest.join("file.txt")).unwrap(),
        "updated content that is longer"
    );
}

#[test]
fn test_sync_delete_orphans() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    // file_a in both, file_b only in dest
    create_file(&source, "file_a.txt", "keep me");
    std::fs::copy(source.join("file_a.txt"), dest.join("file_a.txt")).unwrap();
    create_file(&dest, "file_b.txt", "orphan");

    flux()
        .args([
            "sync",
            "--delete",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // file_a remains, file_b removed
    assert!(dest.join("file_a.txt").exists());
    assert!(!dest.join("file_b.txt").exists());
}

#[test]
fn test_sync_exclude_pattern() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "file.txt", "include me");
    create_file(&source, "file.log", "exclude me");

    flux()
        .args([
            "sync",
            "--exclude",
            "*.log",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Only file.txt should be in dest
    assert!(dest.join("file.txt").exists());
    assert!(!dest.join("file.log").exists());
}

#[test]
fn test_sync_empty_source_delete_safety() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&dest, "important.txt", "don't delete");

    flux()
        .args([
            "sync",
            "--delete",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("empty").or(predicate::str::contains("--force")));

    // File should still exist
    assert!(dest.join("important.txt").exists());
}

#[test]
fn test_sync_creates_dest_directory() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("new_dest_that_doesnt_exist");
    std::fs::create_dir_all(&source).unwrap();

    create_file(&source, "file.txt", "sync me");

    // dest doesn't exist yet
    assert!(!dest.exists());

    flux()
        .args(["sync", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // dest should now exist with the file
    assert!(dest.join("file.txt").exists());
    assert_eq!(
        std::fs::read_to_string(dest.join("file.txt")).unwrap(),
        "sync me"
    );
}

#[test]
fn test_sync_watch_schedule_mutex() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    flux()
        .args([
            "sync",
            "--watch",
            "--schedule",
            "* * * * *",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("mutually exclusive"));
}

// ---- Plan 02 integration tests ----

#[test]
fn test_sync_watch_initial_sync() {
    // Watch mode should perform an initial sync immediately on start.
    // We spawn the process, wait for files to appear in dest, then kill it.
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "initial.txt", "watch mode test");

    let mut child = std::process::Command::new(flux_bin_path())
        .args([
            "sync",
            "--watch",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("Failed to spawn flux sync --watch");

    // Wait up to 5 seconds for the initial sync to complete
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    let mut synced = false;
    while std::time::Instant::now() < deadline {
        if dest.join("initial.txt").exists() {
            let content = std::fs::read_to_string(dest.join("initial.txt")).unwrap_or_default();
            if content == "watch mode test" {
                synced = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(200));
    }

    // Kill the watcher process
    let _ = child.kill();
    let _ = child.wait();

    assert!(synced, "Watch mode should perform initial sync: initial.txt should appear in dest");
}

#[test]
fn test_sync_schedule_invalid_cron() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    flux()
        .args([
            "sync",
            "--schedule",
            "not valid",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid cron expression"));
}

#[test]
fn test_sync_schedule_prints_next_time() {
    // Schedule mode should print "Next sync at:" before waiting.
    // Spawn and read stderr for a few seconds to verify.
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    let mut child = std::process::Command::new(flux_bin_path())
        .args([
            "sync",
            "--schedule",
            "0 0 */1 * * *",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .stderr(Stdio::piped())
        .stdout(Stdio::null())
        .spawn()
        .expect("Failed to spawn flux sync --schedule");

    let stderr = child.stderr.take().expect("Failed to capture stderr");
    let reader = BufReader::new(stderr);

    let mut found_next_sync = false;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);

    for line in reader.lines() {
        if std::time::Instant::now() > deadline {
            break;
        }
        if let Ok(line) = line {
            if line.contains("Next sync at:") {
                found_next_sync = true;
                break;
            }
        }
    }

    let _ = child.kill();
    let _ = child.wait();

    assert!(found_next_sync, "Schedule mode should print 'Next sync at:' message");
}

#[test]
fn test_sync_nested_directories() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    // Create deeply nested structure
    create_file(&source, "a/b/c/deep.txt", "deep content");
    create_file(&source, "a/sibling.txt", "sibling content");
    create_file(&source, "top.txt", "top content");

    flux()
        .args(["sync", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // Verify nested structure preserved
    assert_eq!(
        std::fs::read_to_string(dest.join("a/b/c/deep.txt")).unwrap(),
        "deep content"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("a/sibling.txt")).unwrap(),
        "sibling content"
    );
    assert_eq!(
        std::fs::read_to_string(dest.join("top.txt")).unwrap(),
        "top content"
    );
}

#[test]
fn test_sync_verify_flag() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&source, "verified.txt", "verify this content");

    flux()
        .args([
            "sync",
            "--verify",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // File should be copied correctly (verify mode checks BLAKE3 hash)
    assert_eq!(
        std::fs::read_to_string(dest.join("verified.txt")).unwrap(),
        "verify this content"
    );
}

#[test]
fn test_sync_force_empty_source_delete() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("src");
    let dest = dir.path().join("dst");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir_all(&dest).unwrap();

    create_file(&dest, "doomed.txt", "will be deleted");

    // Without --force, should fail
    flux()
        .args([
            "sync",
            "--delete",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure();

    // File still exists
    assert!(dest.join("doomed.txt").exists());

    // With --force, should succeed and delete
    flux()
        .args([
            "sync",
            "--delete",
            "--force",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // File should be deleted
    assert!(!dest.join("doomed.txt").exists());
}
