---
phase: 05-discovery-security
plan: 03
subsystem: network, cli
tags: [tcp, sender, receiver, file-transfer, mdns, encryption, tofu, cli-dispatch, tokio, framed-protocol]

# Dependency graph
requires:
  - phase: 05-discovery-security
    provides: "Plan 01: EncryptedChannel, DeviceIdentity, TrustStore. Plan 02: mDNS registration/discovery, FluxMessage protocol, encode/decode"
  - phase: 01-foundation
    provides: "FluxError, config::paths, CLI structure"
  - phase: 04-user-experience
    provides: "CLI args.rs pattern, gethostname, config/data dir env overrides for test isolation"
provides:
  - "send_file: TCP client that connects to receiver, handshakes, optionally encrypts, streams file chunks"
  - "start_receiver: TCP server with mDNS registration, TOFU trust checks, encrypted file reception"
  - "CLI commands: flux discover, flux send, flux receive, flux trust list/rm"
  - "resolve_device_target: @devicename mDNS lookup, host:port parsing"
  - "Sync wrappers (send_file_sync, start_receiver_sync) using local tokio Runtime"
affects: [06-polish, future-directory-send, future-pull-model]

# Tech tracking
tech-stack:
  added: []
  patterns: [tokio::runtime::Runtime::new() for sync-async bridge, LengthDelimitedCodec framing, TOFU auto-trust v1]

key-files:
  created:
    - src/net/sender.rs
    - src/net/receiver.rs
    - tests/phase5_integration.rs
  modified:
    - src/net/mod.rs
    - src/cli/args.rs
    - src/main.rs

key-decisions:
  - "tokio_util::bytes::Bytes re-export used instead of adding bytes crate directly"
  - "Receiver auto-trusts unknown devices in v1 (future: interactive prompt)"
  - "SSH-style WARNING on key change with connection rejection"
  - "find_unique_path for auto-rename on receiver (file_1.txt pattern)"
  - "Default trust action is 'list' (matches alias/queue pattern)"
  - "gethostname() as default device name for send/receive"

patterns-established:
  - "Sync-async bridge: tokio::runtime::Runtime::new().block_on() for commands needing async TCP"
  - "Device target resolution: @name -> mDNS lookup, host:port -> direct, host -> default port"
  - "Receiver connection handling: spawned tokio tasks per connection with Framed LengthDelimitedCodec"
  - "TOFU v1: auto-trust unknown, verify trusted, reject key-changed with SSH-style warning"

requirements-completed: [DISC-03, SEC-01, SEC-02, SEC-03, CLI-03]

# Metrics
duration: 7min
completed: 2026-02-17
---

# Phase 5 Plan 3: Send/Receive Protocol & CLI Integration Summary

**TCP file transfer with framed protocol, X25519 encrypted channels, TOFU trust verification, and discover/send/receive/trust CLI commands**

## Performance

- **Duration:** 7 min
- **Started:** 2026-02-17T02:04:09Z
- **Completed:** 2026-02-17T02:11:50Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments
- Complete send/receive file transfer over TCP with LengthDelimitedCodec framing and bincode protocol messages
- Optional end-to-end encryption via X25519 key exchange + XChaCha20-Poly1305 per-chunk encryption
- TOFU trust verification on receiver: auto-trust new devices, verify known devices, reject key changes with SSH-style warning
- Four new CLI commands: `flux discover`, `flux send`, `flux receive`, `flux trust` with proper help text and argument parsing
- 10 new Phase 5 integration tests passing (plus 2 loopback tests marked #[ignore] for manual verification)
- All 351 tests passing across Phases 1-5, zero regressions

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement net sender and receiver** - `9dbcd17` (feat)
2. **Task 2: Add CLI commands and main.rs dispatch** - `f0d77df` (feat)
3. **Task 3: Integration tests for Phase 5 features** - `b2389c9` (test)

## Files Created/Modified
- `src/net/sender.rs` - send_file async fn, resolve_device_target, send_file_sync wrapper, 5 unit tests
- `src/net/receiver.rs` - start_receiver async fn, handle_connection with TOFU, find_unique_path, start_receiver_sync, 4 unit tests
- `src/net/mod.rs` - Added pub mod sender/receiver exports
- `src/cli/args.rs` - DiscoverArgs, SendArgs, ReceiveArgs, TrustArgs/TrustAction/TrustRmArgs structs
- `src/main.rs` - Dispatch for Discover, Send, Receive, Trust commands with table output formatting
- `tests/phase5_integration.rs` - 12 tests (10 active, 2 ignored loopback): help text, trust commands, error handling, discovery

## Decisions Made
- Used `tokio_util::bytes::Bytes` re-export instead of adding `bytes` crate as a direct dependency (it's already a transitive dep)
- Receiver auto-trusts unknown devices in v1 with fingerprint display (future versions will add interactive prompt)
- SSH-style `WARNING: DEVICE IDENTIFICATION HAS CHANGED!` banner on key mismatch, with connection rejection
- `find_unique_path` auto-renames conflicting files on receiver (file_1.txt, file_2.txt pattern, up to 9999 then timestamp fallback)
- Default `flux trust` with no subcommand defaults to `list` (matches `flux alias` and `flux queue` patterns)
- `gethostname()` from the gethostname crate used as default device name for send/receive when --name not specified

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Used tokio_util::bytes re-export instead of bytes crate**
- **Found during:** Task 1 (sender.rs compilation)
- **Issue:** `bytes::Bytes` import failed -- bytes crate not in direct dependencies
- **Fix:** Changed to `tokio_util::bytes::Bytes` which re-exports the bytes types already available as a transitive dependency
- **Files modified:** src/net/sender.rs, src/net/receiver.rs
- **Verification:** `cargo check` passes
- **Committed in:** 9dbcd17 (Task 1 commit)

---

**Total deviations:** 1 auto-fixed (1 blocking)
**Impact on plan:** Minor import path change. No scope creep.

## Issues Encountered
None beyond the auto-fixed item above.

## User Setup Required
None - no external service configuration required.

## Next Phase Readiness
- All Phase 5 requirements complete: DISC-01, DISC-02, DISC-03, SEC-01, SEC-02, SEC-03, CLI-03
- Users can discover devices, send/receive files (plain or encrypted), and manage trusted devices
- Foundation ready for Phase 6 polish (directory send, pull model, etc.)
- 351 total tests passing (274 unit + 77 integration), zero regressions from Phases 1-4

## Self-Check: PASSED

All created files verified present:
- src/net/sender.rs
- src/net/receiver.rs
- tests/phase5_integration.rs

All commits verified in repository:
- 9dbcd17 (Task 1: net sender and receiver)
- f0d77df (Task 2: CLI commands and dispatch)
- b2389c9 (Task 3: integration tests)

---
*Phase: 05-discovery-security*
*Completed: 2026-02-17*
