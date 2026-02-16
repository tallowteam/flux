//! BLAKE3 checksum functions for file and chunk integrity verification.
//!
//! Provides `hash_file` for whole-file hashing and `hash_chunk` for hashing
//! a specific byte range of an open file using positional I/O.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use crate::error::FluxError;
use crate::transfer::parallel::read_at;

/// Buffer size for hashing: 64KB.
const HASH_BUF_SIZE: usize = 64 * 1024;

/// Compute the BLAKE3 hash of an entire file, returning the hex string.
///
/// Opens the file, reads it in 64KB chunks through a BLAKE3 Hasher,
/// and returns the finalized hash as a lowercase hex string.
pub fn hash_file(path: &Path) -> Result<String, FluxError> {
    let mut file = File::open(path).map_err(|e| match e.kind() {
        std::io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: path.to_path_buf(),
        },
        std::io::ErrorKind::PermissionDenied => FluxError::PermissionDenied {
            path: path.to_path_buf(),
        },
        _ => FluxError::Io { source: e },
    })?;

    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; HASH_BUF_SIZE];

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}

/// Compute the BLAKE3 hash of a specific byte range of a file.
///
/// Uses positional I/O (`read_at`) so this is safe to call from multiple
/// threads on the same file handle without synchronization.
///
/// # Arguments
/// * `file` - An open file handle (does not need exclusive access)
/// * `offset` - Byte offset to start reading from
/// * `length` - Number of bytes to hash
///
/// # Returns
/// The BLAKE3 hash as a lowercase hex string.
pub fn hash_chunk(file: &File, offset: u64, length: u64) -> Result<String, FluxError> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; HASH_BUF_SIZE];
    let mut remaining = length;
    let mut pos = offset;

    while remaining > 0 {
        let to_read = std::cmp::min(remaining, buf.len() as u64) as usize;
        let n = read_at(file, pos, &mut buf[..to_read])?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        pos += n as u64;
        remaining -= n as u64;
    }

    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: create a temp file with known content.
    fn create_temp_file(content: &[u8]) -> tempfile::NamedTempFile {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        tmp.write_all(content).expect("write content");
        tmp.flush().expect("flush");
        tmp
    }

    #[test]
    fn hash_file_consistent_for_known_content() {
        let content = b"Hello, BLAKE3! This is a test of file hashing.";
        let tmp = create_temp_file(content);

        let hash1 = hash_file(tmp.path()).unwrap();
        let hash2 = hash_file(tmp.path()).unwrap();

        // Same content produces same hash
        assert_eq!(hash1, hash2);
        // Hash is a 64-char hex string (BLAKE3 produces 256-bit / 32-byte output)
        assert_eq!(hash1.len(), 64);
        // All chars are hex
        assert!(hash1.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn hash_chunk_of_entire_file_equals_hash_file() {
        let content = b"Full file hash should match chunk hash of entire range.";
        let tmp = create_temp_file(content);

        let file_hash = hash_file(tmp.path()).unwrap();

        let file = File::open(tmp.path()).unwrap();
        let chunk_hash = hash_chunk(&file, 0, content.len() as u64).unwrap();

        assert_eq!(file_hash, chunk_hash);
    }

    #[test]
    fn hash_chunk_different_ranges_return_different_hashes() {
        // Create a file where first half is all 0x00 and second half is all 0xFF
        // so the two halves are guaranteed to produce different hashes.
        let mut content = vec![0x00u8; 512];
        content.extend(vec![0xFFu8; 512]);
        let tmp = create_temp_file(&content);

        let file = File::open(tmp.path()).unwrap();

        let hash_first_half = hash_chunk(&file, 0, 512).unwrap();
        let hash_second_half = hash_chunk(&file, 512, 512).unwrap();

        assert_ne!(hash_first_half, hash_second_half);
    }

    #[test]
    fn hash_file_empty_file_produces_valid_hash() {
        let tmp = create_temp_file(b"");

        let hash = hash_file(tmp.path()).unwrap();

        // Empty file should produce a valid 64-char hex hash
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));

        // BLAKE3 hash of empty input is well-defined
        let expected = blake3::hash(b"").to_hex().to_string();
        assert_eq!(hash, expected);
    }

    #[test]
    fn hash_chunk_empty_range_produces_valid_hash() {
        let content = b"some content";
        let tmp = create_temp_file(content);
        let file = File::open(tmp.path()).unwrap();

        let hash = hash_chunk(&file, 0, 0).unwrap();

        // Zero-length chunk should produce the empty-input hash
        let expected = blake3::hash(b"").to_hex().to_string();
        assert_eq!(hash, expected);
    }

    #[test]
    fn hash_file_nonexistent_returns_error() {
        let result = hash_file(Path::new("/nonexistent/file.bin"));
        assert!(result.is_err());
    }

    #[test]
    fn hash_file_large_content() {
        // Test with content larger than the 64KB buffer
        let content: Vec<u8> = (0..200_000u32).map(|i| (i % 256) as u8).collect();
        let tmp = create_temp_file(&content);

        let hash = hash_file(tmp.path()).unwrap();

        // Verify against direct blake3 computation
        let expected = blake3::hash(&content).to_hex().to_string();
        assert_eq!(hash, expected);
    }
}
