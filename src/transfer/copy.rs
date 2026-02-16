use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::Path;

use indicatif::ProgressBar;

use crate::error::FluxError;

/// Buffer size for BufReader/BufWriter: 256KB.
const BUF_SIZE: usize = 256 * 1024;

/// Wraps a Read and updates a ProgressBar as bytes are read.
pub struct ProgressReader<R: Read> {
    inner: R,
    progress: ProgressBar,
}

impl<R: Read> ProgressReader<R> {
    pub fn new(inner: R, progress: ProgressBar) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.inner.read(buf)?;
        self.progress.inc(bytes_read as u64);
        Ok(bytes_read)
    }
}

/// Copy a single file with progress reporting.
///
/// Opens source and dest directly with std::fs, wraps in BufReader/BufWriter
/// with 256KB buffers, and tracks bytes through ProgressReader.
///
/// Ensures parent directory of dest exists before writing.
pub fn copy_file_with_progress(
    source: &Path,
    dest: &Path,
    progress: &ProgressBar,
) -> Result<u64, FluxError> {
    // Open source file
    let src_file = std::fs::File::open(source).map_err(|e| match e.kind() {
        io::ErrorKind::NotFound => FluxError::SourceNotFound {
            path: source.to_path_buf(),
        },
        io::ErrorKind::PermissionDenied => FluxError::PermissionDenied {
            path: source.to_path_buf(),
        },
        _ => FluxError::Io { source: e },
    })?;

    // Get source size and set progress bar length
    let src_size = src_file
        .metadata()
        .map_err(|e| FluxError::Io { source: e })?
        .len();
    progress.set_length(src_size);

    // Wrap in buffered reader, then progress-tracking reader
    let reader = BufReader::with_capacity(BUF_SIZE, src_file);
    let mut reader = ProgressReader::new(reader, progress.clone());

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

    // Create dest file with buffered writer
    let dest_file = std::fs::File::create(dest).map_err(|e| match e.kind() {
        io::ErrorKind::PermissionDenied => FluxError::DestinationNotWritable {
            path: dest.to_path_buf(),
        },
        _ => FluxError::Io { source: e },
    })?;
    let mut writer = BufWriter::with_capacity(BUF_SIZE, dest_file);

    // Perform the copy
    let bytes_copied = io::copy(&mut reader, &mut writer)?;

    // Flush remaining buffered data
    writer.flush()?;

    // Mark progress complete
    progress.finish_with_message("done");

    Ok(bytes_copied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indicatif::ProgressBar;
    use std::io::Cursor;

    #[test]
    fn progress_reader_tracks_bytes() {
        let data = b"hello world, this is a test of the progress reader";
        let cursor = Cursor::new(data.as_ref());
        let pb = ProgressBar::hidden();
        let mut reader = ProgressReader::new(cursor, pb.clone());

        let mut buf = [0u8; 10];
        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(pb.position(), 10);

        let n = reader.read(&mut buf).unwrap();
        assert_eq!(n, 10);
        assert_eq!(pb.position(), 20);
    }

    #[test]
    fn copy_file_with_progress_copies_content() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.txt");
        let dst_path = dir.path().join("dest.txt");

        let content = "Hello, Flux! This is a test file for copy.";
        std::fs::write(&src_path, content).unwrap();

        let pb = ProgressBar::hidden();
        let bytes = copy_file_with_progress(&src_path, &dst_path, &pb).unwrap();

        assert_eq!(bytes, content.len() as u64);
        assert_eq!(std::fs::read_to_string(&dst_path).unwrap(), content);
    }

    #[test]
    fn copy_file_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("source.txt");
        let dst_path = dir.path().join("nested").join("deep").join("dest.txt");

        std::fs::write(&src_path, "nested test").unwrap();

        let pb = ProgressBar::hidden();
        let bytes = copy_file_with_progress(&src_path, &dst_path, &pb).unwrap();

        assert_eq!(bytes, 11);
        assert_eq!(std::fs::read_to_string(&dst_path).unwrap(), "nested test");
    }

    #[test]
    fn copy_nonexistent_source_returns_error() {
        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("nonexistent.txt");
        let dst_path = dir.path().join("dest.txt");

        let pb = ProgressBar::hidden();
        let result = copy_file_with_progress(&src_path, &dst_path, &pb);

        assert!(result.is_err());
        match result {
            Err(FluxError::SourceNotFound { .. }) => {} // expected
            Err(other) => panic!("Expected SourceNotFound, got: {:?}", other),
            Ok(_) => panic!("Expected error, got Ok"),
        }
    }
}
