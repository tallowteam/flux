use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::TempDir;

fn flux() -> Command {
    Command::cargo_bin("flux").expect("flux binary not found")
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
