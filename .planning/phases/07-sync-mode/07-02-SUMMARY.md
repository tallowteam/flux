---
phase: 07-sync-mode
plan: 02
subsystem: sync
tags: [sync, watch, cron, notify, debounce, schedule, filesystem-events]

# Dependency graph
requires:
  - phase: 07-sync-mode-plan-01
    provides: compute_sync_plan, execute_sync_plan, SyncPlan, SyncAction, TransferFilter
  - phase: 01-foundation
    provides: FluxError, copy_file_with_progress, TransferFilter
provides:
  - watch_and_sync function (continuous filesystem monitoring with 2s debounce)
  - scheduled_sync function (cron-based recurring sync with tokio sleep loop)
  - normalize_cron_expression (5-field to 6-field auto-expansion)
  - run_sync_cycle reusable helper for one sync pass
  - --watch and --schedule CLI dispatch in execute_sync
affects: [tui]

# Tech tracking
tech-stack:
  added: []
  patterns: [notify-debouncer-full event loop with recv_timeout, cron Schedule with tokio sleep loop, 5-field cron auto-expansion]

key-files:
  created:
    - src/sync/watch.rs
    - src/sync/schedule.rs
  modified:
    - src/sync/mod.rs
    - tests/sync_tests.rs

key-decisions:
  - "recv_timeout(500ms) event loop for watch mode -- allows natural Ctrl+C without ctrlc crate"
  - "5-field cron auto-expansion: prepend '0 ' for user convenience (standard cron -> cron crate 6-field format)"
  - "tokio::runtime::Runtime::new() with block_on for schedule loop -- reuses existing tokio dependency"
  - "run_sync_cycle helper extracted for reuse by watch mode event loop"

patterns-established:
  - "Watch mode: debouncer -> mpsc channel -> recv_timeout loop with sync cycle on events"
  - "Schedule mode: cron parse -> tokio async loop with sleep_until next fire time"
  - "Spawned process integration tests with kill pattern for long-running modes"

requirements-completed: [SYNC-03, SYNC-04]

# Metrics
duration: 6min
completed: 2026-02-17
---

# Phase 7 Plan 02: Watch Mode and Schedule Mode Summary

**Filesystem watch mode with notify-debouncer-full (2s debounce) and cron-scheduled recurring sync with 5-field auto-expansion**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-17T03:32:19Z
- **Completed:** 2026-02-17T03:38:19Z
- **Tasks:** 3
- **Files modified:** 4

## Accomplishments
- `flux sync --watch source/ dest/` monitors source directory for changes and auto-syncs with 2-second debounce
- `flux sync --schedule "*/5 * * * *" source/ dest/` runs sync on cron schedule with auto-expansion of 5-field expressions
- Watch mode performs initial sync on startup, then re-syncs on filesystem events with timestamped output
- Schedule mode prints "Next sync at:" before each sleep, runs sync on wake
- 6 new integration tests (watch initial sync, schedule invalid cron, schedule prints next time, nested dirs, verify flag, force delete)
- 10 new unit tests (4 watch + 6 schedule), total test suite now 432 tests, 0 failures

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement watch mode with notify-debouncer-full** - `c373794` (feat)
2. **Task 2: Implement schedule mode with cron parsing and tokio sleep loop** - `54b0814` (feat)
3. **Task 3: Integration tests for watch, schedule, and sync polish** - `192edc1` (test)

## Files Created/Modified
- `src/sync/watch.rs` - watch_and_sync function with debounced event loop, run_sync_cycle helper, 4 unit tests
- `src/sync/schedule.rs` - scheduled_sync function with cron parsing, 5-field auto-expansion, tokio sleep loop, 6 unit tests
- `src/sync/mod.rs` - Added watch/schedule module declarations and dispatch in execute_sync
- `tests/sync_tests.rs` - 6 new integration tests for watch/schedule modes plus sync polish (15 total sync tests)

## Decisions Made
- Used recv_timeout(500ms) event loop instead of ctrlc crate -- simpler, allows natural Ctrl+C termination without extra dependency
- 5-field cron auto-expansion: detect field count and prepend "0 " -- the cron crate requires 6+ fields but users expect standard 5-field format
- Created tokio Runtime in scheduled_sync (not in main) since main() is synchronous -- matches existing pattern from TUI phase
- Extracted run_sync_cycle helper for reuse between watch mode and potential future callers

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Phase 7 (Sync Mode) is now fully complete: SYNC-01, SYNC-02, SYNC-03, SYNC-04 all implemented
- All 7 phases of the Flux project are complete
- 432 total tests passing across all phases
- Full feature set: file copy, parallel transfer, resume, SFTP/SMB/WebDAV backends, aliases, queue, history, TUI, sync with watch and schedule modes

## Self-Check: PASSED

- [x] src/sync/watch.rs exists
- [x] src/sync/schedule.rs exists
- [x] src/sync/mod.rs exists
- [x] tests/sync_tests.rs exists
- [x] Commit c373794 found (Task 1)
- [x] Commit 54b0814 found (Task 2)
- [x] Commit 192edc1 found (Task 3)
- [x] cargo check: clean compilation
- [x] cargo test: all tests pass (432 total, 0 failures)

---
*Phase: 07-sync-mode*
*Completed: 2026-02-17*
