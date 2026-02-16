//! Bandwidth throttling for I/O streams using a token-bucket algorithm.
//!
//! `ThrottledReader` and `ThrottledWriter` wrap `Read`/`Write` implementations
//! and limit throughput to a specified bytes-per-second rate. They use a simple
//! token-bucket approach: tokens accumulate over time up to a burst cap (2 seconds
//! worth), and each read/write consumes tokens. When tokens are exhausted, the
//! thread sleeps until enough tokens accumulate.
//!
//! `parse_bandwidth` converts human-readable strings like "10MB/s" into bytes/sec.

use std::io::{self, Read, Write};
use std::time::{Duration, Instant};

use crate::error::FluxError;

/// Parse a human-readable bandwidth string into bytes per second.
///
/// Accepts formats like "10MB/s", "500KB/s", "1GB/s", "100B/s".
/// The "/s" suffix is optional. Uses the `bytesize` crate for parsing,
/// which supports both SI (MB = 1,000,000) and IEC (MiB = 1,048,576) units.
///
/// # Examples
/// ```ignore
/// assert_eq!(parse_bandwidth("10MB/s").unwrap(), 10_000_000);
/// assert_eq!(parse_bandwidth("500KB/s").unwrap(), 500_000);
/// assert_eq!(parse_bandwidth("1GiB/s").unwrap(), 1_073_741_824);
/// ```
pub fn parse_bandwidth(s: &str) -> Result<u64, FluxError> {
    // Strip trailing "/s" (case insensitive)
    let s = s.trim();
    let size_str = if s.to_lowercase().ends_with("/s") {
        &s[..s.len() - 2]
    } else {
        s
    };

    let bytes: bytesize::ByteSize = size_str
        .parse()
        .map_err(|_| FluxError::Config(format!("Invalid bandwidth format: '{}'. Use formats like '10MB/s', '500KB/s'", s)))?;

    let bps = bytes.as_u64();
    if bps == 0 {
        return Err(FluxError::Config(
            "Bandwidth limit must be greater than 0".to_string(),
        ));
    }

    Ok(bps)
}

/// A `Read` wrapper that limits throughput using a token-bucket algorithm.
///
/// Tokens represent available bytes to read. They accumulate over time at
/// `bytes_per_sec` rate, capped at 2 seconds of burst. When tokens are
/// depleted, the reader sleeps until enough tokens are available.
pub struct ThrottledReader<R: Read> {
    inner: R,
    bytes_per_sec: u64,
    tokens: u64,
    last_refill: Instant,
}

impl<R: Read> ThrottledReader<R> {
    /// Create a new throttled reader wrapping `inner` at `bytes_per_sec`.
    ///
    /// Starts with 1 second worth of tokens for initial burst.
    pub fn new(inner: R, bytes_per_sec: u64) -> Self {
        Self {
            inner,
            bytes_per_sec,
            tokens: bytes_per_sec, // Start with 1 second of tokens
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time since last refill.
    ///
    /// Caps tokens at 2 seconds worth to limit burst size.
    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed();
        let new_tokens = (elapsed.as_secs_f64() * self.bytes_per_sec as f64) as u64;
        if new_tokens > 0 {
            self.tokens = std::cmp::min(
                self.tokens.saturating_add(new_tokens),
                self.bytes_per_sec * 2, // Max burst = 2 seconds
            );
            self.last_refill = Instant::now();
        }
    }
}

impl<R: Read> Read for ThrottledReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.refill();

        if self.tokens == 0 {
            // Sleep until we would have enough tokens for at least some data
            let sleep_bytes = std::cmp::min(buf.len() as u64, self.bytes_per_sec);
            let sleep_secs = sleep_bytes as f64 / self.bytes_per_sec as f64;
            std::thread::sleep(Duration::from_secs_f64(sleep_secs));
            self.refill();
        }

        // Limit read size to available tokens
        let max_read = std::cmp::min(buf.len(), self.tokens as usize);
        if max_read == 0 {
            // Edge case: still no tokens after sleep (shouldn't normally happen)
            return Ok(0);
        }
        let n = self.inner.read(&mut buf[..max_read])?;
        self.tokens = self.tokens.saturating_sub(n as u64);
        Ok(n)
    }
}

/// A `Write` wrapper that limits throughput using a token-bucket algorithm.
///
/// Same mechanism as `ThrottledReader` but for write operations.
pub struct ThrottledWriter<W: Write> {
    inner: W,
    bytes_per_sec: u64,
    tokens: u64,
    last_refill: Instant,
}

impl<W: Write> ThrottledWriter<W> {
    /// Create a new throttled writer wrapping `inner` at `bytes_per_sec`.
    pub fn new(inner: W, bytes_per_sec: u64) -> Self {
        Self {
            inner,
            bytes_per_sec,
            tokens: bytes_per_sec,
            last_refill: Instant::now(),
        }
    }

