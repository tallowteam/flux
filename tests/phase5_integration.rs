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

// ============================================================================
// HELP TESTS -- Verify all Phase 5 CLI commands exist and have correct help
// ============================================================================

#[test]
fn test_discover_help() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["discover", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Discover"))
        .stdout(predicate::str::contains("--timeout"));
}

#[test]
fn test_send_help() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["send", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Send"))
        .stdout(predicate::str::contains("--no-encrypt"))
        .stdout(predicate::str::contains("TARGET"));
}

#[test]
fn test_receive_help() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["receive", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Receive"))
        .stdout(predicate::str::contains("--port"))
        .stdout(predicate::str::contains("--no-encrypt"));
}

#[test]
fn test_trust_help() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["trust", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("trust"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("rm"));
}

// ============================================================================
// TRUST COMMAND TESTS
// ============================================================================

#[test]
fn test_trust_list_empty() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["trust", "list"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No trusted devices"));
}

#[test]
fn test_trust_list_default_action() {
    // `flux trust` without subcommand defaults to list
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["trust"])
        .assert()
        .success()
        .stderr(predicate::str::contains("No trusted devices"));
}

#[test]
fn test_trust_rm_nonexistent() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["trust", "rm", "nonexistent-device"])
        .assert()
        .success()
        .stderr(predicate::str::contains("Device not found"));
}

// ============================================================================
// SEND ERROR TESTS
// ============================================================================

#[test]
fn test_send_missing_file() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["send", "nonexistent-file.txt", "127.0.0.1:9999"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Source not found"));
}

// ============================================================================
// DISCOVER TESTS
// ============================================================================

#[test]
fn test_discover_timeout() {
    // Discovery with short timeout should complete without crash.
    // May find 0 devices -- that's OK. We just verify the command runs cleanly.
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["discover", "--timeout", "1"])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success();
}

// ============================================================================
// HELP VISIBILITY TEST -- all Phase 5 commands appear in top-level help
// ============================================================================

#[test]
fn test_help_includes_phase5_commands() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();

    flux_isolated(iso.path(), data.path())
        .args(["--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("discover"))
        .stdout(predicate::str::contains("send"))
        .stdout(predicate::str::contains("receive"))
        .stdout(predicate::str::contains("trust"));
}

// ============================================================================
// LOOPBACK SEND/RECEIVE TEST
// These tests require TCP networking and may be unreliable in some CI
// environments, so they are marked #[ignore].
// ============================================================================

#[test]
#[ignore]
fn test_send_receive_loopback() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    // Create a test file with known content
    let source_path = work.path().join("test_transfer.txt");
    let content = "Hello Flux!\n".repeat(100); // ~1.2KB
    fs::write(&source_path, &content).unwrap();

    let output_dir = work.path().join("received");
    fs::create_dir_all(&output_dir).unwrap();

    // Use a high port to avoid conflicts
    let port = 19741;

    // Start receiver in a background thread
    let recv_iso = iso.path().to_path_buf();
    let recv_data = data.path().to_path_buf();
    let recv_output = output_dir.clone();
    let handle = std::thread::spawn(move || {
        let mut cmd = Command::cargo_bin("flux").expect("flux binary not found");
        cmd.env("FLUX_CONFIG_DIR", recv_iso.to_str().unwrap());
        cmd.env("FLUX_DATA_DIR", recv_data.to_str().unwrap());
        cmd.args([
            "receive",
            "--port",
            &port.to_string(),
            "--output",
            recv_output.to_str().unwrap(),
        ]);
        cmd.timeout(std::time::Duration::from_secs(10));
        // The receiver will be killed by the timeout
        cmd.assert();
    });

    // Give receiver time to start
    std::thread::sleep(std::time::Duration::from_secs(2));

    // Send the file
    flux_isolated(iso.path(), data.path())
        .args([
            "send",
            source_path.to_str().unwrap(),
            &format!("127.0.0.1:{}", port),
        ])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success();

    // Wait for receiver thread to finish (it will timeout)
    let _ = handle.join();

    // Verify the file was received
    let received = output_dir.join("test_transfer.txt");
    assert!(
        received.exists(),
        "Received file should exist at {:?}",
        received
    );
    let received_content = fs::read_to_string(&received).unwrap();
    assert_eq!(
        received_content, content,
        "Received file content should match sent content"
    );
}

#[test]
#[ignore]
fn test_send_receive_encrypted() {
    let iso = TempDir::new().unwrap();
    let data = TempDir::new().unwrap();
    let work = TempDir::new().unwrap();

    // Create a test file
    let source_path = work.path().join("secret.txt");
    let content = "Top secret data!\n".repeat(50);
    fs::write(&source_path, &content).unwrap();

    let output_dir = work.path().join("received");
    fs::create_dir_all(&output_dir).unwrap();

    let port = 19742;

    // Start encrypted receiver
    let recv_iso = iso.path().to_path_buf();
    let recv_data = data.path().to_path_buf();
    let recv_output = output_dir.clone();
    let handle = std::thread::spawn(move || {
        let mut cmd = Command::cargo_bin("flux").expect("flux binary not found");
        cmd.env("FLUX_CONFIG_DIR", recv_iso.to_str().unwrap());
        cmd.env("FLUX_DATA_DIR", recv_data.to_str().unwrap());
        cmd.args([
            "receive",
            "--port",
            &port.to_string(),
            "--output",
            recv_output.to_str().unwrap(),
        ]);
        cmd.timeout(std::time::Duration::from_secs(10));
        cmd.assert();
    });

    std::thread::sleep(std::time::Duration::from_secs(2));

    // Send with encryption (on by default)
    flux_isolated(iso.path(), data.path())
        .args([
            "send",
            source_path.to_str().unwrap(),
            &format!("127.0.0.1:{}", port),
        ])
        .timeout(std::time::Duration::from_secs(10))
        .assert()
        .success();

    let _ = handle.join();

    let received = output_dir.join("secret.txt");
    assert!(received.exists(), "Received encrypted file should exist");
    let received_content = fs::read_to_string(&received).unwrap();
    assert_eq!(received_content, content);
}
