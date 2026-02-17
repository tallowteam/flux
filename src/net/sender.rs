//! TCP client for sending files to Flux receivers.
//!
//! Connects to a Flux receiver, performs protocol handshake (with optional
//! encryption key exchange), and streams file data in chunks.

use std::path::Path;
use std::time::Instant;

use futures::{SinkExt, StreamExt};
use tokio_util::bytes::Bytes;
use tokio::net::TcpStream;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

use crate::discovery::mdns::discover_flux_devices;
use crate::discovery::service::DEFAULT_PORT;
use crate::error::FluxError;
use crate::net::protocol::{
    decode_message, encode_message, FluxMessage, CHUNK_SIZE, MAX_FRAME_SIZE, PROTOCOL_VERSION,
};
use crate::security::crypto::EncryptedChannel;
use crate::transfer::stats::TransferStats;

/// Timeout for receiving HandshakeAck from the receiver.
const HANDSHAKE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Timeout for receiving TransferComplete from the receiver after all data is sent.
const COMPLETION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(300);

/// Send a file to a remote Flux receiver over TCP.
///
/// Performs the full transfer lifecycle:
/// 1. Connect to host:port via TCP
/// 2. Send Handshake (with optional public key for encryption)
/// 3. Receive HandshakeAck (reject => error)
/// 4. If encrypting: complete key exchange to create EncryptedChannel
/// 5. Send FileHeader with filename and size
/// 6. Stream DataChunks (encrypted if requested)
/// 7. Wait for TransferComplete acknowledgement
pub async fn send_file(
    host: &str,
    port: u16,
    file_path: &Path,
    encrypt: bool,
    device_name: &str,
) -> Result<(), FluxError> {
    let started = Instant::now();

    // Connect to the receiver
    let stream = TcpStream::connect(format!("{}:{}", host, port))
        .await
        .map_err(|e| FluxError::ConnectionFailed {
            protocol: "flux".to_string(),
            host: format!("{}:{}", host, port),
            reason: e.to_string(),
        })?;

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_SIZE)
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // --- Handshake ---
    let (ephemeral_secret, our_public_key) = if encrypt {
        let (secret, public) = EncryptedChannel::initiate();
        (Some(secret), Some(public.as_bytes().to_vec()))
    } else {
        (None, None)
    };

    let handshake = FluxMessage::Handshake {
        version: PROTOCOL_VERSION,
        device_name: device_name.to_string(),
        public_key: our_public_key,
    };
    framed
        .send(Bytes::from(encode_message(&handshake)?))
        .await
        .map_err(|e| FluxError::TransferError(format!("Failed to send handshake: {}", e)))?;

    // Wait for HandshakeAck (with timeout to prevent indefinite stalls)
    let ack_bytes = tokio::time::timeout(HANDSHAKE_TIMEOUT, framed.next())
        .await
        .map_err(|_| FluxError::TransferError("Timed out waiting for handshake response".into()))?
        .ok_or_else(|| FluxError::TransferError("Connection closed during handshake".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to receive handshake ack: {}", e)))?;

    let ack = decode_message(&ack_bytes)?;
    let channel = match ack {
        FluxMessage::HandshakeAck {
            accepted,
            public_key: peer_key,
            reason,
        } => {
            if !accepted {
                return Err(FluxError::TransferError(format!(
                    "Connection rejected: {}",
                    reason.unwrap_or_else(|| "unknown reason".into())
                )));
            }
            if encrypt {
                // Complete key exchange
                let peer_pub_bytes: [u8; 32] = peer_key
                    .ok_or_else(|| {
                        FluxError::EncryptionError(
                            "Peer accepted encryption but sent no public key".into(),
                        )
                    })?
                    .try_into()
                    .map_err(|_| {
                        FluxError::EncryptionError("Peer public key must be 32 bytes".into())
                    })?;
                let peer_public = x25519_dalek::PublicKey::from(peer_pub_bytes);
                Some(EncryptedChannel::complete(
                    ephemeral_secret.expect("ephemeral_secret is Some when encrypt is true"),
                    &peer_public,
                ))
            } else {
                None
            }
        }
        FluxMessage::Error { message } => {
            return Err(FluxError::TransferError(format!("Peer error: {}", message)));
        }
        _ => {
            return Err(FluxError::TransferError(
                "Unexpected message during handshake".into(),
            ));
        }
    };

    // --- File metadata ---
    let file_meta = std::fs::metadata(file_path).map_err(|e| {
        FluxError::TransferError(format!(
            "Cannot read file '{}': {}",
            file_path.display(),
            e
        ))
    })?;
    let file_size = file_meta.len();

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    // --- Pass 1: Compute BLAKE3 checksum by streaming from disk ---
    let checksum = {
        use std::io::Read;
        let mut file = std::fs::File::open(file_path).map_err(|e| {
            FluxError::TransferError(format!("Failed to open '{}': {}", file_path.display(), e))
        })?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = file.read(&mut buf).map_err(|e| {
                FluxError::TransferError(format!("Failed to read '{}': {}", file_path.display(), e))
            })?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        hasher.finalize().to_hex().to_string()
    };

    let header = FluxMessage::FileHeader {
        filename: filename.clone(),
        size: file_size,
        checksum: Some(checksum),
        encrypted: encrypt,
    };
    framed
        .send(Bytes::from(encode_message(&header)?))
        .await
        .map_err(|e| FluxError::TransferError(format!("Failed to send file header: {}", e)))?;

    // --- Pass 2: Stream file data in chunks ---
    let mut offset: u64 = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];

    let pb = indicatif::ProgressBar::new(file_size);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .expect("static progress template is valid")
        .progress_chars("#>-"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());

    {
        use std::io::Read;
        let mut file = std::fs::File::open(file_path).map_err(|e| {
            FluxError::TransferError(format!("Failed to open '{}': {}", file_path.display(), e))
        })?;
        loop {
            let n = file.read(&mut buf).map_err(|e| {
                FluxError::TransferError(format!("Failed to read '{}': {}", file_path.display(), e))
            })?;
            if n == 0 { break; }

            let raw_data = &buf[..n];
            let (data, nonce) = if let Some(ref ch) = channel {
                let (ct, nc) = ch.encrypt(raw_data)?;
                (ct, Some(nc.to_vec()))
            } else {
                (raw_data.to_vec(), None)
            };

            let chunk_msg = FluxMessage::DataChunk {
                offset,
                data,
                nonce,
            };
            framed
                .send(Bytes::from(encode_message(&chunk_msg)?))
                .await
                .map_err(|e| FluxError::TransferError(format!("Failed to send data chunk: {}", e)))?;

            offset += n as u64;
            pb.set_position(offset);
        }
    }

    pb.finish_and_clear();

    // --- Wait for TransferComplete (with timeout) ---
    let complete_bytes = tokio::time::timeout(COMPLETION_TIMEOUT, framed.next())
        .await
        .map_err(|_| FluxError::TransferError("Timed out waiting for transfer confirmation".into()))?
        .ok_or_else(|| {
            FluxError::TransferError("Connection closed before transfer complete".into())
        })?
        .map_err(|e| {
            FluxError::TransferError(format!("Failed to receive transfer complete: {}", e))
        })?;

    let complete = decode_message(&complete_bytes)?;
    match complete {
        FluxMessage::TransferComplete {
            bytes_received, ..
        } => {
            let mut stats = TransferStats::new(1, file_size);
            stats.started = started;
            stats.add_done(bytes_received);
            stats.print_file_summary(&filename, false);
        }
        FluxMessage::Error { message } => {
            return Err(FluxError::TransferError(format!(
                "Receiver error: {}",
                message
            )));
        }
        _ => {
            return Err(FluxError::TransferError(
                "Unexpected message after data transfer".into(),
            ));
        }
    }

    Ok(())
}

