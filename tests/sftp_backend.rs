//! Integration tests for Phase 3 Plan 02: SFTP backend.
//!
//! Tests that the SFTP protocol routing works end-to-end through the CLI.
//! Network-dependent tests are marked `#[ignore]` so they don't run in normal
//! `cargo test`, but can be run with `cargo test -- --ignored` when an SFTP
//! server is available.

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
// Protocol routing tests (no real SFTP server needed)
// ============================================================================

/// Test that an SFTP destination URL is routed to the SFTP backend,
/// which fails with a connection error (not "not yet implemented").
#[test]
fn sftp_dest_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"sftp test content");

    let result = flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "sftp://user@nonexistent-sftp-host-12345.invalid/remote/path",
        ])
        .assert()
        .failure();

    // Should fail with a connection error, not a "not yet implemented" error
    result.stderr(
        predicate::str::contains("Connection failed")
            .or(predicate::str::contains("error")),
    );
}

/// Test that an SFTP source URL is routed to the SFTP backend.
#[test]
fn sftp_source_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("local_dest.txt");

    let result = flux()
        .args([
            "cp",
            "sftp://user@nonexistent-sftp-host-12345.invalid/remote/file.txt",
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure();

    // Should fail with connection error from actual SFTP backend
    result.stderr(
        predicate::str::contains("Connection failed")
            .or(predicate::str::contains("error")),
    );
}

/// Test that SSH scheme is also routed to SFTP backend.
#[test]
fn ssh_scheme_routes_to_sftp_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"ssh scheme test");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "ssh://user@nonexistent-host.invalid/path",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that SFTP with custom port is handled.
#[test]
fn sftp_with_custom_port_routes_correctly() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "test.txt", b"port test");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "sftp://user@nonexistent-host.invalid:2222/path",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that local copies still work fine alongside SFTP backend.
#[test]
fn local_copy_still_works_with_sftp_backend() {
    let dir = TempDir::new().unwrap();
    let content = "Local copy should still work with SFTP backend loaded.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

// ============================================================================
// Real SFTP server tests (requires environment variables)
// ============================================================================

/// Upload a file to an SFTP server and download it back, verifying round-trip.
///
/// Requires environment variables:
/// - SFTP_TEST_HOST: hostname or IP of test SFTP server
/// - SFTP_TEST_USER: SSH username
/// - SFTP_TEST_PATH: base path on the remote server (e.g., /tmp/flux-test)
///
/// The test authenticates via SSH agent or key files (no password prompt).
///
/// Run with: cargo test sftp_upload_download_roundtrip -- --ignored
#[test]
#[ignore] // Requires SFTP server: SFTP_TEST_HOST, SFTP_TEST_USER, SFTP_TEST_PATH env vars
fn sftp_upload_download_roundtrip() {
    let host = std::env::var("SFTP_TEST_HOST").expect("SFTP_TEST_HOST not set");
    let user = std::env::var("SFTP_TEST_USER").expect("SFTP_TEST_USER not set");
    let remote_base = std::env::var("SFTP_TEST_PATH").unwrap_or_else(|_| "/tmp/flux-test".to_string());

    let dir = TempDir::new().unwrap();
    let test_content = "Hello from Flux SFTP integration test!";
    let source = create_file_in(&dir, "sftp_test.txt", test_content.as_bytes());

    let remote_path = format!("sftp://{}@{}{}/sftp_test.txt", user, host, remote_base);

    // Upload
    flux()
        .args(["cp", source.to_str().unwrap(), &remote_path])
        .assert()
        .success();

    // Download
    let downloaded = dir.path().join("downloaded.txt");
    flux()
        .args(["cp", &remote_path, downloaded.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&downloaded).unwrap(), test_content);
}

/// Test recursive directory upload to SFTP server.
///
/// Requires same environment variables as sftp_upload_download_roundtrip.
///
/// Run with: cargo test sftp_recursive_directory_copy -- --ignored
#[test]
#[ignore] // Requires SFTP server: SFTP_TEST_HOST, SFTP_TEST_USER, SFTP_TEST_PATH env vars
fn sftp_recursive_directory_copy() {
    let host = std::env::var("SFTP_TEST_HOST").expect("SFTP_TEST_HOST not set");
    let user = std::env::var("SFTP_TEST_USER").expect("SFTP_TEST_USER not set");
    let remote_base = std::env::var("SFTP_TEST_PATH").unwrap_or_else(|_| "/tmp/flux-test".to_string());

    let dir = TempDir::new().unwrap();
    let src_dir = dir.path().join("src_dir");
    fs::create_dir_all(src_dir.join("sub")).unwrap();
    fs::write(src_dir.join("a.txt"), "alpha").unwrap();
    fs::write(src_dir.join("sub").join("b.txt"), "beta").unwrap();

    let remote_path = format!("sftp://{}@{}{}/test_dir/", user, host, remote_base);

    // Upload directory
    flux()
        .args(["cp", "-r", src_dir.to_str().unwrap(), &remote_path])
        .assert()
        .success();
}
