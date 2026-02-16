//! Zstd compression and decompression for chunk data.
//!
//! Provides `compress_chunk` and `decompress_chunk` functions that operate
//! on in-memory byte slices. Each chunk is compressed independently, which
//! allows parallel decompression and chunk-level resume.
//!
//! For local-to-local copies, compression adds CPU overhead without reducing
//! I/O volume. It is primarily beneficial for network transfers (Phase 3).
//! The infrastructure is implemented here so it is ready when needed.

use std::io::Cursor;

use crate::error::FluxError;

/// Default zstd compression level.
///
/// Level 3 provides a good balance between compression ratio and speed.
pub const DEFAULT_COMPRESSION_LEVEL: i32 = 3;

/// Compress a chunk of data using zstd at the given compression level.
///
/// Returns the compressed bytes. For highly compressible data (text),
/// the output will be significantly smaller than input. For already-compressed
/// or random data, the output may be slightly larger due to framing overhead.
pub fn compress_chunk(data: &[u8], level: i32) -> Result<Vec<u8>, FluxError> {
    zstd::encode_all(Cursor::new(data), level).map_err(|e| {
        FluxError::CompressionError(format!("zstd compression failed: {}", e))
    })
}

/// Decompress a zstd-compressed chunk back to the original data.
pub fn decompress_chunk(data: &[u8]) -> Result<Vec<u8>, FluxError> {
    zstd::decode_all(Cursor::new(data)).map_err(|e| {
        FluxError::CompressionError(format!("zstd decompression failed: {}", e))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip_text() {
        let original = b"Hello, this is a test of zstd compression in Flux. \
            Repeated text compresses well. Repeated text compresses well. \
            Repeated text compresses well. Repeated text compresses well.";
        let compressed = compress_chunk(original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn compressed_text_is_smaller() {
        // Highly repetitive text should compress significantly
        let original: Vec<u8> = "The quick brown fox jumps over the lazy dog. "
            .repeat(100)
            .into_bytes();
        let compressed = compress_chunk(&original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        assert!(
            compressed.len() < original.len(),
            "Compressed size {} should be less than original size {}",
            compressed.len(),
            original.len()
        );
    }

    #[test]
    fn compress_decompress_empty_data() {
        let original = b"";
        let compressed = compress_chunk(original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn compress_decompress_random_binary() {
        // Binary data that may not compress well, but should roundtrip
        let original: Vec<u8> = (0..1024u32).map(|i| (i.wrapping_mul(7919) % 256) as u8).collect();
        let compressed = compress_chunk(&original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn compress_decompress_single_byte() {
        let original = b"X";
        let compressed = compress_chunk(original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();
        assert_eq!(decompressed, original);
    }

    #[test]
    fn compress_decompress_large_data() {
        // 1MB of compressible data
        let original: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
        let compressed = compress_chunk(&original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let decompressed = decompress_chunk(&compressed).unwrap();
        assert_eq!(decompressed.len(), original.len());
        assert_eq!(decompressed, original);
    }

    #[test]
    fn decompress_invalid_data_returns_error() {
        let garbage = b"this is not valid zstd data";
        let result = decompress_chunk(garbage);
        assert!(result.is_err());
        match result {
            Err(FluxError::CompressionError(msg)) => {
                assert!(msg.contains("decompression failed"));
            }
            other => panic!("Expected CompressionError, got: {:?}", other),
        }
    }

    #[test]
    fn different_compression_levels_all_roundtrip() {
        let original = b"Testing different compression levels for zstd roundtrip safety.";
        for level in [1, 3, 6, 10] {
            let compressed = compress_chunk(original, level).unwrap();
            let decompressed = decompress_chunk(&compressed).unwrap();
            assert_eq!(
                decompressed, original,
                "Failed roundtrip at level {}",
                level
            );
        }
    }
}
