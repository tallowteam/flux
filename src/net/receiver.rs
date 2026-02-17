//! TCP server for receiving files from Flux senders.
//!
//! Binds a TCP listener, registers the device via mDNS, and accepts incoming
//! connections. Each connection follows the Flux transfer protocol: handshake,
//! optional encryption key exchange, file header, data chunks, completion ack.

use std::path::{Path, PathBuf};

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use tokio_util::bytes::Bytes;
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::config::paths::flux_config_dir;
use crate::discovery::mdns::register_flux_service;
use crate::discovery::service::FluxService;
use crate::error::FluxError;
use crate::net::protocol::{
    decode_message, encode_message, FluxMessage, MAX_FRAME_SIZE, PROTOCOL_VERSION,
};
use crate::security::crypto::{DeviceIdentity, EncryptedChannel};
use crate::security::trust::{TrustStatus, TrustStore};

/// Start the Flux file receiver.
///
/// Binds a TCP listener on the given port, registers an mDNS service,
/// and accepts incoming connections in a loop. Each connection is handled
/// in a spawned task.
///
/// This function runs until cancelled (Ctrl+C).
pub async fn start_receiver(
    port: u16,
    output_dir: &Path,
    encrypt: bool,
    device_name: &str,
    config_dir: &Path,
) -> Result<(), FluxError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .map_err(|e| {
            FluxError::TransferError(format!(
                "Failed to bind port {}: {}. Try a different port with --port.",
                port, e
            ))
        })?;

    let local_addr = listener.local_addr().map_err(|e| {
        FluxError::TransferError(format!("Failed to get local address: {}", e))
    })?;
    let actual_port = local_addr.port();

    // Load or create device identity for encryption/TOFU
    let identity = if encrypt {
        Some(DeviceIdentity::load_or_create(config_dir)?)
    } else {
        None
    };

    let public_key_b64 = identity.as_ref().map(|id| id.public_key_base64());

    // Register mDNS service
    let service = FluxService::new(Some(device_name.to_string()), actual_port);
    let _mdns_daemon = register_flux_service(&service, public_key_b64.as_deref())?;

    eprintln!("Listening on port {}...", actual_port);
    eprintln!("Device name: {}", service.device_name);
    if encrypt {
        eprintln!("Encryption: enabled");
    }

    let output_dir = output_dir.to_path_buf();
    let config_dir = config_dir.to_path_buf();

    loop {
        let (stream, peer_addr) = listener.accept().await.map_err(|e| {
            FluxError::TransferError(format!("Failed to accept connection: {}", e))
        })?;

        eprintln!("Connection from {}", peer_addr);

        let out = output_dir.clone();
        let cfg = config_dir.clone();
        let enc = encrypt;

        tokio::spawn(async move {
            // Per-connection timeout to prevent slowloris and stalled-connection attacks.
            // The handshake must complete within 30 seconds; the entire transfer within 30 minutes.
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(30 * 60),
                handle_connection(stream, out, enc, cfg),
            )
            .await;
            match result {
                Ok(Err(e)) => eprintln!("Transfer error from {}: {}", peer_addr, e),
                Err(_) => eprintln!("Connection from {} timed out", peer_addr),
                Ok(Ok(())) => {}
            }
        });
    }
}

