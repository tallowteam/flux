//! Cross-platform positional I/O primitives and parallel chunk transfer engine.
//!
//! Provides `read_at` and `write_at` functions that use OS-specific APIs
//! (Unix `pread`/`pwrite`, Windows `seek_read`/`seek_write`) to read/write
//! at specific file offsets without moving the shared file cursor.
//!
//! Also provides `read_at_exact` and `write_at_all` wrappers that handle
//! partial reads/writes, analogous to `Read::read_exact` and `Write::write_all`.
//!
//! The `parallel_copy_chunked` function uses rayon to copy file chunks in
//! parallel, computing per-chunk BLAKE3 checksums during transfer.

use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;
use std::sync::Arc;

use indicatif::ProgressBar;
use rayon::prelude::*;

use crate::error::FluxError;
use crate::transfer::chunk::ChunkPlan;

/// Read bytes from `file` at the given byte `offset` into `buf`.
///
/// Returns the number of bytes actually read (may be less than `buf.len()`).
/// Does not move the file cursor (on Unix). On Windows, the cursor is updated
/// as a side effect but each call specifies its own offset, so concurrent
/// positional reads from different threads are safe.
pub fn read_at(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(buf, offset)
    }
}

/// Write bytes from `buf` to `file` at the given byte `offset`.
///
/// Returns the number of bytes actually written (may be less than `buf.len()`).
/// Does not move the file cursor (on Unix). On Windows, the cursor is updated
/// as a side effect but each call specifies its own offset.
pub fn write_at(file: &File, offset: u64, buf: &[u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_write(buf, offset)
    }
}

