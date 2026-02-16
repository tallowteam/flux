//! Chunk planning module for splitting files into parallel transfer units.
//!
//! Provides `ChunkPlan` and `TransferPlan` types for describing how a file
//! should be split into chunks, plus heuristics for auto-detecting the optimal
//! chunk count based on file size.

use serde::{Deserialize, Serialize};

/// Describes one chunk of a file to transfer.
///
/// Each chunk represents a contiguous byte range [offset, offset+length).
/// Chunks are independent units that can be read/written in parallel.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChunkPlan {
    /// Zero-based index of this chunk within the transfer plan.
    pub index: usize,
    /// Byte offset from the start of the file.
    pub offset: u64,
    /// Number of bytes in this chunk.
    pub length: u64,
    /// Whether this chunk has been successfully transferred.
    pub completed: bool,
    /// BLAKE3 hex hash of this chunk's data (populated after transfer).
    pub checksum: Option<String>,
}

/// Plan for transferring an entire file in chunks.
///
/// Contains metadata about the transfer plus a vector of `ChunkPlan`s
/// describing how the file is split. Serializable for resume manifests.
#[derive(Debug, Serialize, Deserialize)]
pub struct TransferPlan {
    /// Source file path.
    pub source_path: String,
    /// Destination file path.
    pub dest_path: String,
    /// Total file size in bytes.
    pub total_size: u64,
    /// Whole-file BLAKE3 hex hash (populated after transfer if --verify).
    pub file_checksum: Option<String>,
    /// The chunks that make up this transfer.
    pub chunks: Vec<ChunkPlan>,
}

/// Split a file of `total_size` bytes into `chunk_count` chunks.
///
/// Divides evenly, with the last chunk absorbing any remainder.
/// All chunks start as incomplete with no checksum.
///
/// # Edge cases
/// - `chunk_count == 0`: returns an empty Vec
/// - `total_size == 0`: returns `chunk_count` chunks of length 0
pub fn chunk_file(total_size: u64, chunk_count: usize) -> Vec<ChunkPlan> {
    if chunk_count == 0 {
        return Vec::new();
    }

    let chunk_size = total_size / chunk_count as u64;
    let remainder = total_size % chunk_count as u64;
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut offset = 0u64;

    for i in 0..chunk_count {
        // Last chunk absorbs the remainder
        let length = if i == chunk_count - 1 {
            chunk_size + remainder
        } else {
            chunk_size
        };
        chunks.push(ChunkPlan {
            index: i,
            offset,
            length,
            completed: false,
            checksum: None,
        });
        offset += length;
    }

    chunks
}