    /// Refill tokens based on elapsed time.
    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed();
        let new_tokens = (elapsed.as_secs_f64() * self.bytes_per_sec as f64) as u64;
        if new_tokens > 0 {
            self.tokens = std::cmp::min(
                self.tokens.saturating_add(new_tokens),
                self.bytes_per_sec * 2,
            );
            self.last_refill = Instant::now();
        }
    }
}

impl<W: Write> Write for ThrottledWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.refill();

        if self.tokens == 0 {
            let sleep_bytes = std::cmp::min(buf.len() as u64, self.bytes_per_sec);
            let sleep_secs = sleep_bytes as f64 / self.bytes_per_sec as f64;
            std::thread::sleep(Duration::from_secs_f64(sleep_secs));
            self.refill();
        }

        let max_write = std::cmp::min(buf.len(), self.tokens as usize);
        if max_write == 0 {
            return Ok(0);
        }
        let n = self.inner.write(&buf[..max_write])?;
        self.tokens = self.tokens.saturating_sub(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_bandwidth_megabytes_per_sec() {
        // bytesize treats MB as 1,000,000 (SI) and MiB as 1,048,576 (IEC)
        let bps = parse_bandwidth("10MB/s").unwrap();
        assert_eq!(bps, 10_000_000);
    }

    #[test]
    fn parse_bandwidth_kilobytes_per_sec() {
        let bps = parse_bandwidth("500KB/s").unwrap();
        assert_eq!(bps, 500_000);
    }

    #[test]
    fn parse_bandwidth_gigabytes_per_sec() {
        let bps = parse_bandwidth("1GB/s").unwrap();
        assert_eq!(bps, 1_000_000_000);
    }

    #[test]
    fn parse_bandwidth_iec_units() {
        let bps = parse_bandwidth("1MiB/s").unwrap();
        assert_eq!(bps, 1_048_576);
    }

    #[test]
    fn parse_bandwidth_without_per_sec_suffix() {
        // Should also work without "/s"
        let bps = parse_bandwidth("10MB").unwrap();
        assert_eq!(bps, 10_000_000);
    }

    #[test]
    fn parse_bandwidth_bytes() {
        let bps = parse_bandwidth("1024B/s").unwrap();
        assert_eq!(bps, 1024);
    }

    #[test]
    fn parse_bandwidth_invalid_returns_error() {
        let result = parse_bandwidth("not_a_number");
        assert!(result.is_err());
        match result {
            Err(FluxError::Config(msg)) => {
                assert!(msg.contains("Invalid bandwidth"));
            }
            other => panic!("Expected Config error, got: {:?}", other),
        }
    }

    #[test]
    fn parse_bandwidth_zero_returns_error() {
        let result = parse_bandwidth("0B/s");
        assert!(result.is_err());
        match result {
            Err(FluxError::Config(msg)) => {
                assert!(msg.contains("greater than 0"));
            }
            other => panic!("Expected Config error about zero, got: {:?}", other),
        }
    }

    #[test]
    fn throttled_reader_reads_data_correctly() {
        // Verify that throttled reader returns correct data (not testing timing)
        let data = b"Hello, throttled world! This is test data for reading.";
        let cursor = Cursor::new(data.as_ref());
        let mut reader = ThrottledReader::new(cursor, 1_000_000); // 1MB/s

        let mut output = Vec::new();
        std::io::copy(&mut reader, &mut output).unwrap();
        assert_eq!(output, data);
    }

    #[test]
    fn throttled_writer_writes_data_correctly() {
        // Verify that throttled writer produces correct output
        let data = b"Hello, throttled writer! Testing output correctness.";
        let buffer = Vec::new();
        let mut writer = ThrottledWriter::new(buffer, 1_000_000); // 1MB/s

        writer.write_all(data).unwrap();
        writer.flush().unwrap();

        let output = writer.inner;
        assert_eq!(output, data);
    }

    #[test]
    fn throttled_reader_limits_speed() {
        // Read 100KB at 50KB/s -- should take at least ~1 second
        // (We use generous bounds for CI reliability)
        let data = vec![0u8; 100_000]; // 100KB
        let cursor = Cursor::new(data);
        let mut reader = ThrottledReader::new(cursor, 50_000); // 50KB/s

        let start = Instant::now();
        let mut output = Vec::new();
        std::io::copy(&mut reader, &mut output).unwrap();
        let elapsed = start.elapsed();

        assert_eq!(output.len(), 100_000);
        // Should take at least 0.5 seconds (accounting for initial 1s burst of tokens)
        // Initial tokens = 50KB, so first 50KB is instant. Remaining 50KB takes ~1s.
        assert!(
            elapsed >= Duration::from_millis(500),
            "Expected at least 500ms, got {:?}",
            elapsed
        );
    }

    #[test]
    fn throttled_reader_empty_data() {
        let cursor = Cursor::new(Vec::<u8>::new());
        let mut reader = ThrottledReader::new(cursor, 1_000_000);

        let mut output = Vec::new();
        std::io::copy(&mut reader, &mut output).unwrap();
        assert!(output.is_empty());
    }
}
