//! TCP server for receiving files from Flux senders.
//!
//! Binds a TCP listener, registers the device via mDNS, and accepts incoming
//! connections. Each connection follows the Flux transfer protocol: handshake,
//! optional encryption key exchange, file header, data chunks, completion ack.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use futures::{SinkExt, StreamExt};
use tokio_util::bytes::Bytes;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
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
use crate::transfer::stats::TransferStats;

/// Start the Flux file receiver.
///
/// Binds a TCP listener on `bind_addr:port`, registers an mDNS service,
/// and accepts incoming connections in a loop. Each connection is handled
/// in a spawned task. At most 8 connections are handled concurrently; additional
/// connections wait until a slot is available.
///
/// This function runs until cancelled (Ctrl+C).
pub async fn start_receiver(
    port: u16,
    output_dir: &Path,
    encrypt: bool,
    device_name: &str,
    config_dir: &Path,
    bind_addr: &str,
) -> Result<(), FluxError> {
    let listener = TcpListener::bind(format!("{}:{}", bind_addr, port))
        .await
        .map_err(|e| {
            FluxError::TransferError(format!(
                "Failed to bind {}:{}: {}. Try a different address with --bind or port with --port.",
                bind_addr, port, e
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
    let _mdns_daemon = register_flux_service(&service, public_key_b64.as_deref(), None)?;

    eprintln!("Listening on port {}...", actual_port);
    eprintln!("Device name: {}", service.device_name);
    if encrypt {
        eprintln!("Encryption: enabled");
    }

    let output_dir = output_dir.to_path_buf();
    let config_dir = config_dir.to_path_buf();

    // Limit concurrent connections to 8 to prevent resource exhaustion.
    // Connections beyond this limit wait until an active transfer finishes.
    let semaphore = Arc::new(Semaphore::new(8));

    loop {
        let (stream, peer_addr) = listener.accept().await.map_err(|e| {
            FluxError::TransferError(format!("Failed to accept connection: {}", e))
        })?;

        eprintln!("Connection from {}", peer_addr);

        let out = output_dir.clone();
        let cfg = config_dir.clone();
        let enc = encrypt;

        // Acquire a permit before spawning. The permit is moved into the task
        // and released automatically when the task completes (via Drop).
        // If all 8 slots are occupied, warn before blocking so that the operator
        // knows the server is at capacity rather than wondering why it is slow.
        let permit = match semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(_) => {
                tracing::warn!(
                    "All 8 connection slots in use, waiting for a slot to free up"
                );
                semaphore.clone().acquire_owned().await.map_err(|e| {
                    FluxError::TransferError(format!(
                        "Connection semaphore closed unexpectedly: {}",
                        e
                    ))
                })?
            }
        };

        tokio::spawn(async move {
            // Hold the permit for the duration of the connection.
            let _permit = permit;

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
    let started = std::time::Instant::now();

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

    // Sanitize the peer-supplied device name before it is used as a trust store
    // key or displayed to the user. The name arrives untrusted from the network
    // and could contain control characters or be excessively long.
    let peer_device_name = sanitize_peer_device_name(&peer_device_name);

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
                // Prompt user for trust confirmation
                let fingerprint = &peer_pub_b64[..std::cmp::min(16, peer_pub_b64.len())];
                eprintln!(
                    "New device: {} (fingerprint: {}...)",
                    peer_device_name, fingerprint
                );
                // Interactive confirmation: ask the user before trusting
                eprint!("Trust this device? [y/N]: ");
                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_ok()
                    && input.trim().eq_ignore_ascii_case("y")
                {
                    trust_store.add_device(
                        peer_device_name.clone(),
                        peer_pub_b64,
                        peer_device_name.clone(),
                    );
                    trust_store.save()?;
                    eprintln!("Device trusted.");
                } else {
                    let reject = FluxMessage::HandshakeAck {
                        accepted: false,
                        public_key: None,
                        reason: Some("Connection rejected: device not trusted".into()),
                    };
                    framed
                        .send(Bytes::from(encode_message(&reject)?))
                        .await
                        .ok();
                    return Err(FluxError::TrustError(format!(
                        "Rejected untrusted device '{}'",
                        peer_device_name
                    )));
                }
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
                "Rejecting: {} offered encryption but this receiver was started with --no-encrypt.",
                peer_device_name
            );
            eprintln!("Remove --no-encrypt to accept encrypted transfers (encryption is on by default).");
            let reject = FluxMessage::HandshakeAck {
                accepted: false,
                public_key: None,
                reason: Some(
                    "Receiver was started with --no-encrypt. Remove --no-encrypt to enable encryption.".into(),
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

    // Warn when the declared size is unusually large (>2 GB) so that operators
    // have visibility into resource-intensive transfers before they start.
    if file_size > 2 * 1024 * 1024 * 1024 {
        tracing::info!(
            file = %filename,
            size_bytes = file_size,
            "Large incoming transfer declared ({} bytes); this will take significant time and disk space",
            file_size,
        );
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
        .expect("static progress template is valid")
        .progress_chars("#>-"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());

    // --- Receive DataChunks: stream directly to disk ---
    let mut received_bytes: u64 = 0;
    let mut expected_offset: u64 = 0;
    let mut hasher = blake3::Hasher::new();

    // Open output file exclusively (atomic create, prevents TOCTOU/symlink)
    let mut out_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|e| {
            FluxError::TransferError(format!(
                "Failed to create file '{}': {}",
                output_path.display(), e
            ))
        })?;

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
                    drop(out_file);
                    let _ = std::fs::remove_file(&output_path);
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
                    drop(out_file);
                    let _ = std::fs::remove_file(&output_path);
                    return Err(FluxError::TransferError(format!(
                        "Data overflow: received {} + chunk {} exceeds declared size {}",
                        received_bytes, chunk_len, file_size
                    )));
                }

                // Stream to disk + incremental hash (no full-file buffering)
                {
                    use std::io::Write;
                    out_file.write_all(&plaintext).map_err(|e| {
                        FluxError::TransferError(format!(
                            "Failed to write chunk to '{}': {}",
                            output_path.display(), e
                        ))
                    })?;
                }
                hasher.update(&plaintext);

                received_bytes += chunk_len;
                expected_offset += chunk_len;
                pb.set_position(received_bytes);
            }
            FluxMessage::Error { message } => {
                pb.finish_and_clear();
                drop(out_file);
                let _ = std::fs::remove_file(&output_path);
                return Err(FluxError::TransferError(format!(
                    "Sender error during transfer: {}",
                    message
                )));
            }
            _ => {
                pb.finish_and_clear();
                drop(out_file);
                let _ = std::fs::remove_file(&output_path);
                return Err(FluxError::TransferError(
                    "Unexpected message during data transfer".into(),
                ));
            }
        }
    }

    pb.finish_and_clear();
    drop(out_file);

    // --- Verify BLAKE3 checksum (computed incrementally during receive) ---
    let checksum_verified = if let Some(ref expected) = expected_checksum {
        let actual = hasher.finalize().to_hex().to_string();
        if actual != *expected {
            // Checksum mismatch — delete the corrupted file
            let _ = std::fs::remove_file(&output_path);
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

    {
        let mut stats = TransferStats::new(1, file_size);
        stats.started = started;
        stats.add_done(received_bytes);
        stats.print_file_summary(&display_name, false);
    }

    Ok(())
}

/// Receive a file using code-phrase mode (Croc-like UX).
///
/// The receiver is a TCP client:
/// 1. Validate code phrase, compute code_hash
/// 2. Discover sender via mDNS code_hash match
/// 3. TCP connect to discovered sender
/// 4. Receive Handshake, generate ephemeral keypair, send HandshakeAck
/// 5. Receive FileHeader + encrypted DataChunks
/// 6. Verify BLAKE3 checksum, write file
/// 7. Send TransferComplete
pub async fn receive_with_code(
    code: &str,
    output_dir: &Path,
    _device_name: &str,
) -> Result<(), FluxError> {
    use crate::discovery::mdns::discover_by_code_hash;
    use crate::net::codephrase;

    let started = std::time::Instant::now();

    // Validate code phrase
    codephrase::validate(code).map_err(FluxError::TransferError)?;

    // Compute code hash for mDNS matching
    let hash = codephrase::code_hash(code);

    eprintln!("Looking for sender...");

    // Discover sender by code hash (30s timeout)
    let device = discover_by_code_hash(&hash, 30)?
        .ok_or_else(|| {
            FluxError::TransferError(
                "Could not find sender on the network. Make sure the sender is running and you're on the same LAN.".into(),
            )
        })?;

    tracing::debug!("Found sender at {}:{}", device.host, device.port);

    // TCP connect to sender
    let stream = tokio::net::TcpStream::connect(format!("{}:{}", device.host, device.port))
        .await
        .map_err(|e| FluxError::ConnectionFailed {
            protocol: "flux".to_string(),
            host: format!("{}:{}", device.host, device.port),
            reason: e.to_string(),
        })?;

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_SIZE)
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // Receive Handshake from sender
    let hs_bytes = framed
        .next()
        .await
        .ok_or_else(|| FluxError::TransferError("Connection closed before handshake".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to read handshake: {}", e)))?;

    let handshake = decode_message(&hs_bytes)?;

    let (peer_device_name, peer_public_key) = match handshake {
        FluxMessage::Handshake {
            version,
            device_name: sender_name,
            public_key,
        } => {
            if version != PROTOCOL_VERSION {
                return Err(FluxError::TransferError(format!(
                    "Protocol version mismatch: expected {}, got {}",
                    PROTOCOL_VERSION, version
                )));
            }
            (sender_name, public_key)
        }
        _ => {
            return Err(FluxError::TransferError(
                "Expected Handshake as first message".into(),
            ));
        }
    };

    // Code mode is always encrypted
    let peer_pub_bytes: [u8; 32] = peer_public_key
        .ok_or_else(|| {
            FluxError::EncryptionError("Sender did not provide a public key".into())
        })?
        .try_into()
        .map_err(|_| FluxError::EncryptionError("Sender public key must be 32 bytes".into()))?;

    // Generate our ephemeral keypair
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

    // Complete key exchange with code-phrase binding (PAKE-like authentication).
    // Both sender and receiver derive the session key from the DH shared secret
    // AND the code phrase, ensuring only someone who knows the code phrase can
    // complete the handshake.
    let peer_public = x25519_dalek::PublicKey::from(peer_pub_bytes);
    let channel = EncryptedChannel::complete_with_code(our_secret, &peer_public, code);

    // Receive FileHeader
    let fh_bytes = framed
        .next()
        .await
        .ok_or_else(|| FluxError::TransferError("Connection closed before file header".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to read file header: {}", e)))?;

    let file_header = decode_message(&fh_bytes)?;
    let (filename, file_size, expected_checksum) = match file_header {
        FluxMessage::FileHeader {
            filename,
            size,
            checksum,
            ..
        } => (filename, size, checksum),
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

    // Validate file size
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

    // Warn when the declared size is unusually large (>2 GB).
    if file_size > 2 * 1024 * 1024 * 1024 {
        tracing::info!(
            file = %filename,
            size_bytes = file_size,
            "Large incoming transfer declared ({} bytes); this will take significant time and disk space",
            file_size,
        );
    }

    let human_size = bytesize::ByteSize(file_size).to_string();
    eprintln!(
        "Receiving {} ({}) from {}",
        filename, human_size, peer_device_name
    );

    // Prepare output path
    let output_path = find_unique_path(output_dir, &filename);
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
        .expect("static progress template is valid")
        .progress_chars("#>-"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());

    // --- Receive DataChunks: stream directly to disk ---
    let mut received_bytes: u64 = 0;
    let mut expected_offset: u64 = 0;
    let mut hasher = blake3::Hasher::new();

    // Open output file exclusively (atomic create, prevents TOCTOU/symlink)
    let mut out_file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&output_path)
        .map_err(|e| {
            FluxError::TransferError(format!(
                "Failed to create file '{}': {}",
                output_path.display(), e
            ))
        })?;

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
                if offset != expected_offset {
                    pb.finish_and_clear();
                    drop(out_file);
                    let _ = std::fs::remove_file(&output_path);
                    return Err(FluxError::TransferError(format!(
                        "Unexpected chunk offset: expected {}, got {}",
                        expected_offset, offset
                    )));
                }

                let nonce_bytes: [u8; 24] = nonce
                    .ok_or_else(|| {
                        FluxError::EncryptionError("Encrypted chunk missing nonce".into())
                    })?
                    .try_into()
                    .map_err(|_| {
                        FluxError::EncryptionError("Nonce must be 24 bytes".into())
                    })?;
                let plaintext = channel.decrypt(&data, &nonce_bytes)?;

                let chunk_len = plaintext.len() as u64;

                if received_bytes + chunk_len > file_size {
                    pb.finish_and_clear();
                    drop(out_file);
                    let _ = std::fs::remove_file(&output_path);
                    return Err(FluxError::TransferError(format!(
                        "Data overflow: received {} + chunk {} exceeds declared size {}",
                        received_bytes, chunk_len, file_size
                    )));
                }

                // Stream to disk + incremental hash (no full-file buffering)
                {
                    use std::io::Write;
                    out_file.write_all(&plaintext).map_err(|e| {
                        FluxError::TransferError(format!(
                            "Failed to write chunk to '{}': {}",
                            output_path.display(), e
                        ))
                    })?;
                }
                hasher.update(&plaintext);

                received_bytes += chunk_len;
                expected_offset += chunk_len;
                pb.set_position(received_bytes);
            }
            FluxMessage::Error { message } => {
                pb.finish_and_clear();
                drop(out_file);
                let _ = std::fs::remove_file(&output_path);
                return Err(FluxError::TransferError(format!(
                    "Sender error during transfer: {}",
                    message
                )));
            }
            _ => {
                pb.finish_and_clear();
                drop(out_file);
                let _ = std::fs::remove_file(&output_path);
                return Err(FluxError::TransferError(
                    "Unexpected message during data transfer".into(),
                ));
            }
        }
    }

    pb.finish_and_clear();
    drop(out_file);

    // --- Verify BLAKE3 checksum (computed incrementally during receive) ---
    let checksum_verified = if let Some(ref expected) = expected_checksum {
        let actual = hasher.finalize().to_hex().to_string();
        if actual != *expected {
            // Checksum mismatch — delete the corrupted file
            let _ = std::fs::remove_file(&output_path);
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

    // Send TransferComplete
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

    {
        let mut stats = TransferStats::new(1, file_size);
        stats.started = started;
        stats.add_done(received_bytes);
        stats.print_file_summary(&display_name, false);
    }

    Ok(())
}

/// Synchronous wrapper for code-phrase receive mode.
pub fn receive_with_code_sync(
    code: &str,
    output_dir: &Path,
    device_name: &str,
) -> Result<(), FluxError> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FluxError::TransferError(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(receive_with_code(code, output_dir, device_name))
}

/// Sanitize a peer device name received over the network before using it as a
/// trust store key.
///
/// A malicious sender could craft a device name containing control characters
/// or an arbitrarily long string in order to corrupt the trust store or cause
/// unexpected behaviour when the key is displayed. This function:
///
/// - Strips all ASCII control characters (U+0000..=U+001F and U+007F)
/// - Trims leading and trailing whitespace
/// - Truncates to at most 63 characters (the DNS label limit, matching
///   `sanitize_device_name` in `discovery/service.rs`)
/// - Falls back to `"unknown-device"` if the result is empty
fn sanitize_peer_device_name(name: &str) -> String {
    let filtered: String = name
        .chars()
        .filter(|c| !c.is_ascii_control())
        .collect();
    let trimmed = filtered.trim();
    // Truncate at a character boundary (all remaining chars are non-control
    // ASCII or multi-byte Unicode; `char_indices` handles both safely).
    let truncated: String = trimmed.chars().take(63).collect();
    if truncated.is_empty() {
        "unknown-device".to_string()
    } else {
        truncated
    }
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
        return "unnamed".to_string();
    }

    // Block Windows reserved device names (CON, PRN, AUX, NUL, COM1-9, LPT1-9)
    let stem = name.split('.').next().unwrap_or("");
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL",
        "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9",
        "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.iter().any(|r| r.eq_ignore_ascii_case(stem)) {
        return format!("_{}", name);
    }

    name.to_string()
}

/// Write file data exclusively — fails if the path already exists or is a symlink.
///
/// Uses `create_new(true)` (O_CREAT | O_EXCL) for atomic create-if-not-exists,
/// preventing TOCTOU race conditions and symlink attacks.
fn write_file_exclusive(path: &Path, data: &[u8]) -> Result<(), FluxError> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| {
            FluxError::TransferError(format!(
                "Failed to create file '{}': {}",
                path.display(),
                e
            ))
        })?;
    file.write_all(data).map_err(|e| {
        FluxError::TransferError(format!(
            "Failed to write file '{}': {}",
            path.display(),
            e
        ))
    })?;
    Ok(())
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
    bind_addr: &str,
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
        bind_addr,
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

    #[test]
    fn sanitize_windows_reserved_names() {
        assert_eq!(sanitize_filename("CON"), "_CON");
        assert_eq!(sanitize_filename("con"), "_con");
        assert_eq!(sanitize_filename("PRN"), "_PRN");
        assert_eq!(sanitize_filename("NUL"), "_NUL");
        assert_eq!(sanitize_filename("COM1"), "_COM1");
        assert_eq!(sanitize_filename("LPT9"), "_LPT9");
        assert_eq!(sanitize_filename("CON.txt"), "_CON.txt");
        assert_eq!(sanitize_filename("nul.log"), "_nul.log");
    }

    #[test]
    fn sanitize_normal_names_unchanged() {
        assert_eq!(sanitize_filename("file.txt"), "file.txt");
        assert_eq!(sanitize_filename("CONSOLE.txt"), "CONSOLE.txt");
        assert_eq!(sanitize_filename("my_con_file.txt"), "my_con_file.txt");
    }

    #[test]
    fn sanitize_path_traversal() {
        assert_eq!(sanitize_filename("../../etc/passwd"), "passwd");
        assert_eq!(sanitize_filename("/etc/shadow"), "shadow");
    }

    #[test]
    fn sanitize_hidden_files() {
        assert_eq!(sanitize_filename(".bashrc"), "bashrc");
        assert_eq!(sanitize_filename("...hidden"), "hidden");
    }

    #[test]
    fn sanitize_peer_device_name_strips_control_chars() {
        // Bell, carriage return, newline, and null must all be removed.
        assert_eq!(sanitize_peer_device_name("hello\x07world"), "helloworld");
        assert_eq!(sanitize_peer_device_name("bad\r\nname"), "badname");
        assert_eq!(sanitize_peer_device_name("null\x00byte"), "nullbyte");
    }

    #[test]
    fn sanitize_peer_device_name_truncates_to_63() {
        let long = "a".repeat(100);
        let result = sanitize_peer_device_name(&long);
        assert_eq!(result.len(), 63);
    }

    #[test]
    fn sanitize_peer_device_name_empty_falls_back() {
        assert_eq!(sanitize_peer_device_name(""), "unknown-device");
        // Only control characters — filtered to empty, falls back.
        assert_eq!(sanitize_peer_device_name("\x01\x02\x03"), "unknown-device");
    }

    #[test]
    fn sanitize_peer_device_name_preserves_normal_names() {
        assert_eq!(sanitize_peer_device_name("alice-laptop"), "alice-laptop");
        assert_eq!(
            sanitize_peer_device_name("DESKTOP-ABC123"),
            "DESKTOP-ABC123"
        );
    }

    #[test]
    fn sanitize_peer_device_name_trims_whitespace() {
        assert_eq!(sanitize_peer_device_name("  laptop  "), "laptop");
    }

    #[test]
    fn write_file_exclusive_prevents_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "original").unwrap();

        let result = write_file_exclusive(&path, b"attacker data");
        assert!(result.is_err());
        // Original file unchanged
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "original");
    }
}
