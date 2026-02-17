---
phase: 04-user-experience
plan: 01
subsystem: cli
tags: [aliases, toml, dirs, config, path-resolution, clap]

# Dependency graph
requires:
  - phase: 03-network-protocols
    provides: "Protocol detection (detect_protocol) and CpArgs with String source/dest"
provides:
  - "AliasStore with CRUD operations and TOML persistence"
  - "resolve_alias() function for name:subpath pattern expansion"
  - "validate_alias_name() with scheme/drive-letter collision prevention"
  - "flux_config_dir() and flux_data_dir() platform directory helpers"
  - "CLI subcommands: flux add, flux alias, flux alias rm"
  - "Alias resolution integrated into execute_copy before detect_protocol"
affects: [04-user-experience, transfer-pipeline]

# Tech tracking
tech-stack:
  added: [chrono, clap_complete]
  patterns: [alias-resolution-before-protocol-detection, atomic-toml-writes, graceful-config-degradation]

key-files:
  created:
    - src/config/paths.rs
    - src/config/aliases.rs
  modified:
    - Cargo.toml
    - src/config/mod.rs
    - src/error.rs
    - src/cli/args.rs
    - src/main.rs
    - src/transfer/mod.rs

key-decisions:
  - "Alias resolution runs BEFORE detect_protocol in execute_copy, not inside parser"
  - "AliasStore::default() provides graceful degradation when config dir unavailable"
  - "Aliases stored in separate aliases.toml (not config.toml) for independent editing"
  - "Atomic writes via temp file + rename for crash safety"
  - "Single-char alias names rejected to prevent drive letter collision (C:)"
  - "Reserved URL scheme names (sftp, ssh, smb, etc.) rejected at add time"
  - "Default destination uses 'default' as a regular alias name"

patterns-established:
  - "Alias resolution before protocol detection: resolve_alias() -> detect_protocol()"
  - "Platform config directory: flux_config_dir() wrapping dirs::config_dir()"
  - "Graceful config degradation: AliasStore::default() when config unavailable"
  - "Atomic TOML writes: write to .toml.tmp then rename"

requirements-completed: [PATH-01, PATH-02, PATH-03, PATH-05]

# Metrics
duration: 6min
completed: 2026-02-17
---

# Phase 4 Plan 01: Path Alias System Summary

**TOML-backed path alias system with CLI management, name validation, and pre-protocol-detection alias resolution in the transfer pipeline**

## Performance

- **Duration:** 6 min
- **Started:** 2026-02-17T00:45:12Z
- **Completed:** 2026-02-17T00:51:12Z
- **Tasks:** 2
- **Files modified:** 8

## Accomplishments
- AliasStore with load/save/add/remove/get/list operations, persisted in aliases.toml with atomic writes
- Alias resolution (name: and name:subpath patterns) integrated into execute_copy before protocol detection
- CLI subcommands: `flux add <name> <path>`, `flux alias` (list), `flux alias rm <name>`
- Name validation rejects single chars, digit-leading names, reserved URL schemes, and special chars
- 20 new unit tests covering resolution, validation, and persistence roundtrips
- All 173 unit tests + 56 integration tests pass with zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Config directory helpers, AliasStore, and error types** - `de72a37` (feat)
2. **Task 2: Alias CLI subcommands and transfer integration** - `0897135` (feat)

## Files Created/Modified
- `src/config/paths.rs` - Platform-specific config/data directory helpers (flux_config_dir, flux_data_dir)
- `src/config/aliases.rs` - AliasStore CRUD, resolve_alias, validate_alias_name with 18 unit tests
- `src/config/mod.rs` - Module exports for aliases and paths
- `src/error.rs` - AliasError variant with suggestion, From<toml::ser::Error> impl
- `src/cli/args.rs` - Add, Alias, AliasAction::Rm subcommands with AddArgs, AliasArgs, AliasRmArgs
- `src/main.rs` - Command dispatch for Add (validate+save) and Alias (list or rm)
- `src/transfer/mod.rs` - Alias resolution before detect_protocol in execute_copy
- `Cargo.toml` - Added chrono and clap_complete dependencies

## Decisions Made
- Alias resolution runs BEFORE detect_protocol in execute_copy -- aliases are a user-facing convenience layer, not part of protocol parsing
- AliasStore::default() provides graceful degradation when config dir is unavailable (e.g., restricted environments)
- Separate aliases.toml (not config.toml) for clean separation -- aliases change frequently, config rarely
- Atomic writes via temp file + rename for crash safety on alias save
- Single-char names rejected to avoid collision with Windows drive letters (C:, D:)
- Reserved URL scheme names (sftp, ssh, smb, https, http, webdav, dav, ftp) rejected at add-time, not at use-time
- Default destination uses the regular alias name "default" -- no special handling needed beyond standard alias resolution
- chrono and clap_complete added to Cargo.toml now (needed by Plans 03/04) to avoid future merge conflicts

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
None

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- Alias system complete and integrated into transfer pipeline
- Config directory infrastructure (flux_config_dir, flux_data_dir) ready for Plans 02-04 (queue, history, config)
- clap_complete dependency ready for Plan 04 (shell completions)
- chrono dependency ready for Plan 03 (transfer history timestamps)

## Self-Check: PASSED

All 9 files verified present. Both task commits (de72a37, 0897135) verified in git log.

---
*Phase: 04-user-experience*
*Completed: 2026-02-17*
