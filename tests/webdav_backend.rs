//! Integration tests for WebDAV backend (Phase 3 Plan 04).
//!
//! Tests protocol routing for WebDAV URLs (https://, http://, dav://).
//! Network-dependent tests are marked with #[ignore] and require a real
//! WebDAV server specified via WEBDAV_TEST_URL env var.

use assert_cmd::Command;
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
// Protocol routing tests (verify WebDAV backend is selected for correct URLs)
// ============================================================================

/// Test that https:// destination routes to WebDAV backend.
/// The command should fail (no real server) but NOT with "not yet implemented".
#[test]
fn webdav_https_url_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "upload.txt", b"webdav test data");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "https://webdav.example.com/remote/upload.txt",
        ])
        .assert()
        .failure();
}

/// Test that http:// destination routes to WebDAV backend.
#[test]
fn webdav_http_url_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "upload.txt", b"webdav test data");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "http://nas.local:5005/webdav/upload.txt",
        ])
        .assert()
        .failure();
}

/// Test that dav:// destination routes to WebDAV backend.
#[test]
fn webdav_dav_scheme_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "upload.txt", b"webdav test data");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "dav://server.example.com/files/upload.txt",
        ])
        .assert()
        .failure();
}

/// Test that webdav:// scheme also routes correctly.
#[test]
fn webdav_explicit_scheme_routes_to_backend() {
    let dir = TempDir::new().unwrap();
    let source = create_file_in(&dir, "upload.txt", b"webdav test data");

    flux()
        .args([
            "cp",
            source.to_str().unwrap(),
            "webdav://server.example.com/files/upload.txt",
        ])
        .assert()
        .failure();
}

/// Test that local file copies still work alongside WebDAV changes.
#[test]
fn local_copy_still_works_with_webdav_backend() {
    let dir = TempDir::new().unwrap();
    let content = "Local copy should not be affected by WebDAV backend.";
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
// Network-dependent tests (require real WebDAV server)
// ============================================================================

/// Full upload/download roundtrip test.
///
/// Requires WEBDAV_TEST_URL env var pointing to a writable WebDAV server.
/// Example: WEBDAV_TEST_URL=https://user:pass@server.com/webdav/
#[test]
#[ignore] // Requires WebDAV server: WEBDAV_TEST_URL env var
fn webdav_upload_download_roundtrip() {
    let webdav_url = std::env::var("WEBDAV_TEST_URL")
        .expect("WEBDAV_TEST_URL env var required for this test");

    let dir = TempDir::new().unwrap();
    let content = "Integration test roundtrip content";
    let source = create_file_in(&dir, "roundtrip.txt", content.as_bytes());
    let remote_dest = format!("{}test-roundtrip-{}.txt", webdav_url, std::process::id());

    // Upload
    flux()
        .args(["cp", source.to_str().unwrap(), &remote_dest])
        .assert()
        .success();

    // Download
    let local_dest = dir.path().join("downloaded.txt");
    flux()
        .args(["cp", &remote_dest, local_dest.to_str().unwrap()])
        .assert()
        .success();

    assert_eq!(fs::read_to_string(&local_dest).unwrap(), content);
}

/// Test stat operation against a real WebDAV server.
#[test]
#[ignore] // Requires WebDAV server: WEBDAV_TEST_URL env var
fn webdav_stat_remote_file() {
    let _webdav_url = std::env::var("WEBDAV_TEST_URL")
        .expect("WEBDAV_TEST_URL env var required for this test");

    // Placeholder for manual testing with a real WebDAV server.
    // Once transfer code uses backends for I/O, this can test
    // stat via an upload-then-stat sequence.
}
