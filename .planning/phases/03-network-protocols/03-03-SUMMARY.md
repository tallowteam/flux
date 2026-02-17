---
phase: 03-network-protocols
plan: 03
subsystem: backend
tags: [smb, cifs, unc-paths, windows, network-share, platform-conditional, cfg-attributes]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: FluxBackend trait, LocalBackend, BackendFeatures, FileStat, FileEntry
  - phase: 03-network-protocols
    plan: 01
    provides: Protocol enum with Smb variant, detect_protocol parser, create_backend factory
provides:
  - SmbBackend implementing FluxBackend with all 6 trait methods
  - Platform-conditional compilation via #[cfg(windows)] / #[cfg(not(windows))]
  - Windows UNC path resolution (\\server\share\path via std::fs)
  - Non-Windows stub with clear feature flag error message
  - Backend factory wired for Protocol::Smb
  - SMB integration test stubs for manual network testing
affects: [05-security]

# Tech tracking
tech-stack:
  added: []  # No new dependencies; Windows uses std::fs natively
  patterns: [platform-conditional-backend, unc-path-delegation, cfg-windows-cfg-not-windows]

key-files:
  created:
    - src/backend/smb.rs
    - tests/smb_backend.rs
  modified:
    - src/backend/mod.rs
    - tests/protocol_detection.rs

key-decisions:
  - "Windows SMB uses native UNC paths via std::fs with no external dependencies (sambrs crate does not exist)"
  - "Non-Windows returns ProtocolError with clear message about smb feature flag and OS mount alternative"
  - "supports_parallel=false, supports_seek=false for SMB (network I/O not suitable for positional reads)"
  - "Integration tests updated from stub-error checks to real-backend routing verification"

patterns-established:
  - "Platform-conditional backend: #[cfg(windows)] for real impl, #[cfg(not(windows))] for stub with error"
  - "UNC path resolution: base_unc.join(relative_path) for all operations"
  - "SMB error mapping: NotFound -> SourceNotFound, PermissionDenied -> PermissionDenied, other -> ProtocolError"

requirements-completed: [PROT-02]

# Metrics
duration: 12min
completed: 2026-02-17
---

# Phase 3 Plan 03: SMB Backend Summary

**Platform-conditional SmbBackend delegating to std::fs via UNC paths on Windows, with non-Windows stub returning feature-flag guidance**

## Performance

- **Duration:** 12 min
- **Started:** 2026-02-16T23:59:19Z
- **Completed:** 2026-02-17T00:12:04Z
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments
- SmbBackend implements all 6 FluxBackend methods with Windows UNC path delegation via std::fs
- Platform-conditional compilation: real implementation on Windows, clear error stub on non-Windows
- Backend factory updated: Protocol::Smb routes to SmbBackend::connect(server, share)
- 12 SMB-specific tests (10 active unit/integration, 2 ignored network-dependent)
- Integration tests updated to verify real backend routing (not stub error messages)
- Total test suite at 188 tests (186 active, 2 ignored), all passing

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement platform-conditional SmbBackend** - `bd166b6` (feat)
2. **Task 2: Add SMB unit tests and integration test stubs** - `82c49fa` (test)

**Plan metadata:** [pending] (docs: complete plan)

## Files Created/Modified
- `src/backend/smb.rs` - SmbBackend with #[cfg(windows)] std::fs delegation and #[cfg(not(windows))] stub
- `src/backend/mod.rs` - Added `pub mod smb;` and Protocol::Smb arm in create_backend (committed by 03-04 in parallel)
- `tests/smb_backend.rs` - 4 routing tests + 2 ignored network roundtrip tests
- `tests/protocol_detection.rs` - Updated from stub-error checks to real-backend routing assertions

