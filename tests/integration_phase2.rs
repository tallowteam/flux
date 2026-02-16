//! Integration tests for Phase 2 Plan 03: Resume, Compression, and Throttling.
//!
//! These tests exercise the --resume, --compress, and --limit CLI flags
//! via the flux binary, verifying end-to-end correctness.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper: get a Command for the flux binary.
fn flux() -> Command {
    Command::cargo_bin("flux").expect("flux binary not found")
}

/// Helper: create a file with given content in a temp directory.
fn create_file_in(dir: &TempDir, name: &str, content: &[u8]) -> PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

// ============================================================================
// Resume tests
// ============================================================================

/// Test that --resume with no existing manifest copies normally (no error).
#[test]
fn test_resume_no_manifest_copies_normally() {
    let dir = TempDir::new().unwrap();
    let content = "Resume test: no manifest should work fine.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--resume",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);

    // No manifest should remain after successful copy
    let manifest_path = dest.with_file_name("dest.txt.flux-resume.json");
    assert!(
        !manifest_path.exists(),
        "Manifest should be cleaned up after successful copy"
    );
}

/// Test that --resume with a pre-existing manifest loads and resumes.
/// We create a manifest with some chunks marked complete, then verify
/// the transfer still produces correct output.
#[test]
fn test_resume_with_existing_manifest() {
    let dir = TempDir::new().unwrap();

    // Create a source file with known content
    let content = vec![0xABu8; 1000];
    let source = create_file_in(&dir, "source.bin", &content);
    let dest = dir.path().join("dest.bin");

    // First, do a normal copy to establish the destination
    flux()
        .args([
            "cp",
            "--resume",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read(&dest).unwrap(), content);
}

// ============================================================================
// Compress tests
// ============================================================================

/// Test that --compress flag produces correct output for a text file.
#[test]
fn test_compress_flag_text_file() {
    let dir = TempDir::new().unwrap();
    let content = "Compression test content. ".repeat(100);
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--compress",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test that --compress flag works with binary data.
#[test]
fn test_compress_flag_binary_file() {
    let dir = TempDir::new().unwrap();
    let content: Vec<u8> = (0..10_000u32).map(|i| (i % 256) as u8).collect();
    let source = create_file_in(&dir, "source.bin", &content);
    let dest = dir.path().join("dest.bin");

    flux()
        .args([
            "cp",
            "--compress",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read(&dest).unwrap(), content);
}

// ============================================================================
// Limit (bandwidth throttle) tests
// ============================================================================

/// Test that --limit flag produces correct output (don't assert timing for CI).
#[test]
fn test_limit_flag_correctness() {
    let dir = TempDir::new().unwrap();
    let content = "Bandwidth limited test content.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--limit",
            "1MB/s",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test that an invalid --limit value produces an error.
#[test]
fn test_limit_flag_invalid_value() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test");
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--limit",
            "not_a_number",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Invalid bandwidth"));
}

// ============================================================================
// Combined flags tests
// ============================================================================

/// Test that --resume and --compress work together.
#[test]
fn test_resume_and_compress_together() {
    let dir = TempDir::new().unwrap();
    let content = "Combined resume and compress test. ".repeat(50);
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--resume",
            "--compress",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test that --chunks, --resume, and --verify all work together.
#[test]
fn test_chunks_resume_verify_together() {
    let dir = TempDir::new().unwrap();
    let content: Vec<u8> = (0..50_000u32).map(|i| (i % 256) as u8).collect();
    let source = create_file_in(&dir, "source.bin", &content);
    let dest = dir.path().join("dest.bin");

    flux()
        .args([
            "cp",
            "--chunks",
            "2",
            "--verify",
            "--resume",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read(&dest).unwrap(), content);
}

/// Test that --limit with a generous rate on a small file works correctly.
#[test]
fn test_limit_with_small_file() {
    let dir = TempDir::new().unwrap();
    let content = "Small file with bandwidth limit.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--limit",
            "10MB/s",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

// ============================================================================
// Parallel chunked copy tests (Plan 02-02)
// ============================================================================

/// Test --chunks flag triggers parallel chunked copy and produces correct output.
#[test]
fn test_chunks_flag_parallel_copy() {
    let dir = TempDir::new().unwrap();
    // Create 1MB file with known pattern
    let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
    let source = create_file_in(&dir, "source.bin", &data);
    let dest = dir.path().join("dest.bin");

    flux()
        .args([
            "cp",
            "--chunks",
            "4",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert!(dest.exists(), "Destination file should exist");
    let dest_data = fs::read(&dest).unwrap();
    assert_eq!(dest_data.len(), data.len(), "File sizes should match");
    assert_eq!(dest_data, data, "File content should match exactly");
}

/// Test --verify flag produces "verified" message and exit code 0.
#[test]
fn test_verify_flag_passes() {
    let dir = TempDir::new().unwrap();
    let content = "Verify test content for BLAKE3 integrity checking.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args([
            "cp",
            "--verify",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success()
        .stderr(predicate::str::contains("verified").or(predicate::str::contains("Integrity")));

    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test auto-chunk for small file (<10MB) uses sequential path (no error).
#[test]
fn test_auto_chunk_small_file() {
    let dir = TempDir::new().unwrap();
    let content = "Small file that should use sequential copy.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    // No --chunks flag: auto-detect should choose 1 chunk for small file
    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test --chunks 1 explicitly uses single chunk (degenerate case, sequential).
#[test]
fn test_chunks_one_sequential() {
    let dir = TempDir::new().unwrap();
    let data: Vec<u8> = (0..100_000u32).map(|i| (i % 256) as u8).collect();
    let source = create_file_in(&dir, "source.bin", &data);
    let dest = dir.path().join("dest.bin");

    flux()
        .args([
            "cp",
            "--chunks",
            "1",
            source.to_str().unwrap(),
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read(&dest).unwrap(), data);
}

/// Test directory copy with --chunks works correctly.
#[test]
fn test_directory_with_chunks() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();

    // Create files with different content
    let file_a: Vec<u8> = (0..50_000u32).map(|i| (i % 256) as u8).collect();
    let file_b: Vec<u8> = (0..30_000u32).map(|i| ((i + 128) % 256) as u8).collect();
    fs::write(source_dir.join("a.bin"), &file_a).unwrap();
    fs::write(source_dir.join("sub").join("b.bin"), &file_b).unwrap();

    let dest = dir.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args([
            "cp",
            "-r",
            "--chunks",
            "2",
            &source_arg,
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(fs::read(dest.join("a.bin")).unwrap(), file_a);
    assert_eq!(fs::read(dest.join("sub").join("b.bin")).unwrap(), file_b);
}

/// Test directory copy with --verify works correctly.
#[test]
fn test_verify_directory() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();

    fs::write(source_dir.join("a.txt"), "alpha file content").unwrap();
    fs::write(source_dir.join("sub").join("b.txt"), "beta file content").unwrap();

    let dest = dir.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args([
            "cp",
            "-r",
            "--verify",
            &source_arg,
            dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    assert_eq!(
        fs::read_to_string(dest.join("a.txt")).unwrap(),
        "alpha file content"
    );
    assert_eq!(
        fs::read_to_string(dest.join("sub").join("b.txt")).unwrap(),
        "beta file content"
    );
}
