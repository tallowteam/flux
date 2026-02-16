//! Cross-platform positional I/O primitives for parallel chunk transfers.
//!
//! Provides `read_at` and `write_at` functions that use OS-specific APIs
//! (Unix `pread`/`pwrite`, Windows `seek_read`/`seek_write`) to read/write
//! at specific file offsets without moving the shared file cursor.
//!
//! Also provides `read_at_exact` and `write_at_all` wrappers that handle
//! partial reads/writes, analogous to `Read::read_exact` and `Write::write_all`.

use std::fs::File;
use std::io;

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
}
