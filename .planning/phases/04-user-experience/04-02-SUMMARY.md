---
phase: 04-user-experience
plan: 02
subsystem: config
tags: [config, toml, serde, conflict-resolution, dry-run, retry, failure-handling, clap]

# Dependency graph
requires:
  - phase: 04-user-experience
    provides: "AliasStore, flux_config_dir(), CLI subcommand structure from Plan 01"
provides:
  - "FluxConfig with ConflictStrategy, FailureStrategy, retry settings, serde defaults"
  - "load_config() for TOML-based config loading with graceful fallback"
  - "resolve_conflict() and find_unique_name() for file conflict resolution"
  - "Dry-run mode (--dry-run) previewing operations without I/O"
  - "Failure handling with retry/skip/pause strategies and exponential backoff"
  - "CLI flags --on-conflict, --on-error, --dry-run on CpArgs"
  - "Config merging: CLI flags override config.toml values"
affects: [04-user-experience, transfer-pipeline]

# Tech tracking
tech-stack:
  added: []
  patterns: [lazy-config-loading, cli-overrides-config, conflict-resolution-before-copy, dry-run-shared-validation, failure-retry-exponential-backoff]

key-files:
  created:
    - src/transfer/conflict.rs
  modified:
    - src/config/types.rs
    - src/cli/args.rs
    - src/transfer/mod.rs

key-decisions:
  - "Config loaded lazily inside execute_copy, not at CLI parse time -- keeps --help and non-transfer commands fast"
  - "CLI flags override config.toml values via Option<T>.unwrap_or(config.field) pattern"
  - "Ask conflict strategy falls back to Skip when stdin is not a TTY (non-interactive safety)"
  - "find_unique_name uses sequential numbering (file_1.txt) up to 9999 then timestamp fallback"
  - "Dry-run shares validation/alias/protocol pipeline, only skips actual I/O"
  - "Retry uses exponential backoff: delay_ms * 2^attempt"
  - "Pause failure strategy only prompts if stdin is TTY, returns error regardless"

patterns-established:
  - "Lazy config loading: load_config() called inside execute_copy, not at main"
  - "CLI-overrides-config: args.on_conflict.unwrap_or(config.conflict)"
  - "Conflict resolution before copy: resolve_conflict() returns Option<PathBuf>"
  - "Failure handling wrapper: copy_with_failure_handling() encapsulates retry/skip/pause"
  - "Dry-run as flag not separate path: same validation pipeline, skip I/O"

requirements-completed: [CONF-01, CONF-02, CONF-03, CONF-05]

# Metrics
duration: 6min
completed: 2026-02-17
---

# Phase 4 Plan 02: Configuration and Conflict Handling Summary

**TOML-backed config system with conflict/failure strategies, retry with exponential backoff, and dry-run preview mode integrated into the transfer pipeline**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-17T00:54:53Z
- **Completed:** 2026-02-17T01:01:14Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- FluxConfig expanded with ConflictStrategy, FailureStrategy, retry_count, retry_backoff_ms, default_destination, history_limit fields with serde defaults
- Config loading from config.toml via load_config() with graceful fallback to defaults when file missing or invalid
- Conflict resolution (overwrite/skip/rename/ask) applied before every file copy in both single-file and directory paths
- Dry-run mode walks full operation pipeline (alias resolution, protocol detection, validation) and reports per-file actions without I/O
- Failure handling with retry (exponential backoff), skip, and pause strategies in directory copy loop
- CLI flags --on-conflict, --on-error, --dry-run added to CpArgs with clap::ValueEnum derives
- 15 new tests: 7 config types (defaults, TOML roundtrip, partial config, strategy serialization) + 8 conflict (overwrite/skip/rename/ask/unique names)
- All 236 tests passing (188 unit + 48 integration) with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Expand FluxConfig with strategies, retry settings, and config loading** - `333bfac` (feat)
2. **Task 2: Conflict resolution logic, dry-run mode, and transfer integration** - `23b00cd` (feat)

## Files Created/Modified
- `src/config/types.rs` - ConflictStrategy, FailureStrategy enums; FluxConfig with serde defaults; load_config(); 7 unit tests
- `src/cli/args.rs` - --on-conflict, --on-error, --dry-run flags on CpArgs
- `src/transfer/conflict.rs` - resolve_conflict(), find_unique_name(), 8 unit tests
- `src/transfer/mod.rs` - Integrated config loading, conflict resolution, dry-run, failure handling into execute_copy and copy_directory

## Decisions Made
- Config loaded lazily inside execute_copy, not at CLI startup -- prevents config.toml errors from blocking --help or non-transfer commands (Pitfall 3 prevention)
- CLI flags use Option<T> and override config values via unwrap_or pattern -- clean merging without complex precedence logic
- Ask conflict strategy falls back to Skip when stdin is not a TTY -- prevents hanging in scripts/CI (non-interactive safety)
- find_unique_name uses sequential numbering (file_1.txt, file_2.txt) up to 9999 then falls back to Unix timestamp -- predictable, human-friendly
- Dry-run shares the full validation/alias/protocol/filter pipeline, only skips actual I/O -- ensures dry-run output matches real behavior (Pitfall 6 prevention)
- Retry uses exponential backoff (delay * 2^attempt) via thread::sleep -- simple, effective for transient I/O errors
- Used std::io::IsTerminal (stable since Rust 1.70) instead of atty crate for TTY detection -- fewer dependencies

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Fixed TOML serialization test for bare enums**
- **Found during:** Task 1 (config types tests)
- **Issue:** toml crate cannot serialize bare enum values (UnsupportedType error) -- tests tried `toml::to_string(&ConflictStrategy::Skip)` directly
- **Fix:** Wrapped enum values in a struct for serialization roundtrip tests
- **Files modified:** src/config/types.rs
- **Verification:** All 7 config::types tests pass
- **Committed in:** 333bfac (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 bug)
**Impact on plan:** Minor test fix. No scope creep.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Config system complete and integrated into transfer pipeline
- FluxConfig ready for future features (history_limit for Plan 03, default_destination for queue)
- Conflict resolution module reusable for any file operation
- Plan 03 (transfer queue) can build on config infrastructure
- Plan 04 (shell completions) uses existing clap CLI structure

## Self-Check: PASSED

All 4 files verified present. Both task commits (333bfac, 23b00cd) verified in git log.

---
*Phase: 04-user-experience*
*Completed: 2026-02-17*