/// Determine the optimal chunk count for a file based on its size.
///
/// Heuristic tiers:
/// - < 10 MB: 1 chunk (no parallelism -- overhead not worth it)
/// - 10 MB - 100 MB: 2 chunks
/// - 100 MB - 1 GB: 4 chunks
/// - 1 GB - 10 GB: 8 chunks
/// - > 10 GB: 16 chunks
///
/// The result is capped at the available CPU parallelism to avoid
/// over-subscribing the system.
pub fn auto_chunk_count(file_size: u64) -> usize {
    let base_count = match file_size {
        0..=10_485_759 => 1,                // < 10 MB
        10_485_760..=104_857_599 => 2,      // 10 MB - 100 MB
        104_857_600..=1_073_741_823 => 4,   // 100 MB - 1 GB
        1_073_741_824..=10_737_418_239 => 8, // 1 GB - 10 GB
        _ => 16,                            // > 10 GB
    };

    // Don't exceed available CPU parallelism
    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    std::cmp::min(base_count, max_threads)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunk_file_even_split() {
        let chunks = chunk_file(100, 4);
        assert_eq!(chunks.len(), 4);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
            assert_eq!(chunk.length, 25);
            assert_eq!(chunk.offset, (i as u64) * 25);
            assert!(!chunk.completed);
            assert!(chunk.checksum.is_none());
        }
    }

    #[test]
    fn chunk_file_with_remainder() {
        let chunks = chunk_file(101, 4);
        assert_eq!(chunks.len(), 4);
        // First 3 chunks: 25 bytes each
        assert_eq!(chunks[0].length, 25);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[1].length, 25);
        assert_eq!(chunks[1].offset, 25);
        assert_eq!(chunks[2].length, 25);
        assert_eq!(chunks[2].offset, 50);
        // Last chunk absorbs remainder: 25 + 1 = 26
        assert_eq!(chunks[3].length, 26);
        assert_eq!(chunks[3].offset, 75);
        // Verify total coverage
        let total: u64 = chunks.iter().map(|c| c.length).sum();
        assert_eq!(total, 101);
    }

    #[test]
    fn chunk_file_zero_size() {
        let chunks = chunk_file(0, 4);
        assert_eq!(chunks.len(), 4);
        for chunk in &chunks {
            assert_eq!(chunk.length, 0);
        }
    }

    #[test]
    fn chunk_file_zero_count() {
        let chunks = chunk_file(100, 0);
        assert!(chunks.is_empty());
    }

    #[test]
    fn chunk_file_single_chunk() {
        let chunks = chunk_file(500, 1);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].index, 0);
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[0].length, 500);
    }

    #[test]
    fn chunk_file_offsets_contiguous() {
        // Verify that chunks perfectly tile the file with no gaps or overlaps
        let sizes = [100, 101, 1000, 1023, 1024, 999_999];
        let counts = [1, 2, 3, 4, 7, 16];

        for &size in &sizes {
            for &count in &counts {
                let chunks = chunk_file(size, count);
                let total: u64 = chunks.iter().map(|c| c.length).sum();
                assert_eq!(total, size, "size={}, count={}", size, count);

                // Verify contiguous offsets
                let mut expected_offset = 0u64;
                for chunk in &chunks {
                    assert_eq!(
                        chunk.offset, expected_offset,
                        "size={}, count={}, index={}",
                        size, count, chunk.index
                    );
                    expected_offset += chunk.length;
                }
            }
        }
    }

    #[test]
    fn auto_chunk_count_small_file() {
        // < 10 MB should return 1
        assert_eq!(auto_chunk_count(0), 1);
        assert_eq!(auto_chunk_count(1), 1);
        assert_eq!(auto_chunk_count(1_000_000), 1); // 1 MB
        assert_eq!(auto_chunk_count(10_485_759), 1); // Just under 10 MB
    }

    #[test]
    fn auto_chunk_count_medium_file() {
        // 10 MB - 100 MB should return 2 (capped at available_parallelism)
        let count = auto_chunk_count(50_000_000); // 50 MB
        assert!(count >= 1 && count <= 2);
        let count = auto_chunk_count(10_485_760); // Exactly 10 MB
        assert!(count >= 1 && count <= 2);
    }

    #[test]
    fn auto_chunk_count_large_file() {
        // 100 MB - 1 GB should return up to 4
        let count = auto_chunk_count(500_000_000); // 500 MB
        assert!(count >= 1 && count <= 4);
    }

    #[test]
    fn auto_chunk_count_very_large_file() {
        // 1 GB - 10 GB should return up to 8
        let count = auto_chunk_count(5_000_000_000); // 5 GB
        assert!(count >= 1 && count <= 8);
    }

    #[test]
    fn auto_chunk_count_huge_file() {
        // > 10 GB should return up to 16
        let count = auto_chunk_count(20_000_000_000); // 20 GB
        assert!(count >= 1 && count <= 16);
    }

    #[test]
    fn auto_chunk_count_capped_at_parallelism() {
        // The result should never exceed available_parallelism
        let max_threads = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let count = auto_chunk_count(100_000_000_000); // 100 GB
        assert!(count <= max_threads);
    }

    #[test]
    fn chunk_plan_serialization() {
        let chunk = ChunkPlan {
            index: 0,
            offset: 0,
            length: 1024,
            completed: true,
            checksum: Some("abc123".to_string()),
        };
        let json = serde_json::to_string(&chunk).unwrap();
        let deserialized: ChunkPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.index, 0);
        assert_eq!(deserialized.offset, 0);
        assert_eq!(deserialized.length, 1024);
        assert!(deserialized.completed);
        assert_eq!(deserialized.checksum, Some("abc123".to_string()));
    }

    #[test]
    fn transfer_plan_serialization() {
        let plan = TransferPlan {
            source_path: "/tmp/source.bin".to_string(),
            dest_path: "/tmp/dest.bin".to_string(),
            total_size: 1000,
            file_checksum: None,
            chunks: chunk_file(1000, 2),
        };
        let json = serde_json::to_string_pretty(&plan).unwrap();
        let deserialized: TransferPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.source_path, "/tmp/source.bin");
        assert_eq!(deserialized.dest_path, "/tmp/dest.bin");
        assert_eq!(deserialized.total_size, 1000);
        assert!(deserialized.file_checksum.is_none());
        assert_eq!(deserialized.chunks.len(), 2);
        assert_eq!(deserialized.chunks[0].length, 500);
        assert_eq!(deserialized.chunks[1].length, 500);
    }
}
