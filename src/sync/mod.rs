pub mod engine;
pub mod plan;

use std::path::Path;

use bytesize::ByteSize;

use crate::cli::args::SyncArgs;
use crate::error::FluxError;
use crate::transfer::filter::TransferFilter;

use self::engine::{compute_sync_plan, execute_sync_plan};

/// Entry point for the `flux sync` command.
///
/// Validates inputs, builds filter, computes sync plan, and either
/// prints it (dry-run) or executes it.
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
    let result = execute_sync_plan(&plan, quiet, args.verify)?;

    // Print summary
    if !quiet {
        eprintln!(
            "Sync complete: {} copied, {} updated, {} deleted, {} skipped ({})",
            result.files_copied,
            result.files_updated,
            result.files_deleted,
            result.files_skipped,
            ByteSize(result.bytes_transferred),
        );
    }

    Ok(())
}