/// Read exactly `buf.len()` bytes from `file` starting at `offset`.
///
/// Loops calling `read_at` until the buffer is completely filled or EOF
/// is reached. Returns `UnexpectedEof` if EOF is hit before filling `buf`.
///
/// Analogous to `Read::read_exact` but for positional reads.
pub fn read_at_exact(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    let mut bytes_read = 0usize;

    while bytes_read < buf.len() {
        let current_offset = offset + bytes_read as u64;
        match read_at(file, current_offset, &mut buf[bytes_read..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    format!(
                        "read_at_exact: EOF after {} of {} bytes at offset {}",
                        bytes_read,
                        buf.len(),
                        offset
                    ),
                ));
            }
            Ok(n) => {
                bytes_read += n;
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Write all bytes from `buf` to `file` starting at `offset`.
///
/// Loops calling `write_at` until all bytes are written. Retries on
/// `Interrupted` errors.
///
/// Analogous to `Write::write_all` but for positional writes.
pub fn write_at_all(file: &File, offset: u64, buf: &[u8]) -> io::Result<()> {
    let mut bytes_written = 0usize;

    while bytes_written < buf.len() {
        let current_offset = offset + bytes_written as u64;
        match write_at(file, current_offset, &buf[bytes_written..]) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::WriteZero,
                    "write_at_all: write returned 0 bytes",
                ));
            }
            Ok(n) => {
                bytes_written += n;
            }
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {
                continue;
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

/// Buffer size for per-chunk I/O during parallel copy: 256KB.
const CHUNK_BUF_SIZE: usize = 256 * 1024;

/// Copy a file using parallel chunked I/O with per-chunk BLAKE3 checksums.
///
/// Opens the source file for reading and creates/pre-allocates the destination
/// file to the total size. Each chunk is processed in parallel using rayon's
/// parallel iterator: read from source at the chunk's offset, write to dest
/// at the same offset, and compute a BLAKE3 hash of the chunk data.
///
/// After each buffer write, the progress bar is incremented by the number of
/// bytes written.
///
/// # Arguments
/// * `source` - Path to the source file
/// * `dest` - Path to the destination file (will be created/truncated)
/// * `chunks` - Mutable slice of ChunkPlans describing byte ranges to copy
/// * `progress` - Progress bar to update with bytes transferred
///
/// # Errors
/// Returns `FluxError` if any I/O operation fails. If a chunk fails, the
/// entire operation is aborted (rayon's `try_for_each` short-circuits).
pub fn parallel_copy_chunked(
    source: &Path,
    dest: &Path,
    chunks: &mut [ChunkPlan],
    progress: &ProgressBar,
) -> Result<(), FluxError> {
    // Open source file (read-only), wrap in Arc for sharing across threads
    let src_file = File::open(source).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: source.to_path_buf(),
        },
        io::ErrorKind::PermissionDenied => FluxError::PermissionDenied {
            path: source.to_path_buf(),
        },
        _ => FluxError::Io { source: e },
    })?;
    let src_file = Arc::new(src_file);

    // Compute total size from chunks for pre-allocation
    let total_size: u64 = chunks.iter().map(|c| c.offset + c.length).max().unwrap_or(0);

    // Ensure dest parent directory exists
    if let Some(parent) = dest.parent() {
        if !parent.as_os_str().is_empty() && !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| match e.kind() {
                io::ErrorKind::PermissionDenied => FluxError::DestinationNotWritable {
                    path: parent.to_path_buf(),
                },
                _ => FluxError::Io { source: e },
            })?;
        }
    }

    // Create dest file with read+write permissions, pre-allocate to full size
    let dst_file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(dest)
        .map_err(|e| match e.kind() {
            io::ErrorKind::PermissionDenied => FluxError::DestinationNotWritable {
                path: dest.to_path_buf(),
            },
            _ => FluxError::Io { source: e },
        })?;

    // Pre-allocate destination file to full size
    dst_file
        .set_len(total_size)
        .map_err(|e| FluxError::Io { source: e })?;

    let dst_file = Arc::new(dst_file);

    // Process chunks in parallel using rayon
    chunks
        .par_iter_mut()
        .filter(|chunk| !chunk.completed)
        .try_for_each(|chunk| -> Result<(), FluxError> {
            let mut buf = vec![0u8; CHUNK_BUF_SIZE];
            let mut remaining = chunk.length;
            let mut chunk_offset = chunk.offset;
            let mut hasher = blake3::Hasher::new();

            while remaining > 0 {
                let to_read = std::cmp::min(remaining, CHUNK_BUF_SIZE as u64) as usize;
                let n = read_at(&src_file, chunk_offset, &mut buf[..to_read])?;
                if n == 0 {
                    break;
                }

                write_at_all(&dst_file, chunk_offset, &buf[..n])?;
                hasher.update(&buf[..n]);
                progress.inc(n as u64);

                chunk_offset += n as u64;
                remaining -= n as u64;
            }

            chunk.checksum = Some(hasher.finalize().to_hex().to_string());
            chunk.completed = true;
            Ok(())
        })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Helper: create a temp file with known content and return the file handle.
    fn create_temp_file(content: &[u8]) -> (tempfile::NamedTempFile, u64) {
        let mut tmp = tempfile::NamedTempFile::new().expect("create temp file");
        tmp.write_all(content).expect("write content");
        tmp.flush().expect("flush");
        let len = content.len() as u64;
        (tmp, len)
    }

    #[test]
    fn read_at_beginning() {
        let data = b"Hello, World! This is a test of positional I/O.";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 5];
        let n = read_at(&file, 0, &mut buf).unwrap();
        assert_eq!(n, 5);
        assert_eq!(&buf, b"Hello");
    }

    #[test]
    fn read_at_middle() {
        let data = b"Hello, World! This is a test of positional I/O.";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 6];
        let n = read_at(&file, 7, &mut buf).unwrap();
        assert_eq!(n, 6);
        assert_eq!(&buf, b"World!");
    }

    #[test]
    fn read_at_end() {
        let data = b"Hello, World!";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 10];
        // Read past the end
        let n = read_at(&file, 10, &mut buf).unwrap();
        assert_eq!(n, 3); // Only 3 bytes left
        assert_eq!(&buf[..3], b"ld!");
    }

    #[test]
    fn read_at_past_eof() {
        let data = b"Hello";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 10];
        let n = read_at(&file, 100, &mut buf).unwrap();
        assert_eq!(n, 0);
    }

    #[test]
    fn write_at_beginning() {
        let data = b"Hello, World!";
        let (tmp, _) = create_temp_file(data);

        // Reopen for writing
        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .unwrap();

        let n = write_at(&file, 0, b"XXXXX").unwrap();
        assert_eq!(n, 5);

        // Verify by reading back
        let result = std::fs::read(tmp.path()).unwrap();
        assert_eq!(&result, b"XXXXX, World!");
    }

    #[test]
    fn write_at_middle() {
        let data = b"Hello, World!";
        let (tmp, _) = create_temp_file(data);

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .unwrap();

        let n = write_at(&file, 7, b"Flux!!").unwrap();
        assert_eq!(n, 6);

        let result = std::fs::read(tmp.path()).unwrap();
        assert_eq!(&result, b"Hello, Flux!!");
    }

    #[test]
    fn write_at_extends_file() {
        let (tmp, _) = create_temp_file(b"AB");

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .unwrap();

        // Write beyond current file size
        let n = write_at(&file, 5, b"XY").unwrap();
        assert_eq!(n, 2);

        let result = std::fs::read(tmp.path()).unwrap();
        // Bytes 2-4 are zero-filled (sparse/padding)
        assert_eq!(result.len(), 7);
        assert_eq!(result[0], b'A');
        assert_eq!(result[1], b'B');
        assert_eq!(result[5], b'X');
        assert_eq!(result[6], b'Y');
    }

    #[test]
    fn read_at_exact_reads_full_buffer() {
        let data = b"abcdefghijklmnopqrstuvwxyz";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 10];
        read_at_exact(&file, 5, &mut buf).unwrap();
        assert_eq!(&buf, b"fghijklmno");
    }

    #[test]
    fn read_at_exact_eof_returns_error() {
        let data = b"short";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 20]; // Larger than file
        let result = read_at_exact(&file, 0, &mut buf);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_at_exact_at_offset_eof() {
        let data = b"Hello, World!";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf = [0u8; 10];
        // Offset 10 + 10 bytes = 20, but file is only 13 bytes
        let result = read_at_exact(&file, 10, &mut buf);
        assert!(result.is_err());
    }

    #[test]
    fn write_at_all_writes_complete_buffer() {
        let data = b"0000000000"; // 10 zeros
        let (tmp, _) = create_temp_file(data);

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .unwrap();

        write_at_all(&file, 3, b"ABCDE").unwrap();

        let result = std::fs::read(tmp.path()).unwrap();
        assert_eq!(&result, b"000ABCDE00");
    }

    #[test]
    fn write_at_all_at_offset_zero() {
        let data = b"XXXXXXXXXX";
        let (tmp, _) = create_temp_file(data);

        let file = std::fs::OpenOptions::new()
            .write(true)
            .open(tmp.path())
            .unwrap();

        write_at_all(&file, 0, b"Hello").unwrap();

        let result = std::fs::read(tmp.path()).unwrap();
        assert_eq!(&result, b"HelloXXXXX");
    }

    #[test]
    fn read_at_and_write_at_roundtrip() {
        // Write known patterns at specific offsets, then read them back
        let initial = vec![0u8; 100];
        let (tmp, _) = create_temp_file(&initial);

        let file = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(tmp.path())
            .unwrap();

        // Write patterns at different offsets
        write_at_all(&file, 0, b"AAAA").unwrap();
        write_at_all(&file, 25, b"BBBB").unwrap();
        write_at_all(&file, 50, b"CCCC").unwrap();
        write_at_all(&file, 75, b"DDDD").unwrap();

        // Read back and verify each pattern
        let mut buf = [0u8; 4];

        read_at_exact(&file, 0, &mut buf).unwrap();
        assert_eq!(&buf, b"AAAA");

        read_at_exact(&file, 25, &mut buf).unwrap();
        assert_eq!(&buf, b"BBBB");

        read_at_exact(&file, 50, &mut buf).unwrap();
        assert_eq!(&buf, b"CCCC");

        read_at_exact(&file, 75, &mut buf).unwrap();
        assert_eq!(&buf, b"DDDD");

        // Verify zeros between patterns
        let mut between = [0u8; 1];
        read_at_exact(&file, 4, &mut between).unwrap();
        assert_eq!(between[0], 0);
        read_at_exact(&file, 29, &mut between).unwrap();
        assert_eq!(between[0], 0);
    }

    #[test]
    fn multiple_reads_at_different_offsets_dont_interfere() {
        // Verify that reading at one offset doesn't affect reading at another
        let data = b"0123456789ABCDEFGHIJ";
        let (tmp, _) = create_temp_file(data);
        let file = File::open(tmp.path()).expect("open file");

        let mut buf1 = [0u8; 5];
        let mut buf2 = [0u8; 5];
        let mut buf3 = [0u8; 5];

        // Read in non-sequential order
        read_at_exact(&file, 10, &mut buf2).unwrap();
        read_at_exact(&file, 0, &mut buf1).unwrap();
        read_at_exact(&file, 15, &mut buf3).unwrap();

        assert_eq!(&buf1, b"01234");
        assert_eq!(&buf2, b"ABCDE");
        assert_eq!(&buf3, b"FGHIJ");
    }

    // ========================================================================
    // parallel_copy_chunked tests
    // ========================================================================

    #[test]
    fn parallel_copy_chunked_copies_file_correctly() {
        use crate::transfer::chunk::chunk_file;

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");

        // Create 1MB file with known pattern
        let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src_path, &data).unwrap();

        let mut chunks = chunk_file(data.len() as u64, 4);
        let pb = ProgressBar::hidden();

        parallel_copy_chunked(&src_path, &dst_path, &mut chunks, &pb).unwrap();

        // Verify dest content matches source byte-for-byte
        let dest_data = std::fs::read(&dst_path).unwrap();
        assert_eq!(dest_data.len(), data.len());
        assert_eq!(dest_data, data);
    }

    #[test]
    fn parallel_copy_chunked_populates_checksums() {
        use crate::transfer::chunk::chunk_file;

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");

        let data: Vec<u8> = (0..1_048_576u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src_path, &data).unwrap();

        let mut chunks = chunk_file(data.len() as u64, 4);
        let pb = ProgressBar::hidden();

        parallel_copy_chunked(&src_path, &dst_path, &mut chunks, &pb).unwrap();

        // All chunks should be completed with checksums
        for chunk in &chunks {
            assert!(chunk.completed, "chunk {} should be completed", chunk.index);
            assert!(
                chunk.checksum.is_some(),
                "chunk {} should have checksum",
                chunk.index
            );
            let checksum = chunk.checksum.as_ref().unwrap();
            assert_eq!(checksum.len(), 64, "checksum should be 64 hex chars");
            assert!(
                checksum.chars().all(|c| c.is_ascii_hexdigit()),
                "checksum should be hex"
            );
        }
    }

    #[test]
    fn parallel_copy_chunked_single_chunk() {
        use crate::transfer::chunk::chunk_file;

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");

        let data: Vec<u8> = (0..500_000u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src_path, &data).unwrap();

        let mut chunks = chunk_file(data.len() as u64, 1);
        let pb = ProgressBar::hidden();

        parallel_copy_chunked(&src_path, &dst_path, &mut chunks, &pb).unwrap();

        let dest_data = std::fs::read(&dst_path).unwrap();
        assert_eq!(dest_data, data);
        assert!(chunks[0].completed);
        assert!(chunks[0].checksum.is_some());
    }

    #[test]
    fn parallel_copy_chunked_progress_tracks_bytes() {
        use crate::transfer::chunk::chunk_file;

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");

        let size = 100_000u64;
        let data: Vec<u8> = (0..size as u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src_path, &data).unwrap();

        let mut chunks = chunk_file(size, 4);
        let pb = ProgressBar::hidden();

        parallel_copy_chunked(&src_path, &dst_path, &mut chunks, &pb).unwrap();

        // Progress bar should have tracked all bytes
        assert_eq!(pb.position(), size);
    }

    #[test]
    fn parallel_copy_chunked_skips_completed_chunks() {
        use crate::transfer::chunk::chunk_file;

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.bin");
        let dst_path = dir.path().join("dest.bin");

        let data: Vec<u8> = (0..1000u32).map(|i| (i % 256) as u8).collect();
        std::fs::write(&src_path, &data).unwrap();

        let mut chunks = chunk_file(data.len() as u64, 2);
        // Mark first chunk as already completed
        chunks[0].completed = true;
        chunks[0].checksum = Some("already_done".to_string());

        let pb = ProgressBar::hidden();

        parallel_copy_chunked(&src_path, &dst_path, &mut chunks, &pb).unwrap();

        // First chunk should retain its original checksum (was not re-processed)
        assert_eq!(chunks[0].checksum.as_deref(), Some("already_done"));
        // Second chunk should have been processed
        assert!(chunks[1].completed);
        assert!(chunks[1].checksum.is_some());
        assert_ne!(chunks[1].checksum.as_deref(), Some("already_done"));
    }
}
