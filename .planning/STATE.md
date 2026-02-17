# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-16)

**Core value:** Transfer files at maximum network speed with zero friction
**Current focus:** Phase 5 - Discovery & Security

## Current Position

Phase: 5 of 7 (Discovery & Security) -- COMPLETE
Plan: 3 of 3 in current phase
Status: Phase Complete
Last activity: 2026-02-17 -- Completed 05-03-PLAN.md (Send/Receive protocol + CLI integration)

Progress: [########################] 76%

## Performance Metrics

**Velocity:**
- Total plans completed: 17
- Average duration: 14min
- Total execution time: 4.0 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 73min | 24min |
| 02-performance | 3/3 | 22min | 7min |
| 03-network-protocols | 4/4 | 55min | 14min |
| 04-user-experience | 4/4 | 21min | 5min |
| 05-discovery-security | 3/3 | 61min | 20min |

**Recent Trend:**
- Last 5 plans: 04-03 (4min), 04-04 (5min), 05-01 (25min), 05-02 (29min), 05-03 (7min)
- Trend: Phase 5 Plan 03 fast (7min) -- integration plan wiring existing modules, no new deps to compile

*Updated after each plan completion*

## Accumulated Context

### Decisions

Decisions are logged in PROJECT.md Key Decisions table.
Recent decisions affecting current work:

- [01-01] Used thiserror 2.x with renamed fields (src/dst) to avoid auto-source detection conflict
- [01-01] MSVC toolchain with VS Build Tools for standard Windows compilation
- [01-01] All tracing output to stderr, stdout reserved for machine-parseable output
- [01-02] FluxBackend trait is synchronous for Phase 1; async evolution deferred to Phase 3
- [01-02] 256KB BufReader/BufWriter buffers for file copy performance
- [01-02] Progress bars render to stderr via ProgressDrawTarget::stderr()
- [01-02] IoContext enum maps PermissionDenied to different FluxError variants based on read vs write
- [01-03] Match globs against both full path and file name for intuitive behavior (*.log matches at any depth)
- [01-03] Two-pass directory walk: count pass for progress total, copy pass for actual transfer
- [01-03] Per-file progress bars hidden during directory copy; only directory-level file count bar shown
- [01-03] Individual file errors collected in TransferResult, not fatal to directory copy
- [01-03] Trailing slash detection checks both / and \\ for Windows compatibility
- [02-01] Chunk remainder absorbed by last chunk (not distributed), matching standard chunking pattern
- [02-01] auto_chunk_count capped at std::thread::available_parallelism to avoid over-subscribing CPU
- [02-01] Positional I/O uses cfg(unix)/cfg(windows) with FileExt traits, no Mutex needed for parallel reads
- [02-01] read_at_exact and write_at_all retry on Interrupted errors, matching std behavior
- [02-02] Parallel copy uses rayon par_iter_mut with try_for_each for error propagation across threads
- [02-02] Destination file pre-allocated with set_len before parallel writes for correctness
- [02-02] Per-chunk BLAKE3 hashes computed during transfer regardless of --verify flag
- [02-02] Post-transfer --verify does whole-file hash comparison for maximum confidence
- [02-02] 256KB buffer per chunk thread matches existing Phase 1 buffer sizes
- [02-03] Resume manifest stored as .flux-resume.json sidecar next to destination for human-readability
- [02-03] Bandwidth limit forces sequential copy to avoid shared token bucket complexity
- [02-03] Compression infrastructure ready for Phase 3; local copies pass through unchanged
- [02-03] Manifest uses crash-safe writes (flush + sync_all) to survive interruptions
- [02-03] Incompatible manifests auto-deleted and transfer restarts fresh
- [03-01] CpArgs source/dest migrated from PathBuf to String to preserve network URI formats
- [03-01] Protocol detection order: UNC backslash > UNC forward > URL scheme > local fallback
- [03-01] Windows drive letters (C:) detected as local paths despite URL parser treating them as schemes
- [03-01] Network backends return ProtocolError stubs until Plans 02-04 implement them
- [03-01] Auth enum includes Password, KeyFile, Agent variants as skeleton for Phase 5
- [03-04] Used reqwest::blocking directly instead of async reqwest_dav -- avoids tokio runtime bridging entirely
- [03-04] WebDavWriter buffers writes to Vec<u8>, uploads via PUT on flush/drop
- [03-04] PROPFIND XML parsing uses string matching instead of full XML parser
- [03-04] Updated protocol_detection tests to accept any error since backends are now live
- [03-03] Windows SMB uses native UNC paths via std::fs -- no external dependencies (sambrs crate does not exist)
- [03-03] Non-Windows SMB returns ProtocolError directing to smb feature flag or OS mount
- [03-03] supports_parallel=false, supports_seek=false for SMB (network I/O not suitable for positional reads)
- [03-02] Strawberry Perl required for vendored-openssl build on Windows (MSYS2 perl missing modules)
- [03-02] unsafe Send+Sync impl for SftpBackend (ssh2 types need explicit markers for FluxBackend trait)
- [03-02] SSH auth cascade: agent > key files (ed25519, rsa, ecdsa) > password > prompt
- [03-02] get_current_username via env vars (USERNAME/USER) instead of whoami crate
- [03-02] sftp_err converts ssh2::Error to FluxError::Io via Into<io::Error> trait
- [04-01] Alias resolution runs BEFORE detect_protocol in execute_copy, not inside parser
- [04-01] AliasStore::default() provides graceful degradation when config dir unavailable
- [04-01] Aliases stored in separate aliases.toml (not config.toml) for independent editing
- [04-01] Atomic writes via temp file + rename for crash safety on alias save
- [04-01] Single-char alias names rejected to prevent drive letter collision (C:)
- [04-01] Reserved URL scheme names (sftp, ssh, smb, etc.) rejected at add time
- [04-02] Config loaded lazily inside execute_copy, not at CLI startup (keeps --help fast)
- [04-02] CLI flags override config.toml via Option<T>.unwrap_or(config.field)
- [04-02] Ask conflict strategy falls back to Skip when stdin is not a TTY
- [04-02] find_unique_name uses sequential numbering (file_1.txt) up to 9999, then timestamp fallback
- [04-02] Dry-run shares full validation pipeline, only skips actual I/O
- [04-02] Retry uses exponential backoff (delay * 2^attempt) via thread::sleep
- [04-02] Used std::io::IsTerminal instead of atty crate for TTY detection
- [04-03] QueueStore uses incremental u64 IDs (max + 1 on reload) for simplicity over UUIDs
- [04-03] Corrupted queue.json silently starts fresh (graceful degradation, not fatal)
- [04-03] State transitions idempotent where safe (pause already-paused is OK)
- [04-03] Queue run builds CpArgs from entry fields, delegates to existing execute_copy
- [04-03] flux queue with no subcommand defaults to list (matches alias pattern)
- [04-04] History recording is best-effort: errors silently ignored so transfer success unaffected
- [04-04] FLUX_CONFIG_DIR/FLUX_DATA_DIR env vars override default dirs for test isolation
- [04-04] History cap removes oldest entries when limit exceeded (FIFO), default 1000
- [04-04] Corrupted history.json silently starts fresh (matches QueueStore/AliasStore pattern)
- [04-04] Shell completions use clap_complete::generate() writing to stdout
- [04-04] format_bytes uses bytesize crate for human-readable size display
- [05-01] Manual Debug impl for DeviceIdentity to redact secret key (StaticSecret has no Debug)
- [05-01] AeadCore trait import required for XChaCha20Poly1305::generate_nonce
- [05-01] TrustStore compares public keys as base64 strings, matching storage format
- [05-01] Corrupted trusted_devices.json silently starts fresh (matches existing graceful degradation pattern)
- [05-02] bincode 2.x serde API (bincode::serde::encode_to_vec) for FluxMessage serialization with standard config
- [05-02] mdns-sd ServiceDaemon is sync (internal thread) -- no async needed for discovery
- [05-02] Device name sanitized to DNS label spec: replace non-alphanumeric with hyphens, collapse, truncate 63 chars
- [05-02] FluxMessage uses Vec<u8> for public_key/nonce fields (serde-compatible, flexible length)
- [05-02] Discovery prefers IPv4 addresses, deduplicates by instance name (first seen wins)
- [05-03] tokio_util::bytes::Bytes re-export used instead of adding bytes crate directly
- [05-03] Receiver auto-trusts unknown devices in v1 (future: interactive prompt)
- [05-03] SSH-style WARNING on key change with connection rejection
- [05-03] find_unique_path for auto-rename on receiver (file_1.txt pattern)
- [05-03] Default trust action is 'list' (matches alias/queue pattern)
- [05-03] gethostname() as default device name for send/receive

### Pending Todos

None yet.

### Blockers/Concerns

- [RESOLVED] ssh2 vendored-openssl build: Fixed by installing Strawberry Perl via winget. Builds require `PATH="/c/Strawberry/perl/bin:$PATH"` before cargo commands.

## Session Continuity

Last session: 2026-02-17
Stopped at: Completed 05-03-PLAN.md (Send/Receive protocol + CLI integration -- 274 unit + 77 integration = 351 tests passing, 19 new tests)
Resume file: .planning/phases/05-discovery-security/05-03-SUMMARY.md

---
*State initialized: 2026-02-16*
