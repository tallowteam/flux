---
phase: 03-network-protocols
plan: 01
subsystem: protocol
tags: [url-parsing, protocol-detection, backend-factory, smb, sftp, webdav]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: FluxBackend trait, LocalBackend, CpArgs CLI parsing, execute_copy
  - phase: 02-performance
    provides: Parallel chunked copy, resume manifests, compression, throttling
provides:
  - Protocol enum with Local, Sftp, Smb, WebDav variants
  - detect_protocol() parser for UNC paths, URLs, and local paths
  - Auth enum skeleton for credential types
  - create_backend() factory dispatching Protocol to FluxBackend
  - CpArgs String-based source/dest (preserves network URIs)
  - ProtocolError variant in FluxError with suggestion hint
affects: [03-02-sftp-backend, 03-03-smb-backend, 03-04-webdav-backend, 05-security]

# Tech tracking
tech-stack:
  added: [url 2.x, rpassword 7.x]
  patterns: [protocol-detection-before-transfer, backend-factory-dispatch, string-based-cli-args]

key-files:
  created:
    - src/protocol/mod.rs
    - src/protocol/parser.rs
    - src/protocol/auth.rs
    - tests/protocol_detection.rs
  modified:
    - src/cli/args.rs
    - src/backend/mod.rs
    - src/transfer/mod.rs
    - src/main.rs
    - src/error.rs
    - Cargo.toml

key-decisions:
  - "CpArgs source/dest migrated from PathBuf to String to preserve network URI formats"
  - "Protocol detection order: UNC backslash > UNC forward > URL scheme > local fallback"
  - "Windows drive letters (C:) detected as local paths despite URL parser treating them as schemes"
  - "Network backends return ProtocolError stubs until Plans 02-04 implement them"
  - "Auth enum includes Password, KeyFile, Agent variants as skeleton for Phase 5"

patterns-established:
  - "Protocol detection at start of execute_copy before any filesystem operations"
  - "Backend factory pattern: create_backend(Protocol) -> Box<dyn FluxBackend>"
  - "Inline credential extraction from URL userinfo (user:password@host)"

requirements-completed: [PROT-05]

# Metrics
duration: 7min
completed: 2026-02-16
---

# Phase 3 Plan 01: Protocol Detection Infrastructure Summary

**Protocol detection module with URL/UNC parsing, backend factory dispatch, and String-based CLI args preserving network URIs**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-16T23:48:22Z
- **Completed:** 2026-02-16T23:55:41Z
- **Tasks:** 2
- **Files modified:** 11

## Accomplishments
- Protocol enum with four variants (Local, Sftp, Smb, WebDav) covering all planned network backends
- detect_protocol() correctly classifies UNC paths, sftp:/ssh:/smb:/https:/http:/dav:/webdav: URLs, and local paths
- CpArgs migrated from PathBuf to String with zero regressions on all 124 existing tests
- Backend factory routes Local to LocalBackend and returns clear stub errors for unimplemented protocols
- 27 new tests (18 parser unit + 9 integration) all passing, total suite at 151 tests

## Task Commits

Each task was committed atomically:

1. **Task 1: Create protocol detection module and update CLI args** - `c7cd0f3` (feat)
2. **Task 2: Backend factory, execute_copy routing, integration tests** - `8bc4dff` (test)

**Plan metadata:** [pending] (docs: complete plan)

## Files Created/Modified
- `src/protocol/mod.rs` - Protocol enum (Local/Sftp/Smb/WebDav), is_local(), local_path(), name() helpers
- `src/protocol/parser.rs` - detect_protocol() with UNC and URL parsing, 18 unit tests
- `src/protocol/auth.rs` - Auth enum skeleton (None/Password/KeyFile/Agent) for Phase 5
- `src/cli/args.rs` - CpArgs source/dest changed from PathBuf to String
- `src/backend/mod.rs` - Added create_backend() factory function with Protocol import
- `src/transfer/mod.rs` - Integrated protocol detection and backend validation at top of execute_copy
- `src/main.rs` - Added protocol module, updated tracing from .display() to %
- `src/error.rs` - Added ProtocolError variant with suggestion hint
- `Cargo.toml` - Added url 2.x and rpassword 7.x dependencies
- `tests/protocol_detection.rs` - 9 integration tests for local and network protocol paths

## Decisions Made
- CpArgs source/dest migrated from PathBuf to String: PathBuf normalizes paths and destroys sftp:// prefixes and UNC paths
- Protocol detection order: UNC backslash first (highest priority), then UNC forward, then URL scheme parsing, then local fallback
- Windows drive letter paths (C:\) detected correctly as Local despite url crate parsing single-letter schemes
- Inline credentials extracted from URL userinfo for WebDAV (user:password@host)
- Network backend stubs return ProtocolError with descriptive message and hint

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered

None.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Protocol detection infrastructure complete and tested, ready for Plans 02-04
- SFTP backend (Plan 02) can use Protocol::Sftp fields directly
- SMB backend (Plan 03) can use both UNC and smb:// URL parsed fields
- WebDAV backend (Plan 04) receives full URL string and optional Auth
- All existing Phase 1+2 functionality preserved (151 tests passing)

## Self-Check: PASSED

- All 11 created/modified files verified present on disk
- Commit c7cd0f3 (Task 1) verified in git log
- Commit 8bc4dff (Task 2) verified in git log
- All 151 tests passing (117 unit + 10 integration + 15 phase2 + 9 protocol_detection)

---
*Phase: 03-network-protocols*
*Completed: 2026-02-16*