/// Send a file using code-phrase mode (Croc-like UX).
///
/// The sender becomes a TCP server:
/// 1. Generate (or validate custom) code phrase
/// 2. Bind TCP on OS-assigned port
/// 3. Register mDNS with code_hash TXT property
/// 4. Print code phrase and wait for receiver
/// 5. Accept one connection, perform encrypted transfer
///
/// Always encrypted -- no `--encrypt` flag needed.
pub async fn send_with_code(
    file_path: &Path,
    device_name: &str,
    code_override: Option<&str>,
) -> Result<(), FluxError> {
    use crate::discovery::mdns::register_flux_service;
    use crate::discovery::service::FluxService;
    use crate::net::codephrase;
    use tokio::net::TcpListener;

    let started = Instant::now();

    // Generate or validate code phrase
    let code = if let Some(custom) = code_override {
        codephrase::validate(custom).map_err(FluxError::TransferError)?;
        custom.to_string()
    } else {
        codephrase::generate()
    };

    // Verify file exists and read metadata
    if !file_path.exists() {
        return Err(FluxError::SourceNotFound {
            path: file_path.to_path_buf(),
        });
    }

    let file_meta = std::fs::metadata(file_path).map_err(|e| {
        FluxError::TransferError(format!(
            "Cannot read file '{}': {}",
            file_path.display(),
            e
        ))
    })?;
    let file_size = file_meta.len();

    let filename = file_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unnamed".to_string());

    // Compute BLAKE3 checksum by streaming from disk (no full-file buffering)
    let checksum = {
        use std::io::Read;
        let mut file = std::fs::File::open(file_path).map_err(|e| {
            FluxError::TransferError(format!("Failed to open '{}': {}", file_path.display(), e))
        })?;
        let mut hasher = blake3::Hasher::new();
        let mut buf = vec![0u8; CHUNK_SIZE];
        loop {
            let n = file.read(&mut buf).map_err(|e| {
                FluxError::TransferError(format!("Failed to read '{}': {}", file_path.display(), e))
            })?;
            if n == 0 { break; }
            hasher.update(&buf[..n]);
        }
        hasher.finalize().to_hex().to_string()
    };

    // Bind TCP on port 0 (OS-assigned)
    let listener = TcpListener::bind("0.0.0.0:0")
        .await
        .map_err(|e| FluxError::TransferError(format!("Failed to bind TCP listener: {}", e)))?;

    let local_addr = listener.local_addr().map_err(|e| {
        FluxError::TransferError(format!("Failed to get local address: {}", e))
    })?;
    let actual_port = local_addr.port();

    // Generate ephemeral X25519 keypair (always encrypted in code mode)
    let (ephemeral_secret, our_public) = EncryptedChannel::initiate();
    let our_pub_bytes = our_public.as_bytes().to_vec();

    // Register mDNS with code_hash TXT property
    let hash = codephrase::code_hash(&code);
    let service = FluxService::new(Some(device_name.to_string()), actual_port);
    let _mdns_daemon = register_flux_service(&service, None, Some(&hash))?;

    // Print code phrase and instructions
    let human_size = bytesize::ByteSize(file_size).to_string();
    eprintln!("Code phrase: {}", code);
    eprintln!("On the other device run:");
    eprintln!("  flux receive {}", code);
    eprintln!(
        "Sending {} ({}) - waiting for receiver...",
        filename, human_size
    );

    // Accept one connection (with timeout)
    let (stream, peer_addr) = tokio::time::timeout(
        std::time::Duration::from_secs(5 * 60),
        listener.accept(),
    )
    .await
    .map_err(|_| FluxError::TransferError("Timed out waiting for receiver (5 minutes)".into()))?
    .map_err(|e| FluxError::TransferError(format!("Failed to accept connection: {}", e)))?;

    tracing::debug!("Connection from {}", peer_addr);

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(MAX_FRAME_SIZE)
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // Send Handshake with public key
    let handshake = FluxMessage::Handshake {
        version: PROTOCOL_VERSION,
        device_name: device_name.to_string(),
        public_key: Some(our_pub_bytes),
    };
    framed
        .send(Bytes::from(encode_message(&handshake)?))
        .await
        .map_err(|e| FluxError::TransferError(format!("Failed to send handshake: {}", e)))?;

    // Wait for HandshakeAck (with timeout)
    let ack_bytes = tokio::time::timeout(HANDSHAKE_TIMEOUT, framed.next())
        .await
        .map_err(|_| FluxError::TransferError("Timed out waiting for handshake response".into()))?
        .ok_or_else(|| FluxError::TransferError("Connection closed during handshake".into()))?
        .map_err(|e| FluxError::TransferError(format!("Failed to receive handshake ack: {}", e)))?;

    let ack = decode_message(&ack_bytes)?;
    let channel = match ack {
        FluxMessage::HandshakeAck {
            accepted,
            public_key: peer_key,
            reason,
        } => {
            if !accepted {
                return Err(FluxError::TransferError(format!(
                    "Connection rejected: {}",
                    reason.unwrap_or_else(|| "unknown reason".into())
                )));
            }
            let peer_pub_bytes: [u8; 32] = peer_key
                .ok_or_else(|| {
                    FluxError::EncryptionError(
                        "Receiver accepted but sent no public key".into(),
                    )
                })?
                .try_into()
                .map_err(|_| {
                    FluxError::EncryptionError("Receiver public key must be 32 bytes".into())
                })?;
            let peer_public = x25519_dalek::PublicKey::from(peer_pub_bytes);
            // Bind code phrase to key exchange (PAKE-like authentication)
            EncryptedChannel::complete_with_code(ephemeral_secret, &peer_public, &code)
        }
        FluxMessage::Error { message } => {
            return Err(FluxError::TransferError(format!("Peer error: {}", message)));
        }
        _ => {
            return Err(FluxError::TransferError(
                "Unexpected message during handshake".into(),
            ));
        }
    };

    // Send FileHeader
    let header = FluxMessage::FileHeader {
        filename: filename.clone(),
        size: file_size,
        checksum: Some(checksum),
        encrypted: true,
    };
    framed
        .send(Bytes::from(encode_message(&header)?))
        .await
        .map_err(|e| FluxError::TransferError(format!("Failed to send file header: {}", e)))?;

    // Stream encrypted DataChunks from disk (no full-file buffering)
    let mut offset: u64 = 0;
    let mut buf = vec![0u8; CHUNK_SIZE];

    let pb = indicatif::ProgressBar::new(file_size);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({bytes_per_sec})",
        )
        .expect("static progress template is valid")
        .progress_chars("#>-"),
    );
    pb.set_draw_target(indicatif::ProgressDrawTarget::stderr());

    {
        use std::io::Read;
        let mut file = std::fs::File::open(file_path).map_err(|e| {
            FluxError::TransferError(format!("Failed to open '{}': {}", file_path.display(), e))
        })?;
        loop {
            let n = file.read(&mut buf).map_err(|e| {
                FluxError::TransferError(format!("Failed to read '{}': {}", file_path.display(), e))
            })?;
            if n == 0 { break; }

            let raw_data = &buf[..n];
            let (data, nonce) = channel.encrypt(raw_data)?;

            let chunk_msg = FluxMessage::DataChunk {
                offset,
                data,
                nonce: Some(nonce.to_vec()),
            };
            framed
                .send(Bytes::from(encode_message(&chunk_msg)?))
                .await
                .map_err(|e| FluxError::TransferError(format!("Failed to send data chunk: {}", e)))?;

            offset += n as u64;
            pb.set_position(offset);
        }
    }

    pb.finish_and_clear();

    // Wait for TransferComplete (with timeout)
    let complete_bytes = tokio::time::timeout(COMPLETION_TIMEOUT, framed.next())
        .await
        .map_err(|_| FluxError::TransferError("Timed out waiting for transfer confirmation".into()))?
        .ok_or_else(|| {
            FluxError::TransferError("Connection closed before transfer complete".into())
        })?
        .map_err(|e| {
            FluxError::TransferError(format!("Failed to receive transfer complete: {}", e))
        })?;

    let complete = decode_message(&complete_bytes)?;
    match complete {
        FluxMessage::TransferComplete {
            bytes_received, ..
        } => {
            let mut stats = TransferStats::new(1, file_size);
            stats.started = started;
            stats.add_done(bytes_received);
            stats.print_file_summary(&filename, false);
        }
        FluxMessage::Error { message } => {
            return Err(FluxError::TransferError(format!(
                "Receiver error: {}",
                message
            )));
        }
        _ => {
            return Err(FluxError::TransferError(
                "Unexpected message after data transfer".into(),
            ));
        }
    }

    Ok(())
}

