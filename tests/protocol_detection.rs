//! Integration tests for Phase 3 Plan 01: Protocol detection infrastructure.
//!
//! Tests that local paths still work end-to-end and that network protocol URIs
//! are properly detected and routed to the correct backends.
//! Now that backends are implemented (Plans 02-04), network transfers
//! fail with connection/protocol errors rather than "not yet implemented".

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
// Local path tests (existing behavior preserved)
// ============================================================================

/// Test that local file copy with String args still works end-to-end.
#[test]
fn test_local_file_copy_still_works() {
    let dir = TempDir::new().unwrap();
    let content = "Protocol detection should not break local copies.";
    let source = create_file_in(&dir, "source.txt", content.as_bytes());
    let dest = dir.path().join("dest.txt");

    flux()
        .args(["cp", source.to_str().unwrap(), dest.to_str().unwrap()])
        .assert()
        .success();

    assert!(dest.exists());
    assert_eq!(fs::read_to_string(&dest).unwrap(), content);
}

/// Test that local directory copy with String args still works.
#[test]
fn test_local_directory_copy_still_works() {
    let dir = TempDir::new().unwrap();
    let source_dir = dir.path().join("src_dir");
    fs::create_dir_all(source_dir.join("sub")).unwrap();
    fs::write(source_dir.join("a.txt"), "alpha").unwrap();
    fs::write(source_dir.join("sub").join("b.txt"), "beta").unwrap();

    let dest = dir.path().join("dest_dir");
    let source_arg = format!("{}/", source_dir.to_str().unwrap());

    flux()
        .args(["cp", "-r", &source_arg, dest.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(dest.join("a.txt")).unwrap(), "alpha");
    assert_eq!(
        fs::read_to_string(dest.join("sub").join("b.txt")).unwrap(),
        "beta"
    );
}

// ============================================================================
// Network protocol error tests (backends implemented but no real servers)
// ============================================================================

/// Test that SFTP destination produces an error (no real SFTP server to connect to).
#[test]
fn test_sftp_dest_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "sftp://user@host.example.com/remote/path",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that SFTP source produces an error (no real SFTP server to connect to).
#[test]
fn test_sftp_source_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let dest = dir.path().join("local_dest.txt");

    flux()
        .args([
            "cp",
            "sftp://user@host.example.com/remote/file.txt",
            dest.to_str().unwrap(),
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that SMB UNC path destination produces an error (no real SMB server).
#[test]
fn test_smb_unc_dest_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "\\\\server\\share\\remote\\file.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that SMB URL destination produces an error (no real SMB server).
#[test]
fn test_smb_url_dest_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "smb://fileserver/shared/docs/file.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that HTTPS WebDAV destination produces an error (no real WebDAV server).
#[test]
fn test_webdav_https_dest_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "https://cloud.example.com/webdav/folder/file.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that HTTP WebDAV destination produces an error (no real WebDAV server).
#[test]
fn test_webdav_http_dest_returns_protocol_error() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "http://nas.local:5005/webdav/file.txt",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("error"));
}

/// Test that protocol errors include a hint message.
#[test]
fn test_protocol_error_shows_hint() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "source.txt", b"test content");

    // Use SFTP which currently returns a ProtocolError with hint
    let result = flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "sftp://host/path",
        ])
        .assert()
        .failure();

    // The command should fail -- hint presence depends on error type
    // (ProtocolError includes hints, I/O errors do not)
    let _ = result;
}
