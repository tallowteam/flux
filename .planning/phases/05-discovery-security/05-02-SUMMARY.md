---
phase: 05-discovery-security
plan: 02
subsystem: discovery, network
tags: [mdns, mdns-sd, bincode, serde, tcp-protocol, service-discovery, lan]

# Dependency graph
requires:
  - phase: 01-foundation
    provides: "FluxError enum, project structure, Cargo.toml"
provides:
  - "mDNS service registration and browsing (discover_flux_devices, register_flux_service)"
  - "DiscoveredDevice and FluxService types for LAN device discovery"
  - "FluxMessage protocol enum with bincode serialization (Handshake, FileHeader, DataChunk, TransferComplete, Error)"
  - "encode_message/decode_message for binary protocol framing"
  - "SERVICE_TYPE, DEFAULT_PORT, PROTOCOL_VERSION, MAX_FRAME_SIZE, CHUNK_SIZE constants"
affects: [05-03-send-receive, net, cli]

# Tech tracking
tech-stack:
  added: [mdns-sd 0.18, gethostname 0.5, bincode 2.0 (serde mode), tokio-util 0.7 (codec)]
  patterns: [mdns-sd ServiceDaemon sync pattern, bincode 2.x serde::encode_to_vec/decode_from_slice, DNS label sanitization]

key-files:
  created:
    - src/discovery/mod.rs
    - src/discovery/service.rs
    - src/discovery/mdns.rs
    - src/net/mod.rs
    - src/net/protocol.rs
  modified:
    - src/main.rs

key-decisions:
  - "bincode 2.x serde API (bincode::serde::encode_to_vec) for FluxMessage serialization -- standard config, compact binary output"
  - "mdns-sd ServiceDaemon is sync (internal thread) -- no async needed for discovery"
  - "Device name sanitized to DNS label spec: replace non-alphanumeric with hyphens, collapse, truncate to 63 chars"
  - "Discovery deduplicates by instance name (first seen wins)"
  - "FluxMessage uses Vec<u8> for public_key and nonce fields (serde-compatible, flexible length)"
  - "Prefer IPv4 addresses when resolving discovered devices"

patterns-established:
  - "mDNS pattern: ServiceDaemon::new() -> browse/register -> recv_timeout loop -> shutdown"
  - "Protocol serialization: bincode::serde::encode_to_vec / decode_from_slice with config::standard()"
  - "DNS label sanitization: alphanumeric + hyphen only, collapsed, trimmed, 63-char max"

requirements-completed: [DISC-01, DISC-02]

# Metrics
duration: 29min
completed: 2026-02-17
---

# Phase 5 Plan 2: Discovery & Protocol Types Summary

**mDNS service discovery via mdns-sd with _flux._tcp.local. registration/browsing, and FluxMessage protocol enum with bincode 2.x binary serialization**

## Performance

- **Duration:** 29 min (includes ~15 min waiting for parallel Plan 05-01 build lock)
- **Started:** 2026-02-17T01:31:23Z
- **Completed:** 2026-02-17T02:00:49Z
- **Tasks:** 2
- **Files modified:** 6

## Accomplishments
- Discovery module with mDNS registration (register_flux_service) and browsing (discover_flux_devices) via mdns-sd
- FluxService and DiscoveredDevice types with DNS label sanitization for device names
- FluxMessage enum covering full transfer lifecycle: Handshake, HandshakeAck, FileHeader, DataChunk, TransferComplete, Error
- Binary encode/decode functions using bincode 2.x serde API with standard config
- 34 new unit tests (14 discovery + 20 protocol), all passing. Total project tests: 312

## Task Commits

Each task was committed atomically:

1. **Task 1: Implement discovery module (mDNS registration and browsing)** - `9ae0e66` (feat)
2. **Task 2: Define transfer protocol message types** - `e502436` (feat)

## Files Created/Modified
- `src/discovery/mod.rs` - Module exports for mdns and service submodules
- `src/discovery/service.rs` - DiscoveredDevice, FluxService types, SERVICE_TYPE/DEFAULT_PORT constants, hostname sanitization
- `src/discovery/mdns.rs` - register_flux_service (mDNS registration) and discover_flux_devices (mDNS browsing)
- `src/net/mod.rs` - Module exports for protocol submodule
- `src/net/protocol.rs` - FluxMessage enum, encode_message/decode_message, PROTOCOL_VERSION/MAX_FRAME_SIZE/CHUNK_SIZE constants
- `src/main.rs` - Added `mod discovery;` and `mod net;` declarations

## Decisions Made
- **bincode 2.x serde API:** Used `bincode::serde::encode_to_vec` and `bincode::serde::decode_from_slice` with `bincode::config::standard()`. The bincode 2.x API separates native Encode/Decode traits from serde; since FluxMessage uses serde derives, the `serde` feature module is the correct entry point.
- **Vec<u8> for crypto fields:** FluxMessage uses `Vec<u8>` instead of fixed-size arrays for `public_key` (32 bytes) and `nonce` (24 bytes) because serde's Serialize/Deserialize works more naturally with Vec than fixed arrays, and it provides flexibility for future crypto changes.
- **IPv4 preference:** Discovery prefers IPv4 addresses when multiple are available, falling back to IPv6. This maximizes compatibility on typical LAN setups.
- **Instance name extraction:** Parses mDNS fullname by finding the SERVICE_TYPE suffix and extracting the prefix as the instance name, rather than splitting on dots (which would break escaped dots in instance names).

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- Build lock contention: Plan 05-01 was compiling in parallel, holding the Cargo build directory lock for several minutes during the first `cargo test` attempt. Resolved by waiting for the lock to release.
- Plan 05-01 added `mod discovery;` and `mod security;` to main.rs in their commit (which included my discovery module declaration), and also added all Phase 5 dependencies to Cargo.toml and error variants to error.rs. No duplicate work was needed.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Discovery module ready for `flux discover` CLI command (Plan 05-03)
- FluxMessage protocol ready for sender/receiver implementation (Plan 05-03)
- Constants (SERVICE_TYPE, DEFAULT_PORT, PROTOCOL_VERSION, MAX_FRAME_SIZE, CHUNK_SIZE) available for wire protocol implementation
- Security module (Plan 05-01) provides EncryptedChannel for encrypting DataChunk payloads

## Self-Check: PASSED

All created files verified present:
- src/discovery/mod.rs
- src/discovery/service.rs
- src/discovery/mdns.rs
- src/net/mod.rs
- src/net/protocol.rs

All commits verified in repository:
- 9ae0e66 (Task 1: discovery module)
- e502436 (Task 2: protocol types)

---
*Phase: 05-discovery-security*
*Completed: 2026-02-17*
