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
             {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})",
        )
        .unwrap()
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
        .unwrap()
        .progress_chars("=>-"),
    );
    pb
}
