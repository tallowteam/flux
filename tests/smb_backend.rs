//! Integration tests for Phase 3 Plan 03: SMB backend.
//!
//! Tests protocol routing for SMB paths (both UNC and smb:// URL format).
//! Network-dependent tests are marked with `#[ignore]` and require
//! SMB_TEST_HOST and SMB_TEST_SHARE environment variables.

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs;
use tempfile::TempDir;

/// Helper: get a Command for the flux binary.
fn flux() -> Command {
    Command::cargo_bin("flux").expect("flux binary not found")
}

/// Helper: create a file with given content in a temp directory.
fn create_file_in(dir: &TempDir, name: &str, content: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(&path, content).unwrap();
    path
}

// ============================================================================
// Protocol routing tests (no real SMB server needed)
// ============================================================================

/// Test that a UNC path (\\server\share\path) is routed to the SMB backend
/// and produces an appropriate error (server unreachable, not "not implemented").
#[test]
fn test_unc_path_routes_to_smb_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"smb routing test");

    let result = flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "\\\\nonexistent-server\\share\\dest.txt",
        ])
        .assert()
        .failure();

    // Should fail because server doesn't exist, NOT because backend isn't implemented
    let output = result.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("not yet implemented"),
        "SMB backend should be active, not a stub. Got: {}",
        stderr
    );
}

/// Test that an smb:// URL is routed to the SMB backend.
#[test]
fn test_smb_url_routes_to_smb_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"smb url routing test");

    let result = flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "smb://nonexistent-server/share/dest.txt",
        ])
        .assert()
        .failure();

    // Should fail due to network error, not "not yet implemented"
    let output = result.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("not yet implemented"),
        "SMB backend should be active. Got: {}",
        stderr
    );
}

/// Test that SMB backend is selected for UNC source paths too.
#[test]
fn test_unc_source_routes_to_smb_backend() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("local_dest.txt");

    let result = flux()
        .args([
            "cp",
            "\\\\nonexistent-server\\share\\source.txt",
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure();

    let output = result.get_output();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("not yet implemented"),
        "SMB backend should handle UNC source paths. Got: {}",
        stderr
    );
}

/// Test that SMB backend reports failure for empty server in smb:// URL.
/// The protocol parser extracts server/share from smb:// URLs;
/// if the share is empty, the backend should produce a clear error.
#[test]
fn test_smb_url_no_share_produces_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"no share test");

    // smb://server (no share specified) -> SMB backend gets empty share
    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "smb://server",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

// ============================================================================
// Network-dependent integration tests (require real SMB share)
// ============================================================================

/// End-to-end SMB upload/download roundtrip test.
///
/// Requires environment variables:
///   SMB_TEST_HOST  - SMB server hostname (e.g., "nas.local")
///   SMB_TEST_SHARE - SMB share name (e.g., "public")
///
/// Run with: cargo test --test smb_backend -- --ignored
#[test]
#[ignore] // Requires SMB share: set SMB_TEST_HOST and SMB_TEST_SHARE env vars
fn smb_upload_download_roundtrip() {
    let host = std::env::var("SMB_TEST_HOST").expect("SMB_TEST_HOST env var required");
    let share = std::env::var("SMB_TEST_SHARE").expect("SMB_TEST_SHARE env var required");

    let dir = TempDir::new().unwrap();
    let content = "Flux SMB roundtrip test data\n";
    let source = create_file_in(&dir, "flux_smb_test.txt", content.as_bytes());

    let smb_dest = format!("\\\\{}\\{}\\flux_test_upload.txt", host, share);

    // Upload to SMB share
    flux()
        .args(["cp", source.to_str().unwrap(), &smb_dest])
        .assert()
        .success();

    // Download from SMB share
    let download_dest = dir.path().join("downloaded.txt");
    flux()
        .args(["cp", &smb_dest, download_dest.to_str().unwrap()])
        .assert()
        .success();

    // Verify content matches
    let downloaded = fs::read_to_string(&download_dest).expect("downloaded file should exist");
    assert_eq!(downloaded, content, "Downloaded content should match uploaded");
}

/// Test SMB directory listing via recursive copy.
///
/// Requires: SMB_TEST_HOST, SMB_TEST_SHARE, and a directory with files on the share.
#[test]
#[ignore] // Requires SMB share with files
fn smb_recursive_directory_copy() {
    let host = std::env::var("SMB_TEST_HOST").expect("SMB_TEST_HOST env var required");
    let share = std::env::var("SMB_TEST_SHARE").expect("SMB_TEST_SHARE env var required");

    let dir = TempDir::new().unwrap();
    let smb_source = format!("\\\\{}\\{}/", host, share);
    let local_dest = dir.path().join("smb_download");

    // This will attempt to recursively copy from the SMB share
    flux()
        .args([
            "cp",
            "-r",
            &smb_source,
            local_dest.to_str().unwrap(),
        ])
        .assert()
        .success();

    // Verify at least something was copied
    assert!(local_dest.exists(), "Destination directory should exist");
}