/// Synchronous wrapper for code-phrase send mode.
pub fn send_with_code_sync(
    file_path: &Path,
    device_name: &str,
    code_override: Option<&str>,
) -> Result<(), FluxError> {
    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FluxError::TransferError(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(send_with_code(file_path, device_name, code_override))
}

/// Resolve a target string to (host, port).
///
/// Formats supported:
/// - `@devicename` -- discover device via mDNS, resolve to its IP:port
/// - `host:port` -- direct address
/// - `host` -- use DEFAULT_PORT
pub fn resolve_device_target(target: &str) -> Result<(String, u16), FluxError> {
    if target.starts_with('@') {
        let name = &target[1..];
        if name.is_empty() {
            return Err(FluxError::TransferError(
                "Empty device name after @".into(),
            ));
        }

        eprintln!("Discovering device '{}'...", name);
        let devices = discover_flux_devices(3)?;

        // Case-insensitive prefix match
        let name_lower = name.to_lowercase();
        let found = devices.iter().find(|d| {
            d.name.to_lowercase() == name_lower
                || d.name.to_lowercase().starts_with(&name_lower)
        });

        match found {
            Some(device) => Ok((device.host.clone(), device.port)),
            None => Err(FluxError::TransferError(format!(
                "Device '{}' not found on the network. Found {} device(s).",
                name,
                devices.len()
            ))),
        }
    } else if let Some(colon_pos) = target.rfind(':') {
        // Check if it looks like host:port (not just IPv6)
        let port_str = &target[colon_pos + 1..];
        if let Ok(port) = port_str.parse::<u16>() {
            let host = &target[..colon_pos];
            Ok((host.to_string(), port))
        } else {
            // Could not parse port -- treat whole thing as host
            Ok((target.to_string(), DEFAULT_PORT))
        }
    } else {
        Ok((target.to_string(), DEFAULT_PORT))
    }
}

/// Synchronous wrapper for sending a file.
///
/// Creates a local tokio runtime, resolves the target, and sends the file.
/// This is the entry point called from main.rs.
pub fn send_file_sync(
    target: &str,
    file_path: &Path,
    encrypt: bool,
    device_name: &str,
) -> Result<(), FluxError> {
    let (host, port) = resolve_device_target(target)?;

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FluxError::TransferError(format!("Failed to create async runtime: {}", e)))?;

    rt.block_on(send_file(&host, port, file_path, encrypt, device_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_host_port() {
        let (host, port) = resolve_device_target("192.168.1.50:8080").unwrap();
        assert_eq!(host, "192.168.1.50");
        assert_eq!(port, 8080);
    }

    #[test]
    fn resolve_host_only() {
        let (host, port) = resolve_device_target("192.168.1.50").unwrap();
        assert_eq!(host, "192.168.1.50");
        assert_eq!(port, DEFAULT_PORT);
    }

    #[test]
    fn resolve_localhost() {
        let (host, port) = resolve_device_target("127.0.0.1:9741").unwrap();
        assert_eq!(host, "127.0.0.1");
        assert_eq!(port, 9741);
    }

    #[test]
    fn resolve_at_empty_name_errors() {
        let result = resolve_device_target("@");
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Empty device name"));
    }

    #[test]
    fn resolve_invalid_port_uses_default() {
        let (host, port) = resolve_device_target("myhost:notaport").unwrap();
        assert_eq!(host, "myhost:notaport");
        assert_eq!(port, DEFAULT_PORT);
    }
}