/// Handle a single incoming connection.
///
/// Protocol flow:
/// 1. Read Handshake, verify version
/// 2. If encrypting: key exchange + TOFU check
/// 3. Send HandshakeAck
/// 4. Read FileHeader, create output file
/// 5. Read DataChunks, decrypt if needed, write to file
/// 6. Send TransferComplete
async fn handle_connection(
    stream: TcpStream,
    output_dir: PathBuf,
    encrypt: bool,
    config_dir: PathBuf,
) -> Result<(), FluxError> {
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_SIZE)
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // --- Read Handshake ---
    let hs_bytes = framed
        .next()
        .await
        .ok_or_else(|| FluxError::TransferError("Connection closed before handshake".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to read handshake: {}", e)))?;

    let handshake = decode_message(&hs_bytes)?;

    let (peer_device_name, peer_public_key) = match handshake {
        FluxMessage::Handshake {
            version,
            device_name,
            public_key,
        } => {
            if version != PROTOCOL_VERSION {
                let reject = FluxMessage::HandshakeAck {
                    accepted: false,
                    public_key: None,
                    reason: Some(format!(
                        "Protocol version mismatch: expected {}, got {}",
                        PROTOCOL_VERSION, version
                    )),
                };
                framed
                    .send(Bytes::from(encode_message(&reject)?))
                    .await
                    .ok();
                return Err(FluxError::TransferError(format!(
                    "Protocol version mismatch: expected {}, got {}",
                    PROTOCOL_VERSION, version
                )));
            }
            (device_name, public_key)
        }
        _ => {
            return Err(FluxError::TransferError(
                "Expected Handshake as first message".into(),
            ));
        }
    };

    // --- Encryption / TOFU ---
    let channel = if encrypt {
        let peer_pub_bytes: [u8; 32] = peer_public_key
            .ok_or_else(|| {
                FluxError::EncryptionError(
                    "Encryption required but sender did not provide a public key".into(),
                )
            })?
            .try_into()
            .map_err(|_| FluxError::EncryptionError("Sender public key must be 32 bytes".into()))?;

        // TOFU check
        let peer_pub_b64 = BASE64.encode(peer_pub_bytes);
        let mut trust_store = TrustStore::load(&config_dir)?;

        match trust_store.is_trusted(&peer_device_name, &peer_pub_b64) {
            TrustStatus::Trusted => {
                eprintln!("Verified: {} (trusted)", peer_device_name);
            }
            TrustStatus::Unknown => {
                // Auto-trust for v1 (future: interactive prompt)
                let fingerprint = &peer_pub_b64[..std::cmp::min(16, peer_pub_b64.len())];
                eprintln!(
                    "New device: {} (fingerprint: {}...)",
                    peer_device_name, fingerprint
                );
                eprintln!("Auto-trusting for this session (v1 behavior).");
                trust_store.add_device(
                    peer_device_name.clone(),
                    peer_pub_b64,
                    peer_device_name.clone(),
                );
                trust_store.save()?;
            }
            TrustStatus::KeyChanged => {
                eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
                eprintln!("@    WARNING: DEVICE IDENTIFICATION HAS CHANGED!          @");
                eprintln!("@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@@");
                eprintln!(
                    "The public key for '{}' has changed.",
                    peer_device_name
                );
                eprintln!("This could indicate a man-in-the-middle attack.");
                eprintln!("Connection rejected. Use `flux trust rm {}` to remove the old key.", peer_device_name);

                let reject = FluxMessage::HandshakeAck {
                    accepted: false,
                    public_key: None,
                    reason: Some("Device key has changed - possible impersonation".into()),
                };
                framed
                    .send(Bytes::from(encode_message(&reject)?))
                    .await
                    .ok();
                return Err(FluxError::TrustError(format!(
                    "Key changed for device '{}'",
                    peer_device_name
                )));
            }
        }

        // Generate our ephemeral key pair for this session
        let (our_secret, our_public) = EncryptedChannel::initiate();
        let our_pub_bytes = our_public.as_bytes().to_vec();

        // Send HandshakeAck with our public key
        let ack = FluxMessage::HandshakeAck {
            accepted: true,
            public_key: Some(our_pub_bytes),
            reason: None,
        };
        framed
            .send(Bytes::from(encode_message(&ack)?))
            .await
            .map_err(|e| FluxError::TransferError(format!("Failed to send handshake ack: {}", e)))?;

        // Complete key exchange
        let peer_public = x25519_dalek::PublicKey::from(peer_pub_bytes);
        Some(EncryptedChannel::complete(our_secret, &peer_public))
    } else {
        // Not encrypting -- reject if sender expected encryption to prevent silent downgrade.
        // A MITM could strip the sender's key, but we cannot detect that here.
        // What we CAN prevent is the receiver silently accepting when the sender
        // explicitly offered encryption.
        if peer_public_key.is_some() {
            eprintln!(
                "Rejecting: {} offered encryption but this receiver is not in encrypt mode.",
                peer_device_name
            );
            eprintln!("Start the receiver with --encrypt to accept encrypted transfers.");
            let reject = FluxMessage::HandshakeAck {
                accepted: false,
                public_key: None,
                reason: Some(
                    "Receiver is not in encrypt mode. Start with --encrypt to enable.".into(),
                ),
            };
            framed
                .send(Bytes::from(encode_message(&reject)?))
                .await
                .ok();
            return Err(FluxError::EncryptionError(
                "Sender offered encryption but receiver is not in encrypt mode".into(),
            ));
        }
        let ack = FluxMessage::HandshakeAck {
            accepted: true,
            public_key: None,
            reason: None,
        };
        framed
            .send(Bytes::from(encode_message(&ack)?))
            .await
            .map_err(|e| FluxError::TransferError(format!("Failed to send handshake ack: {}", e)))?;
        None
    };

    // --- Read FileHeader ---
    let fh_bytes = framed
        .next()
        .await
        .ok_or_else(|| FluxError::TransferError("Connection closed before file header".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to read file header: {}", e)))?;

    let file_header = decode_message(&fh_bytes)?;
    let (filename, file_size, _encrypted, expected_checksum) = match file_header {
        FluxMessage::FileHeader {
            filename,
            size,
            encrypted,
            checksum,
        } => (filename, size, encrypted, checksum),
        FluxMessage::Error { message } => {
            return Err(FluxError::TransferError(format!(
                "Sender error: {}",
                message
            )));
        }
        _ => {
            return Err(FluxError::TransferError(
                "Expected FileHeader message".into(),
            ));
        }
    };

    // Validate file size to prevent memory exhaustion from malicious senders
    if file_size > MAX_RECEIVE_SIZE {
        let reject = FluxMessage::Error {
            message: format!(
                "File too large: {} bytes exceeds maximum {} bytes",
                file_size, MAX_RECEIVE_SIZE
            ),
        };
        framed
            .send(Bytes::from(encode_message(&reject)?))
            .await
            .ok();
        return Err(FluxError::TransferError(format!(
            "Rejected file '{}': size {} exceeds maximum {}",
            filename, file_size, MAX_RECEIVE_SIZE
        )));
    }

    // Create output file with auto-rename if it exists (filename is sanitized inside)
    let output_path = find_unique_path(&output_dir, &filename);
    let display_name = output_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| filename.clone());

    // Progress bar
    let pb = indicatif::ProgressBar::new(file_size);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .unwrap()
        .progress_chars("#>-"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());

    // --- Receive DataChunks ---
    let mut received_bytes: u64 = 0;
    let mut expected_offset: u64 = 0;
    // Cap pre-allocation to avoid panic on 32-bit systems or extreme file_size values.
    // The Vec will grow as needed if file_size was under-reported.
    let alloc_cap = usize::try_from(file_size).unwrap_or(usize::MAX).min(256 * 1024 * 1024);
    let mut file_data = Vec::with_capacity(alloc_cap);

    while received_bytes < file_size {
        let chunk_bytes = framed
            .next()
            .await
            .ok_or_else(|| {
                FluxError::TransferError("Connection closed during data transfer".into())
            })?
            .map_err(|e| {
                FluxError::TransferError(format!("Failed to read data chunk: {}", e))
            })?;

        let chunk = decode_message(&chunk_bytes)?;
        match chunk {
            FluxMessage::DataChunk { offset, data, nonce } => {
                // Validate chunk offset matches expected sequential position
                if offset != expected_offset {
                    pb.finish_and_clear();
                    return Err(FluxError::TransferError(format!(
                        "Unexpected chunk offset: expected {}, got {}",
                        expected_offset, offset
                    )));
                }

                let plaintext = if let Some(ref ch) = channel {
                    let nonce_bytes: [u8; 24] = nonce
                        .ok_or_else(|| {
                            FluxError::EncryptionError(
                                "Encrypted chunk missing nonce".into(),
                            )
                        })?
                        .try_into()
                        .map_err(|_| {
                            FluxError::EncryptionError("Nonce must be 24 bytes".into())
                        })?;
                    ch.decrypt(&data, &nonce_bytes)?
                } else {
                    data
                };

                let chunk_len = plaintext.len() as u64;

                // Prevent data overflow: reject if sender sends more than declared size
                if received_bytes + chunk_len > file_size {
                    pb.finish_and_clear();
                    return Err(FluxError::TransferError(format!(
                        "Data overflow: received {} + chunk {} exceeds declared size {}",
                        received_bytes, chunk_len, file_size
                    )));
                }

                received_bytes += chunk_len;
                expected_offset += chunk_len;
                file_data.extend_from_slice(&plaintext);
                pb.set_position(received_bytes);
            }
            FluxMessage::Error { message } => {
                pb.finish_and_clear();
                return Err(FluxError::TransferError(format!(
                    "Sender error during transfer: {}",
                    message
                )));
            }
            _ => {
                pb.finish_and_clear();
                return Err(FluxError::TransferError(
                    "Unexpected message during data transfer".into(),
                ));
            }
        }
    }

    pb.finish_and_clear();

    // --- Verify BLAKE3 checksum if provided ---
    let checksum_verified = if let Some(ref expected) = expected_checksum {
        let actual = blake3::hash(&file_data).to_hex().to_string();
        if actual != *expected {
            let reject = FluxMessage::Error {
                message: format!(
                    "Checksum mismatch: expected {}, got {}",
                    expected, actual
                ),
            };
            framed
                .send(Bytes::from(encode_message(&reject)?))
                .await
                .ok();
            return Err(FluxError::TransferError(format!(
                "BLAKE3 checksum mismatch for '{}': file may be corrupted or tampered",
                filename
            )));
        }
        Some(true)
    } else {
        None
    };

    // Write the received file
    std::fs::write(&output_path, &file_data).map_err(|e| {
        FluxError::TransferError(format!(
            "Failed to write file '{}': {}",
            output_path.display(),
            e
        ))
    })?;

    // --- Send TransferComplete ---
    let complete = FluxMessage::TransferComplete {
        filename: display_name.clone(),
        bytes_received: received_bytes,
        checksum_verified,
    };
    framed
        .send(Bytes::from(encode_message(&complete)?))
        .await
        .map_err(|e| {
            FluxError::TransferError(format!("Failed to send transfer complete: {}", e))
        })?;

    eprintln!(
        "Received: {} ({} bytes) from {}",
        display_name, received_bytes, peer_device_name
    );

    Ok(())
}

/// Sanitize a filename received from a remote peer.
///
/// Prevents path traversal attacks where a malicious sender could
/// provide filenames like `../../etc/passwd` or `/etc/shadow`.
///
/// Rules:
/// - Strip all directory components (only keep the final filename)
/// - Remove leading dots (prevent hidden files on Unix)
/// - Replace empty result with "unnamed"
fn sanitize_filename(filename: &str) -> String {
    // Extract just the filename component, stripping any path separators
    let name = Path::new(filename)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();

    // Strip leading dots to prevent hidden files
    let name = name.trim_start_matches('.');

    if name.is_empty() {
        "unnamed".to_string()
    } else {
        name.to_string()
    }
}

/// Maximum file size the receiver will accept (4 GB).
/// This prevents a malicious sender from claiming an enormous file size
/// and causing the receiver to allocate unbounded memory.
const MAX_RECEIVE_SIZE: u64 = 4 * 1024 * 1024 * 1024;

/// Find a unique file path in the output directory.
///
/// If `output_dir/filename` does not exist, return it as-is.
/// Otherwise, try `filename_1.ext`, `filename_2.ext`, etc. up to 9999.
fn find_unique_path(output_dir: &Path, filename: &str) -> PathBuf {
    let safe_name = sanitize_filename(filename);
    let base = output_dir.join(&safe_name);
    if !base.exists() {
        return base;
    }

    let stem = Path::new(&safe_name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| safe_name.clone());
    let ext = Path::new(&safe_name)
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()));

    for i in 1..=9999 {
        let candidate = match &ext {
            Some(e) => output_dir.join(format!("{}_{}{}", stem, i, e)),
            None => output_dir.join(format!("{}_{}", stem, i)),
        };
        if !candidate.exists() {
            return candidate;
        }
    }

    // Fallback with timestamp
    let ts = chrono::Utc::now().timestamp();
    match &ext {
        Some(e) => output_dir.join(format!("{}_{}{}", stem, ts, e)),
        None => output_dir.join(format!("{}_{}", stem, ts)),
    }
}

