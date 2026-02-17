//! Transfer protocol message types for Flux peer-to-peer communication.
//!
//! Defines the framed message protocol used over TCP between Flux instances.
//! Messages are serialized with bincode 2.x (via serde) and framed with
//! length-delimited encoding (4-byte big-endian length prefix) for TCP transport.

use serde::{Deserialize, Serialize};

use crate::error::FluxError;

/// Current protocol version. Incremented on breaking changes.
pub const PROTOCOL_VERSION: u8 = 1;

/// Maximum frame size for LengthDelimitedCodec (2 MB).
///
/// This limits the maximum size of a single framed message on the wire.
/// Data chunks are well below this limit (256 KB default), but the headroom
/// accommodates encryption overhead and larger handshake messages.
pub const MAX_FRAME_SIZE: usize = 2 * 1024 * 1024;

/// Default data chunk size for file transfer (256 KB).
///
/// Matches the existing buffer sizes used throughout Flux (BufReader/BufWriter
/// in Phase 1, parallel chunk transfers in Phase 2). Each DataChunk message
/// carries at most this many bytes of file data.
pub const CHUNK_SIZE: usize = 256 * 1024;

/// Protocol messages exchanged between Flux peers during file transfer.
///
/// The transfer lifecycle follows this sequence:
/// 1. Sender sends `Handshake` to identify and optionally request encryption
/// 2. Receiver replies with `HandshakeAck` (accept/reject, optional public key)
/// 3. Sender sends `FileHeader` with file metadata
/// 4. Sender sends one or more `DataChunk` messages with file data
/// 5. Receiver sends `TransferComplete` acknowledgement
/// 6. Either side may send `Error` at any point to abort
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum FluxMessage {
    /// Initial handshake from sender to receiver.
    ///
    /// Identifies the sender and optionally requests encryption by including
    /// a 32-byte X25519 public key.
    Handshake {
        /// Protocol version (must match PROTOCOL_VERSION)
        version: u8,
        /// Friendly device name of the sender
        device_name: String,
        /// X25519 public key (32 bytes) when encryption is requested
        public_key: Option<Vec<u8>>,
    },

    /// Receiver's response to the handshake.
    ///
    /// If accepted and encryption was requested, includes the receiver's
    /// X25519 public key for key exchange completion.
    HandshakeAck {
        /// Whether the connection is accepted
        accepted: bool,
        /// Receiver's X25519 public key (32 bytes) for encrypted sessions
        public_key: Option<Vec<u8>>,
        /// Reason for rejection (when accepted is false)
        reason: Option<String>,
    },

    /// File metadata sent before data transfer begins.
    ///
    /// The receiver uses this to prepare for the incoming file (create the
    /// file, pre-allocate space, set up progress tracking).
    FileHeader {
        /// File name (not a full path -- receiver decides where to save)
        filename: String,
        /// Total file size in bytes
        size: u64,
        /// Optional BLAKE3 checksum for verification (hex-encoded)
        checksum: Option<String>,
        /// Whether the data chunks are encrypted
        encrypted: bool,
    },

    /// A chunk of file data.
    ///
    /// Chunks are sent sequentially with increasing offsets. When encrypted,
    /// each chunk includes a 24-byte XChaCha20 nonce. The receiver writes
    /// each chunk at the specified offset.
    DataChunk {
        /// Byte offset within the file
        offset: u64,
        /// Raw file data (or encrypted data when encryption is active)
        data: Vec<u8>,
        /// XChaCha20 nonce (24 bytes) when the chunk is encrypted
        nonce: Option<Vec<u8>>,
    },

    /// Acknowledgement from receiver after all data has been received.
    ///
    /// Confirms the transfer is complete and optionally reports whether
    /// the checksum was verified.
    TransferComplete {
        /// The filename that was transferred
        filename: String,
        /// Total bytes received
        bytes_received: u64,
        /// Whether the checksum matched (None if no checksum was provided)
        checksum_verified: Option<bool>,
    },

    /// Error message that can be sent by either side to abort the transfer.
    Error {
        /// Human-readable error description
        message: String,
    },
}

/// Encode a FluxMessage into bytes using bincode 2.x (serde mode).
///
/// Uses `bincode::serde::encode_to_vec` with standard configuration.
/// The resulting bytes are suitable for framing with LengthDelimitedCodec.
pub fn encode_message(msg: &FluxMessage) -> Result<Vec<u8>, FluxError> {
    bincode::serde::encode_to_vec(msg, bincode::config::standard()).map_err(|e| {
        FluxError::TransferError(format!("Failed to encode message: {}", e))
    })
}

