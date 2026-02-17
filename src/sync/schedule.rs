use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use bytesize::ByteSize;
use chrono::Utc;
use cron::Schedule;

use crate::error::FluxError;
use crate::transfer::filter::TransferFilter;

use super::engine::{compute_sync_plan, execute_sync_plan};

/// Normalize a cron expression to 6+ field format expected by the `cron` crate.
///
/// The `cron` crate requires 6 or 7 fields (sec min hour day month dow [year]).
/// Standard cron uses 5 fields (min hour day month dow). If the user provides
/// a 5-field expression, we prepend "0 " to set seconds to 0, making it
/// compatible with the cron crate.
fn normalize_cron_expression(expr: &str) -> String {
    let field_count = expr.split_whitespace().count();
    if field_count == 5 {
        format!("0 {}", expr)
    } else {
        expr.to_string()
    }
}

/// Run sync on a cron schedule, sleeping until the next fire time and
/// then executing compute_sync_plan + execute_sync_plan.
///
/// Parses the cron expression, enters a tokio-based async loop that
/// calculates the next occurrence, sleeps until then, and runs sync.
/// Runs forever until Ctrl+C.
pub fn scheduled_sync(
    cron_expr: &str,
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
    quiet: bool,
    verify: bool,
    force: bool,
) -> Result<(), FluxError> {
    let normalized = normalize_cron_expression(cron_expr);

    let schedule = Schedule::from_str(&normalized).map_err(|e| {
        FluxError::SyncError(format!("Invalid cron expression '{}': {}", cron_expr, e))
    })?;

    eprintln!("Scheduled sync: {} -> {}", source.display(), dest.display());
    eprintln!("Cron: {}", cron_expr);

    let rt = tokio::runtime::Runtime::new().map_err(|e| FluxError::Io { source: e })?;

    rt.block_on(async {
        loop {
            let next = schedule
                .upcoming(Utc)
                .next()
                .ok_or_else(|| FluxError::SyncError("No upcoming schedule times".to_string()))?;

            let duration = (next - Utc::now())
                .to_std()
                .unwrap_or(Duration::from_secs(1));

            eprintln!(
                "Next sync at: {}",
                next.format("%Y-%m-%d %H:%M:%S UTC")
            );

            tokio::time::sleep(duration).await;

            // Run sync
            let plan = compute_sync_plan(source, dest, filter, delete_orphans, force)?;

            if !plan.has_changes() {
                if !quiet {
                    let timestamp = chrono::Local::now().format("%H:%M:%S");
                    eprintln!("[{}] Already in sync. Nothing to do.", timestamp);
                }
                continue;
            }

            let result = execute_sync_plan(&plan, quiet, verify)?;

            if !quiet {
                let timestamp = chrono::Local::now().format("%H:%M:%S");
                eprintln!(
                    "[{}] Sync complete: {} copied, {} updated, {} deleted, {} skipped ({})",
                    timestamp,
                    result.files_copied,
                    result.files_updated,
                    result.files_deleted,
                    result.files_skipped,
                    ByteSize(result.bytes_transferred),
                );
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_expression_parsing_valid() {
        // 6-field cron expression (sec min hour day month dow)
        let schedule = Schedule::from_str("0 */5 * * * *");
        assert!(
            schedule.is_ok(),
            "Valid 6-field cron expression should parse: {:?}",
            schedule.err()
        );
    }

    #[test]
    fn test_cron_expression_parsing_invalid() {
        let result = Schedule::from_str("not a cron");
        assert!(result.is_err(), "Invalid cron expression should fail");
    }

    #[test]
    fn test_five_field_cron_auto_expand() {
        // 5-field standard cron -> should get "0 " prepended
        let normalized = normalize_cron_expression("*/5 * * * *");
        assert_eq!(normalized, "0 */5 * * * *");

        // Verify the normalized version parses
        let schedule = Schedule::from_str(&normalized);
        assert!(
            schedule.is_ok(),
            "Normalized 5-field cron should parse: {:?}",
            schedule.err()
        );
    }

    #[test]
    fn test_six_field_cron_unchanged() {
        // 6-field cron should pass through unchanged
        let normalized = normalize_cron_expression("0 */5 * * * *");
        assert_eq!(normalized, "0 */5 * * * *");
    }

    #[test]
    fn test_next_fire_time_computed() {
        let schedule = Schedule::from_str("0 */1 * * * *").unwrap();
        let next = schedule.upcoming(Utc).next();
        assert!(next.is_some(), "Should have a next fire time");

        let next_time = next.unwrap();
        assert!(
            next_time > Utc::now(),
            "Next fire time should be in the future"
        );
    }

    #[test]
    fn test_invalid_cron_produces_sync_error() {
        // Verify the full scheduled_sync function returns SyncError for bad cron
        use tempfile::TempDir;
        let dir = TempDir::new().unwrap();
        let source = dir.path().join("src");
        let dest = dir.path().join("dst");
        std::fs::create_dir_all(&source).unwrap();
        std::fs::create_dir_all(&dest).unwrap();

        let filter = TransferFilter::new(&[], &[]).unwrap();
        let result = scheduled_sync(
            "not valid",
            &source,
            &dest,
            &filter,
            false,
            true,
            false,
            false,
        );
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("Invalid cron expression"),
            "Error should mention invalid cron: {}",
            err_msg
        );
    }
}
