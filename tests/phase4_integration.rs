use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: get a Command for the flux binary with isolated config/data dirs.
fn flux_isolated(config_dir: &std::path::Path, data_dir: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("flux").expect("flux binary not found");
    cmd.env("FLUX_CONFIG_DIR", config_dir.to_str().unwrap());
    cmd.env("FLUX_DATA_DIR", data_dir.to_str().unwrap());
    cmd
}

/// Create a temp dir with a file inside.
fn create_file_in(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

// ============================================================================
// ALIAS TESTS
// ============================================================================

#[test]
fn test_add_and_list_alias() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Add alias
    flux_isolated(iso.path(), data.path())
        .args(["add", "mynas", "/tmp/testshare"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Alias saved"));

    // List aliases
    flux_isolated(iso.path(), data.path())
        .args(["alias"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mynas"))
        .stdout(predicate::str::contains("/tmp/testshare"));
}

#[test]
fn test_remove_alias() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Add alias
    flux_isolated(iso.path(), data.path())
        .args(["add", "mynas", "/tmp/testshare"])
        .assert()
        .success();

    // Remove alias
    flux_isolated(iso.path(), data.path())
        .args(["alias", "rm", "mynas"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Alias removed"));

    // List should be empty
    flux_isolated(iso.path(), data.path())
        .args(["alias"])
        .assert()
        .success()
        .stdout(predicate::str::contains("no aliases saved"));
}

#[test]
fn test_alias_name_validation() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Single char alias should be rejected
    flux_isolated(iso.path(), data.path())
        .args(["add", "C", "/some/path"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("at least 2 characters"));
}

#[test]
fn test_alias_resolution_in_copy() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    // Create source file
    let source = create_file_in(&work, "source.txt", "alias resolution test");

    // Create alias pointing to temp dir
    let alias_dest = work.path().join("alias_dest");
    fs::create_dir_all(&alias_dest).unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["add", "testdest", alias_dest.to_str().unwrap()])
        .assert()
        .success();

    // Copy using alias
    flux_isolated(iso.path(), data.path())
        .args(["cp", source.to_str().unwrap(), "testdest:"])
        .assert()
        .success();

    // Verify file appeared in alias destination
    assert!(
        alias_dest.join("source.txt").exists(),
        "File should appear in alias destination"
    );
    assert_eq!(
        fs::read_to_string(alias_dest.join("source.txt")).unwrap(),
        "alias resolution test"
    );
}

// ============================================================================
// CONFIG / CONFLICT TESTS
// ============================================================================

#[test]
fn test_dry_run_single_file() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source = create_file_in(&work, "source.txt", "dry run test");
    let dest = work.path().join("dest.txt");

    flux_isolated(iso.path(), data.path())
        .args([
            "cp",
            "--dry-run",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"));

    // Dest should NOT exist
    assert!(!dest.exists(), "Dry-run should not create destination file");
}

#[test]
fn test_dry_run_directory() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source_dir = work.path().join("src_dir");
    fs::create_dir_all(&source_dir).unwrap();
    fs::write(source_dir.join("a.txt"), "alpha").unwrap();
    fs::write(source_dir.join("b.txt"), "beta").unwrap();

    let dest = work.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux_isolated(iso.path(), data.path())
        .args(["cp", "-r", "--dry-run", &source_arg, dest.to_str().unwrap()])
        .assert()
        .success()
        .stderr(predicate::str::contains("[dry-run]"));

    // Dest should NOT exist
    assert!(
        !dest.exists(),
        "Dry-run should not create destination directory"
    );
}

#[test]
fn test_on_conflict_skip() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source = create_file_in(&work, "source.txt", "new content");
    let dest = create_file_in(&work, "dest.txt", "original content");

    flux_isolated(iso.path(), data.path())
        .args([
            "cp",
            "--on-conflict",
            "skip",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Dest should remain unchanged
    assert_eq!(
        fs::read_to_string(&dest).unwrap(),
        "original content",
        "Skip should not overwrite existing file"
    );
}

#[test]
fn test_on_conflict_rename() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source = create_file_in(&work, "source.txt", "new content");
    let dest = create_file_in(&work, "dest.txt", "original content");

    flux_isolated(iso.path(), data.path())
        .args([
            "cp",
            "--on-conflict",
            "rename",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Original dest should remain unchanged
    assert_eq!(
        fs::read_to_string(&dest).unwrap(),
        "original content",
        "Original file should be unchanged"
    );

    // A renamed copy should exist (dest_1.txt)
    let renamed = work.path().join("dest_1.txt");
    assert!(
        renamed.exists(),
        "Renamed file (dest_1.txt) should exist"
    );
    assert_eq!(
        fs::read_to_string(&renamed).unwrap(),
        "new content",
        "Renamed file should have new content"
    );
}

// ============================================================================
// QUEUE TESTS
// ============================================================================

#[test]
fn test_queue_add_and_list() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Add to queue
    flux_isolated(iso.path(), data.path())
        .args(["queue", "add", "/tmp/a.txt", "/tmp/b.txt"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Queued transfer"));

    // List queue
    flux_isolated(iso.path(), data.path())
        .args(["queue"])
        .assert()
        .success()
        .stdout(predicate::str::contains("/tmp/a.txt"))
        .stdout(predicate::str::contains("/tmp/b.txt"))
        .stdout(predicate::str::contains("pending"));
}

#[test]
fn test_queue_lifecycle() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Add entry
    flux_isolated(iso.path(), data.path())
        .args(["queue", "add", "/tmp/a.txt", "/tmp/b.txt"])
        .assert()
        .success();

    // Pause
    flux_isolated(iso.path(), data.path())
        .args(["queue", "pause", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Paused"));

    // Verify paused in list
    flux_isolated(iso.path(), data.path())
        .args(["queue", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("paused"));

    // Resume
    flux_isolated(iso.path(), data.path())
        .args(["queue", "resume", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Resumed"));

    // Cancel
    flux_isolated(iso.path(), data.path())
        .args(["queue", "cancel", "1"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Cancelled"));

    // Verify cancelled in list
    flux_isolated(iso.path(), data.path())
        .args(["queue", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("cancelled"));
}

#[test]
fn test_queue_clear() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    // Add and cancel an entry
    flux_isolated(iso.path(), data.path())
        .args(["queue", "add", "/tmp/a.txt", "/tmp/b.txt"])
        .assert()
        .success();

    flux_isolated(iso.path(), data.path())
        .args(["queue", "cancel", "1"])
        .assert()
        .success();

    // Clear
    flux_isolated(iso.path(), data.path())
        .args(["queue", "clear"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Cleared"));

    // Queue should be empty
    flux_isolated(iso.path(), data.path())
        .args(["queue"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Queue is empty"));
}

// ============================================================================
// HISTORY TESTS
// ============================================================================

#[test]
fn test_history_after_copy() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source = create_file_in(&work, "hist_source.txt", "history recording test");
    let dest = work.path().join("hist_dest.txt");

    // Copy a file (which should record history)
    flux_isolated(iso.path(), data.path())
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // Check history shows the transfer
    flux_isolated(iso.path(), data.path())
        .args(["history"])
        .assert()
        .success()
        .stdout(predicate::str::contains("completed"))
        .stdout(predicate::str::contains("TIMESTAMP"))
        .stdout(predicate::str::contains("SOURCE"));
}

#[test]
fn test_history_clear() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    let source = create_file_in(&work, "hist_source.txt", "history clear test");
    let dest = work.path().join("hist_dest.txt");

    // Copy to create a history entry
    flux_isolated(iso.path(), data.path())
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    // Clear history
    flux_isolated(iso.path(), data.path())
        .args(["history", "--clear"])
        .assert()
        .success()
        .stderr(predicate::str::contains("History cleared"));

    // History should now be empty
    flux_isolated(iso.path(), data.path())
        .args(["history"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No transfer history"));
}

// ============================================================================
// COMPLETIONS TESTS
// ============================================================================

#[test]
fn test_completions_bash() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["completions", "bash"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flux"));
}

#[test]
fn test_completions_powershell() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["completions", "powershell"])
        .assert()
        .success()
        .stdout(predicate::str::is_empty().not());
}

#[test]
fn test_completions_zsh() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["completions", "zsh"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flux"));
}

#[test]
fn test_completions_fish() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["completions", "fish"])
        .assert()
        .success()
        .stdout(predicate::str::contains("flux"));
}

// ============================================================================
// HELP / SUBCOMMAND VISIBILITY TESTS
// ============================================================================

#[test]
fn test_help_includes_phase4_commands() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("history"))
        .stdout(predicate::str::contains("completions"))
        .stdout(predicate::str::contains("queue"))
        .stdout(predicate::str::contains("alias"));
}

#[test]
fn test_history_empty_shows_message() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["history"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No transfer history"));
}
