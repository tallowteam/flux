use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

/// Create a progress bar for tracking bytes during a single file copy.
///
/// Renders to stderr (not stdout) so piped output stays clean.
/// Returns a hidden bar if quiet mode is active.
pub fn create_file_progress(total_bytes: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total_bytes);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
             {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta}) {msg}",
        )
        .expect("static progress template is valid")
        .progress_chars("=>-"),
    );
    pb
}

/// Create a progress bar for tracking files during a directory copy.
///
/// Renders to stderr. Returns a hidden bar if quiet mode is active.
/// Defined now for Plan 03 (directory copy) to avoid touching this file later.
pub fn create_directory_progress(total_files: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total_files);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
             {pos}/{len} files ({per_sec}, ETA {eta}) {msg}",
        )
        .expect("static progress template is valid")
        .progress_chars("=>-"),
    );
    pb
}

/// Create a progress bar tracking bytes for directory transfers.
///
/// Tracks bytes (for accurate speed/ETA) while callers use `set_message()`
/// to show file count as a prefix. Used by directory copy and sync operations.
///
/// Renders to stderr. Returns a hidden bar if quiet mode is active.
pub fn create_transfer_progress(total_bytes: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total_bytes);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
             {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta}) {msg}",
        )
        .expect("static progress template is valid")
        .progress_chars("=>-"),
    );
    pb
}