## Decisions Made
- **No sambrs dependency:** The `sambrs` crate referenced in the plan does not exist on crates.io. On Windows, std::fs natively handles UNC paths using the OS SMB client, so no external dependency is needed.
- **No pavao dependency for non-Windows:** Instead of pulling in pavao (which requires libsmbclient), the non-Windows implementation returns a clear ProtocolError directing users to either build with `--features smb` or mount the share with their OS and use a local path.
- **supports_parallel and supports_seek both false:** Network SMB operations don't support positional I/O needed for parallel chunked transfers. Sequential streaming is the correct approach.
- **256KB buffer size:** Matches LocalBackend's BUF_SIZE for consistency.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] sambrs crate does not exist on crates.io**
- **Found during:** Task 1 (dependency research)
- **Issue:** Plan specified `sambrs = "0.1"` under Windows dependencies, but the crate does not exist
- **Fix:** Removed sambrs dependency entirely. Windows SmbBackend uses std::fs directly with UNC paths, which works without any external crate for unauthenticated/session-authenticated shares
- **Files modified:** src/backend/smb.rs (no sambrs references)
- **Verification:** cargo build succeeds, UNC path tests pass
- **Committed in:** bd166b6 (Task 1 commit)

**2. [Rule 3 - Blocking] pavao crate unavailable/unnecessary for Phase 3**
- **Found during:** Task 1 (dependency research)
- **Issue:** Plan specified pavao for non-Windows SMB, but following user instruction for "simplest approach," non-Windows returns a stub error
- **Fix:** Non-Windows implementation returns ProtocolError with actionable guidance instead of attempting pavao integration
- **Files modified:** src/backend/smb.rs
- **Verification:** Compiles on Windows, non-Windows code paths covered by cfg attributes
- **Committed in:** bd166b6 (Task 1 commit)

**3. [Rule 1 - Bug] Integration tests expected old stub error messages**
- **Found during:** Task 1 (verification)
- **Issue:** protocol_detection integration tests checked for "SMB backend not yet implemented" but SmbBackend is now real
- **Fix:** Updated tests to verify backend routing (failure from unreachable server) rather than stub messages
- **Files modified:** tests/protocol_detection.rs
- **Verification:** All 9 protocol detection integration tests pass
- **Committed in:** bd166b6 (Task 1 commit)

**4. [Rule 3 - Blocking] ssh2 vendored-openssl build failure**
- **Found during:** Task 1 (parallel plan interference)
- **Issue:** The SFTP plan (03-02) added ssh2 with vendored-openssl to Cargo.toml, but Git Bash's Perl cannot build OpenSSL from source
- **Fix:** ssh2 was commented out in Cargo.toml and sftp module disabled by parallel plans; no action needed from SMB plan
- **Files modified:** None by this plan (handled by parallel plans)
- **Verification:** cargo test succeeds with ssh2 commented out

---

**Total deviations:** 4 auto-fixed (2 blocking deps, 1 bug, 1 cross-plan blocking)
**Impact on plan:** Core approach unchanged -- Windows SmbBackend delegates to std::fs as planned. External dependencies removed as unnecessary. No scope creep.

## Issues Encountered
- Parallel execution with plans 03-02 (SFTP) and 03-04 (WebDAV) caused file conflicts in shared files (mod.rs, error.rs, Cargo.toml). Resolved by careful staging of only SMB-specific files per commit.

## User Setup Required

None - no external service configuration required. SMB shares are accessed using Windows native UNC path support with the current user's session credentials.

## Next Phase Readiness
- SMB backend complete and tested for Windows
- Non-Windows users directed to mount shares via OS tools
- All 188 tests passing (186 active, 2 ignored network-dependent)
- Ready for Phase 5 (Security) to add authenticated SMB support if needed

## Self-Check: PASSED

- src/backend/smb.rs: FOUND
- tests/smb_backend.rs: FOUND
- Commit bd166b6 (Task 1): FOUND
- Commit 82c49fa (Task 2): FOUND
- All 188 tests passing (cargo test): VERIFIED

---
*Phase: 03-network-protocols*
*Completed: 2026-02-17*