/// Synchronous wrapper for starting the receiver.
///
/// Creates a local tokio runtime and blocks on the receiver loop.
/// This is the entry point called from main.rs.
pub fn start_receiver_sync(
    port: u16,
    output_dir: &Path,
    encrypt: bool,
    device_name: &str,
) -> Result<(), FluxError> {
    let config_dir = flux_config_dir()?;

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FluxError::TransferError(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(start_receiver(
        port,
        output_dir,
        encrypt,
        device_name,
        &config_dir,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_unique_path_no_conflict() {
        let dir = tempfile::tempdir().unwrap();
        let result = find_unique_path(dir.path(), "test.txt");
        assert_eq!(result, dir.path().join("test.txt"));
    }

    #[test]
    fn find_unique_path_with_conflict() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "existing").unwrap();

        let result = find_unique_path(dir.path(), "test.txt");
        assert_eq!(result, dir.path().join("test_1.txt"));
    }

    #[test]
    fn find_unique_path_multiple_conflicts() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "existing").unwrap();
        std::fs::write(dir.path().join("test_1.txt"), "existing").unwrap();
        std::fs::write(dir.path().join("test_2.txt"), "existing").unwrap();

        let result = find_unique_path(dir.path(), "test.txt");
        assert_eq!(result, dir.path().join("test_3.txt"));
    }

    #[test]
    fn find_unique_path_no_extension() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("README"), "existing").unwrap();

        let result = find_unique_path(dir.path(), "README");
        assert_eq!(result, dir.path().join("README_1"));
    }
}
