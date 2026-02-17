pub mod engine;
pub mod plan;
pub mod schedule;
pub mod watch;

use std::path::Path;

use bytesize::ByteSize;

use crate::cli::args::SyncArgs;
use crate::error::FluxError;
use crate::transfer::filter::TransferFilter;
use crate::transfer::stats::TransferStats;

use self::engine::{compute_sync_plan, execute_sync_plan};

/// Entry point for the `flux sync` command.
///
/// Validates inputs, builds filter, computes sync plan, and either
/// prints it (dry-run) or executes it. Dispatches to watch mode or
/// schedule mode if the corresponding flags are set.
pub fn execute_sync(args: SyncArgs, quiet: bool) -> Result<(), FluxError> {
    let source = Path::new(&args.source);
    let dest = Path::new(&args.dest);

    // Validate source exists and is a directory
    if !source.exists() {
        return Err(FluxError::SourceNotFound {
            path: source.to_path_buf(),
        });
    }
    if !source.is_dir() {
        return Err(FluxError::SyncError(format!(
            "Source '{}' is not a directory. Use 'flux cp' for single files.",
            source.display()
        )));
    }

    // Create dest directory if it doesn't exist
    if !dest.exists() {
        std::fs::create_dir_all(dest)?;
    }

    // Validate --watch and --schedule are mutually exclusive
    if args.watch && args.schedule.is_some() {
        return Err(FluxError::SyncError(
            "--watch and --schedule are mutually exclusive. Use one or the other.".to_string(),
        ));
    }

    // Build filter from --exclude/--include patterns
    let filter = TransferFilter::new(&args.exclude, &args.include)?;

    // Dispatch to watch mode
    if args.watch {
        return watch::watch_and_sync(
            source,
            dest,
            &filter,
            args.delete,
            quiet,
            args.verify,
            args.force,
        );
    }

    // Dispatch to schedule mode
    if let Some(ref cron_expr) = args.schedule {
        return schedule::scheduled_sync(
            cron_expr,
            source,
            dest,
            &filter,
            args.delete,
            quiet,
            args.verify,
            args.force,
        );
    }

    // Compute the sync plan
    let plan = compute_sync_plan(source, dest, &filter, args.delete, args.force)?;

    if args.dry_run {
        // Print the plan without executing
        plan.print_summary();
        return Ok(());
    }

    if !plan.has_changes() {
        if !quiet {
            eprintln!("Already in sync. Nothing to do.");
        }
        return Ok(());
    }

    // Execute the plan
    let sync_start = std::time::Instant::now();
    let total_files = plan.files_to_copy + plan.files_to_update + plan.files_to_delete;
    let result = execute_sync_plan(&plan, quiet, args.verify)?;

    // Print summary with throughput
    if !quiet {
        let mut stats = TransferStats::new(total_files, plan.total_copy_bytes);
        stats.started = sync_start;
        stats.bytes_done = result.bytes_transferred;
        stats.files_done = result.files_copied + result.files_updated + result.files_deleted;
        stats.files_skipped = result.files_skipped;
        let throughput = ByteSize(stats.throughput_bps());

        eprintln!(
            "Sync complete: {} copied, {} updated, {} deleted, {} skipped ({}) in {:.1}s @ {}/s",
            result.files_copied,
            result.files_updated,
            result.files_deleted,
            result.files_skipped,
            ByteSize(result.bytes_transferred),
            stats.elapsed().as_secs_f64(),
            throughput,
        );
    }

    Ok(())
}
