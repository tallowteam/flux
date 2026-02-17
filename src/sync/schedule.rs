use std::path::Path;

use crate::error::FluxError;
use crate::transfer::filter::TransferFilter;

/// Run sync on a cron schedule. (Placeholder -- implemented in Task 2.)
pub fn scheduled_sync(
    _cron_expr: &str,
    _source: &Path,
    _dest: &Path,
    _filter: &TransferFilter,
    _delete_orphans: bool,
    _quiet: bool,
    _verify: bool,
    _force: bool,
) -> Result<(), FluxError> {
    Err(FluxError::SyncError("Schedule mode not yet implemented".to_string()))
}
