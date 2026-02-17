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

    // Wait for HandshakeAck
    let ack_bytes = framed
        .next()
        .await
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
                    ephemeral_secret.unwrap(),
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

    // --- Read file data and compute BLAKE3 checksum ---
    let file_data = std::fs::read(file_path).map_err(|e| {
        FluxError::TransferError(format!(
            "Failed to read file '{}': {}",
            file_path.display(),
            e
        ))
    })?;

    let checksum = blake3::hash(&file_data).to_hex().to_string();

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

    let mut offset: u64 = 0;
    let total = file_data.len();
    let mut chunk_start = 0usize;

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

    while chunk_start < total {
        let chunk_end = std::cmp::min(chunk_start + CHUNK_SIZE, total);
        let raw_data = &file_data[chunk_start..chunk_end];

        let (data, nonce) = if let Some(ref ch) = channel {
            let (ct, n) = ch.encrypt(raw_data)?;
            (ct, Some(n.to_vec()))
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

        offset += (chunk_end - chunk_start) as u64;
        chunk_start = chunk_end;
        pb.set_position(offset);
    }

    pb.finish_and_clear();

    // --- Wait for TransferComplete ---
    let complete_bytes = framed
        .next()
        .await
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
            let elapsed = started.elapsed();
            eprintln!(
                "Sent: {} ({} bytes) in {:.1}s",
                filename,
                bytes_received,
                elapsed.as_secs_f64()
            );
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
