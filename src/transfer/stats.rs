//! Transfer statistics tracking and completion summaries.
//!
//! Provides `TransferStats` for collecting metrics during transfers and
//! printing consistent completion summaries across all transfer commands
//! (cp, send, receive, sync, verify).

use std::time::{Duration, Instant};

use bytesize::ByteSize;

/// Aggregated transfer statistics for any operation.
///
/// Tracks file counts, byte totals, and wall-clock time to produce
/// human-readable completion summaries with throughput.
pub struct TransferStats {
    pub files_total: u64,
    pub files_done: u64,
    pub files_failed: u64,
    pub files_skipped: u64,
    pub bytes_total: u64,
    pub bytes_done: u64,
    pub started: Instant,
}

impl TransferStats {
    /// Create stats for a transfer with known totals.
    pub fn new(files_total: u64, bytes_total: u64) -> Self {
        Self {
            files_total,
            files_done: 0,
            files_failed: 0,
            files_skipped: 0,
            bytes_total,
            bytes_done: 0,
            started: Instant::now(),
        }
    }

    /// Record a successfully transferred file.
    pub fn add_done(&mut self, bytes: u64) {
        self.files_done += 1;
        self.bytes_done += bytes;
    }

    /// Record a failed file.
    pub fn add_failed(&mut self) {
        self.files_failed += 1;
    }

    /// Record a skipped file.
    pub fn add_skipped(&mut self) {
        self.files_skipped += 1;
    }

    /// Wall-clock elapsed time since start.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Average throughput in bytes per second.
    pub fn throughput_bps(&self) -> u64 {
        let secs = self.elapsed().as_secs_f64();
        if secs > 0.0 {
            (self.bytes_done as f64 / secs) as u64
        } else {
            0
        }
    }

    /// Print a human-readable completion summary to stderr.
    ///
    /// For single files:
    /// ```text
    /// Completed: photo.jpg (2.4 MB) in 1.2s @ 2.0 MB/s
    /// ```
    ///
    /// For multiple files:
    /// ```text
    /// Completed: 42 files (1.2 GB) in 8.3s @ 148.2 MB/s | 0 failed, 3 skipped
    /// ```
    pub fn print_summary(&self, quiet: bool) {
        if quiet {
            return;
        }
        let elapsed = self.elapsed();
        let secs = elapsed.as_secs_f64();
        let throughput = ByteSize(self.throughput_bps());

        if self.files_total <= 1 && self.files_failed == 0 {
            // Single-file summary
            eprintln!(
                "Completed: {} in {:.1}s @ {}/s",
                ByteSize(self.bytes_done),
                secs,
                throughput,
            );
        } else {
            // Multi-file summary
            eprintln!(
                "Completed: {} files ({}) in {:.1}s @ {}/s | {} failed, {} skipped",
                self.files_done,
                ByteSize(self.bytes_done),
                secs,
                throughput,
                self.files_failed,
                self.files_skipped,
            );
        }
    }

    /// Print a single-file completion summary with the filename.
    pub fn print_file_summary(&self, filename: &str, quiet: bool) {
        if quiet {
            return;
        }
        let elapsed = self.elapsed();
        let secs = elapsed.as_secs_f64();
        let throughput = ByteSize(self.throughput_bps());

        eprintln!(
            "Completed: {} ({}) in {:.1}s @ {}/s",
            filename,
            ByteSize(self.bytes_done),
            secs,
            throughput,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_stats_start_at_zero() {
        let stats = TransferStats::new(10, 1000);
        assert_eq!(stats.files_total, 10);
        assert_eq!(stats.bytes_total, 1000);
        assert_eq!(stats.files_done, 0);
        assert_eq!(stats.bytes_done, 0);
        assert_eq!(stats.files_failed, 0);
        assert_eq!(stats.files_skipped, 0);
    }

    #[test]
    fn add_done_increments() {
        let mut stats = TransferStats::new(5, 500);
        stats.add_done(100);
        stats.add_done(200);
        assert_eq!(stats.files_done, 2);
        assert_eq!(stats.bytes_done, 300);
    }

    #[test]
    fn add_failed_increments() {
        let mut stats = TransferStats::new(5, 500);
        stats.add_failed();
        stats.add_failed();
        assert_eq!(stats.files_failed, 2);
    }

    #[test]
    fn add_skipped_increments() {
        let mut stats = TransferStats::new(5, 500);
        stats.add_skipped();
        assert_eq!(stats.files_skipped, 1);
    }

    #[test]
    fn throughput_zero_on_no_bytes() {
        let stats = TransferStats::new(0, 0);
        // throughput_bps may be 0 since bytes_done is 0
        assert_eq!(stats.throughput_bps(), 0);
    }

    #[test]
    fn quiet_suppresses_output() {
        let stats = TransferStats::new(1, 100);
        // Should not panic when quiet=true
        stats.print_summary(true);
        stats.print_file_summary("test.txt", true);
    }
}
