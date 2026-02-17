# Project State

## Project Reference

See: .planning/PROJECT.md (updated 2026-02-16)

**Core value:** Transfer files at maximum network speed with zero friction
**Current focus:** Phase 3 - Network Protocols

## Current Position

Phase: 3 of 7 (Network Protocols)
Plan: 4 of 4 in current phase
Status: In Progress
Last activity: 2026-02-17 -- Completed 03-02-PLAN.md (SFTP backend)

Progress: [##########] 43%

## Performance Metrics

**Velocity:**
- Total plans completed: 10
- Average duration: 15min
- Total execution time: 2.6 hours

**By Phase:**

| Phase | Plans | Total | Avg/Plan |
|-------|-------|-------|----------|
| 01-foundation | 3/3 | 73min | 24min |
| 02-performance | 3/3 | 22min | 7min |
| 03-network-protocols | 4/4 | 55min | 14min |

**Recent Trend:**
- Last 5 plans: 02-03 (7min), 03-01 (7min), 03-04 (12min), 03-03 (12min), 03-02 (24min)
- Trend: SFTP took 24min due to Strawberry Perl installation for vendored-openssl build

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

### Pending Todos

None yet.

### Blockers/Concerns

- [RESOLVED] ssh2 vendored-openssl build: Fixed by installing Strawberry Perl via winget. Builds require `PATH="/c/Strawberry/perl/bin:$PATH"` before cargo commands.

## Session Continuity

Last session: 2026-02-17
Stopped at: Completed 03-02-PLAN.md (SFTP backend with ssh2, 201 tests passing, Phase 3 all 4 plans done)
Resume file: .planning/phases/03-network-protocols/03-02-SUMMARY.md

---
*State initialized: 2026-02-16*