/// Decode a FluxMessage from bytes using bincode 2.x (serde mode).
///
/// Uses `bincode::serde::decode_from_slice` with standard configuration.
/// Returns the decoded message (discarding the bytes-read count).
pub fn decode_message(bytes: &[u8]) -> Result<FluxMessage, FluxError> {
    let (msg, _bytes_read): (FluxMessage, usize) =
        bincode::serde::decode_from_slice(bytes, bincode::config::standard()).map_err(|e| {
            FluxError::TransferError(format!("Failed to decode message: {}", e))
        })?;
    Ok(msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_version_is_one() {
        assert_eq!(PROTOCOL_VERSION, 1);
    }

    #[test]
    fn max_frame_size_is_2mb() {
        assert_eq!(MAX_FRAME_SIZE, 2 * 1024 * 1024);
        assert!(MAX_FRAME_SIZE > CHUNK_SIZE, "Frame size must exceed chunk size");
    }

    #[test]
    fn chunk_size_is_256kb() {
        assert_eq!(CHUNK_SIZE, 256 * 1024);
    }

    #[test]
    fn chunk_size_fits_in_frame() {
        // Data chunk with max chunk size + overhead must fit in a frame
        // Overhead: offset (8 bytes) + length prefix + nonce (24 bytes optional)
        // bincode overhead is small, well under 1 KB
        assert!(CHUNK_SIZE + 1024 < MAX_FRAME_SIZE);
    }

    #[test]
    fn roundtrip_handshake_without_key() {
        let msg = FluxMessage::Handshake {
            version: PROTOCOL_VERSION,
            device_name: "test-device".to_string(),
            public_key: None,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_handshake_with_public_key() {
        let key = vec![0xAB; 32]; // 32-byte X25519 public key
        let msg = FluxMessage::Handshake {
            version: PROTOCOL_VERSION,
            device_name: "alice-laptop".to_string(),
            public_key: Some(key.clone()),
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);

        // Verify the key is preserved
        if let FluxMessage::Handshake { public_key, .. } = decoded {
            assert_eq!(public_key.unwrap(), key);
        } else {
            panic!("Expected Handshake variant");
        }
    }

    #[test]
    fn roundtrip_handshake_ack_accepted() {
        let msg = FluxMessage::HandshakeAck {
            accepted: true,
            public_key: Some(vec![0xCD; 32]),
            reason: None,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_handshake_ack_rejected() {
        let msg = FluxMessage::HandshakeAck {
            accepted: false,
            public_key: None,
            reason: Some("Transfer rejected by user".to_string()),
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_file_header() {
        let msg = FluxMessage::FileHeader {
            filename: "report.pdf".to_string(),
            size: 1_048_576, // 1 MB
            checksum: Some("abc123def456".to_string()),
            encrypted: false,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_file_header_encrypted() {
        let msg = FluxMessage::FileHeader {
            filename: "secret.docx".to_string(),
            size: 5_000_000,
            checksum: None,
            encrypted: true,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_data_chunk_unencrypted() {
        let data = vec![0u8; CHUNK_SIZE];
        let msg = FluxMessage::DataChunk {
            offset: 0,
            data: data.clone(),
            nonce: None,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_data_chunk_encrypted_with_nonce() {
        let data = vec![0xFFu8; 1024];
        let nonce = vec![0x42u8; 24]; // 24-byte XChaCha20 nonce
        let msg = FluxMessage::DataChunk {
            offset: 262_144, // second chunk at 256KB offset
            data: data.clone(),
            nonce: Some(nonce.clone()),
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);

        // Verify nonce is preserved
        if let FluxMessage::DataChunk { nonce: n, .. } = decoded {
            assert_eq!(n.unwrap(), nonce);
        } else {
            panic!("Expected DataChunk variant");
        }
    }

    #[test]
    fn roundtrip_transfer_complete() {
        let msg = FluxMessage::TransferComplete {
            filename: "report.pdf".to_string(),
            bytes_received: 1_048_576,
            checksum_verified: Some(true),
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_transfer_complete_no_checksum() {
        let msg = FluxMessage::TransferComplete {
            filename: "photo.jpg".to_string(),
            bytes_received: 500_000,
            checksum_verified: None,
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn roundtrip_error() {
        let msg = FluxMessage::Error {
            message: "Disk full: cannot write file".to_string(),
        };
        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn encode_produces_compact_binary() {
        let msg = FluxMessage::Handshake {
            version: 1,
            device_name: "test".to_string(),
            public_key: None,
        };
        let encoded = encode_message(&msg).unwrap();

        // bincode should produce compact output -- much smaller than JSON
        // A simple handshake should be well under 100 bytes
        assert!(encoded.len() < 100, "Encoded size {} should be compact", encoded.len());
    }

    #[test]
    fn decode_garbage_returns_error() {
        let garbage = vec![0xFF, 0xFE, 0xFD, 0xFC];
        let result = decode_message(&garbage);
        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Failed to decode"));
    }

    #[test]
    fn decode_empty_bytes_returns_error() {
        let result = decode_message(&[]);
        assert!(result.is_err());
    }

    #[test]
    fn all_message_variants_are_distinct() {
        // Encode each variant and verify they produce different bytes
        let messages = vec![
            FluxMessage::Handshake {
                version: 1,
                device_name: "a".to_string(),
                public_key: None,
            },
            FluxMessage::HandshakeAck {
                accepted: true,
                public_key: None,
                reason: None,
            },
            FluxMessage::FileHeader {
                filename: "a".to_string(),
                size: 0,
                checksum: None,
                encrypted: false,
            },
            FluxMessage::DataChunk {
                offset: 0,
                data: vec![],
                nonce: None,
            },
            FluxMessage::TransferComplete {
                filename: "a".to_string(),
                bytes_received: 0,
                checksum_verified: None,
            },
            FluxMessage::Error {
                message: "a".to_string(),
            },
        ];

        let encoded: Vec<Vec<u8>> = messages.iter().map(|m| encode_message(m).unwrap()).collect();

        // Each variant should encode differently (at minimum the enum discriminant differs)
        for i in 0..encoded.len() {
            for j in (i + 1)..encoded.len() {
                assert_ne!(
                    encoded[i], encoded[j],
                    "Variants {} and {} should encode differently",
                    i, j
                );
            }
        }
    }

    #[test]
    fn data_chunk_max_size_encodes_within_frame_limit() {
        let msg = FluxMessage::DataChunk {
            offset: u64::MAX,
            data: vec![0u8; CHUNK_SIZE],
            nonce: Some(vec![0u8; 24]),
        };
        let encoded = encode_message(&msg).unwrap();
        assert!(
            encoded.len() < MAX_FRAME_SIZE,
            "Max chunk encoded size {} exceeds frame limit {}",
            encoded.len(),
            MAX_FRAME_SIZE
        );
    }
}
