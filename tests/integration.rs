use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: get a Command for the flux binary.
fn flux() -> Command {
    Command::cargo_bin("flux").expect("flux binary not found")
}

/// Helper: create a TempDir with a file containing given content.
fn create_file_in(dir: &TempDir, name: &str, content: &str) -> std::path::PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

// ============================================================================
// Test 1: Single file copy
// ============================================================================
#[test]
fn test_single_file_copy() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", "hello flux");
    let dest = dir.path().join("dest.txt");

    flux()
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    assert!(dest.exists(), "Destination file should exist");
    assert_eq!(
        fs::read_to_string(&dest).unwrap(),
        "hello flux",
        "Content should match"
    );
}

// ============================================================================
// Test 2: Single file copy - source not found
// ============================================================================
#[test]
fn test_source_not_found() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            dir.path().join("nonexistent.txt").to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Source not found"))
        .stderr(predicate::str::contains("hint:"));
}

// ============================================================================
// Test 3: Directory copy without -r flag
// ============================================================================
#[test]
fn test_directory_without_recursive_flag() {
    let dir = TempDir::new().unwrap();
    let subdir = dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    create_file_in(&dir, "subdir/file.txt", "content");
    let dest = dir.path().join("output");

    flux()
        .args([
            "cp",
            subdir.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("use -r flag"));
}

// ============================================================================
// Test 4: Recursive directory copy (trailing slash = copy contents)
// ============================================================================
#[test]
fn test_recursive_directory_copy() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();
    fs::write(source_dir.join("a.txt"), "alpha").unwrap();
    fs::write(source_dir.join("sub").join("b.txt"), "beta").unwrap();

    let dest = dir.path().join("dest_dir");

    // Use trailing slash to copy contents
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args(["cp", "-r", &source_arg, dest.to_str().unwrap()])
        .assert()
        .success();

    // Contents should be directly in dest (not nested)
    assert_eq!(
        fs::read_to_string(dest.join("a.txt")).unwrap(),
        "alpha",
        "a.txt should be in dest root"
    );
    assert_eq!(
        fs::read_to_string(dest.join("sub").join("b.txt")).unwrap(),
        "beta",
        "sub/b.txt should preserve structure"
    );
}

// ============================================================================
// Test 5: Exclude pattern
// ============================================================================
#[test]
fn test_exclude_pattern() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();
    fs::write(source_dir.join("keep.txt"), "kept").unwrap();
    fs::write(source_dir.join("skip.log"), "skipped").unwrap();
    fs::write(source_dir.join("sub").join("also_skip.log"), "also skipped").unwrap();

    let dest = dir.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args([
            "cp",
            "-r",
            "--exclude",
            "*.log",
            &source_arg,
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.join("keep.txt").exists(), "keep.txt should exist");
    assert!(
        !dest.join("skip.log").exists(),
        "skip.log should NOT exist"
    );
    assert!(
        !dest.join("sub").join("also_skip.log").exists(),
        "sub/also_skip.log should NOT exist"
    );
}

// ============================================================================
// Test 6: Include pattern
// ============================================================================
#[test]
fn test_include_pattern() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();
    fs::write(source_dir.join("want.rs"), "fn main() {}").unwrap();
    fs::write(source_dir.join("ignore.txt"), "ignored").unwrap();
    fs::write(source_dir.join("sub").join("want2.rs"), "fn helper() {}").unwrap();

    let dest = dir.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args([
            "cp",
            "-r",
            "--include",
            "*.rs",
            &source_arg,
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.join("want.rs").exists(), "want.rs should exist");
    assert!(
        !dest.join("ignore.txt").exists(),
        "ignore.txt should NOT exist"
    );
    assert!(
        dest.join("sub").join("want2.rs").exists(),
        "sub/want2.rs should exist"
    );
}

// ============================================================================
// Test 7: Quiet mode suppresses output
// ============================================================================
#[test]
fn test_quiet_mode() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", "quiet test");
    let dest = dir.path().join("dest.txt");

    let output = flux()
        .args([
            "-q",
            "cp",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success()
        .get_output()
        .clone();

    // stderr should be empty (no progress bar, no info messages)
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.is_empty(),
        "Quiet mode should produce no stderr output, got: '{}'",
        stderr
    );

    assert!(dest.exists(), "File should still be copied");
}

// ============================================================================
// Test 8: Help text
// ============================================================================
#[test]
fn test_help_text() {
    flux()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("flux"))
        .stdout(predicate::str::contains("cp"));
}

// ============================================================================
// Test 9: Verbose mode accepted
// ============================================================================
#[test]
fn test_verbose_mode() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", "verbose test");
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "-v",
            "cp",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.exists(), "File should be copied in verbose mode");
}

// ============================================================================
// Test 10: Copy preserves content for binary-like data
// ============================================================================
#[test]
fn test_binary_copy_preserves_content() {
    let dir = TempDir::new().unwrap();
    let source = dir.path().join("source.bin");
    let dest = dir.path().join("dest.bin");

    // Generate 1MB of deterministic binary-like data (0..255 repeated)
    let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
    fs::write(&source, &data).unwrap();

    flux()
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    let dest_data = fs::read(&dest).unwrap();
    assert_eq!(
        dest_data.len(),
        data.len(),
        "Binary file size should match"
    );
    assert_eq!(dest_data, data, "Binary file content should match exactly");
}
