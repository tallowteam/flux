# Project Research Summary

**Project:** Flux - Blazing-fast CLI File Transfer Tool
**Domain:** Cross-platform multi-protocol file transfer (SMB, SFTP, WebDAV, local)
**Researched:** 2026-02-16
**Confidence:** HIGH

## Executive Summary

Flux is entering a fragmented market where no single tool combines parallel chunked transfers, delta sync, strong encryption, and modern UX. rsync dominates server-to-server sync but caps at ~350 MB/sec due to single-threaded architecture. rclone addresses parallelization but lacks delta-transfer, making incremental backups inefficient. Peer-to-peer tools like croc excel at one-time transfers but lack sync capabilities. The opportunity is clear: build a unified tool that delivers parallel performance, multi-protocol support via a clean abstraction layer, and modern TUI experience.

The recommended approach uses a **layered architecture** with the remotefs ecosystem providing protocol abstraction (same API for SFTP, SMB, WebDAV, local), Tokio for async networking, and Rayon for CPU-bound work (compression, checksums). The critical architectural decision is the `FluxBackend` trait that isolates protocol-specific logic, allowing the transfer engine to operate identically regardless of backend. This pattern is proven in production by termscp.

Key risks center on **race conditions in parallel transfers** (silent file corruption), **cross-platform path handling** (Windows vs Unix semantics), and **destructive sync operations** (accidental data loss with `--delete`). These must be designed into the architecture from day one, not bolted on later. The Tokio file I/O gotcha (spawn_blocking overhead) requires explicit batching strategy to avoid performance degradation.

## Key Findings

### Recommended Stack

The 2025/2026 Rust ecosystem provides mature, battle-tested libraries for every component. The remotefs ecosystem is the linchpin, enabling unified file operations across protocols without protocol-specific code in the transfer engine.

**Core technologies:**
- **tokio 1.49.x**: Async runtime - industry standard with LTS releases, best networking support
- **remotefs + remotefs-ssh/smb/webdav**: Protocol abstraction - unified trait across all protocols, proven by termscp
- **clap 4.5.x**: CLI parsing - derive API for type-safe arguments, shell completions
- **ratatui 0.30.x + crossterm 0.28.x**: TUI framework - immediate-mode rendering, async-friendly
- **indicatif 0.18.x**: Progress bars - thread-safe, MultiProgress for parallel transfers
- **rayon 1.11.x**: CPU parallelism - for compression, checksums (not Tokio spawn_blocking)
- **zstd 0.13.x**: Compression - best speed/ratio at level 3-5 (~100MB/s compress)
- **mdns-sd 0.13.x**: Service discovery - pure Rust mDNS for LAN peer discovery
- **thiserror + anyhow**: Error handling - thiserror for library errors, anyhow for application context
- **tracing 0.1.x**: Logging - async-aware structured logging

**Critical version requirements:**
- Tokio LTS 1.43.x+ for stability
- russh 0.54.x (pure Rust SSH, required by remotefs-ssh)
- pavao 0.2.x (SMB, wraps libsmbclient with vendored build option)

### Expected Features

**Must have (table stakes):**
- Resume interrupted transfers - all competitors support this
- Progress display with ETA, speed, percentage
- Recursive directory transfer
- Exclude/include patterns (glob-based, .gitignore compatible)
- Basic compression (zstd level 3-5)
- Integrity verification (checksums during transfer)
- Cross-platform (Windows, Linux, macOS)
- Bandwidth limiting
- Dry run preview

**Should have (competitive differentiators):**
- Parallel chunked transfers - rsync caps at 350 MB/sec, rclone achieves 4x via parallelism
- Multi-connection for large files - overcomes per-connection throughput limits
- TUI mode - interactive file browsing like termscp
- mDNS discovery - LocalSend-style automatic peer discovery
- E2E encryption with PAKE - croc/magic-wormhole style code phrases
- Path aliases/bookmarks

**Defer (v2+):**
- Bidirectional sync - conflict resolution complexity, high risk of data loss bugs
- Cloud storage backends - rclone already does this well
- GUI application - TUI is sufficient, let community build wrappers
- Delta/incremental sync - significant implementation complexity

### Architecture Approach

The architecture follows a five-layer design: **Protocol Backend Layer** (FluxBackend trait + implementations), **Transfer Engine** (chunk scheduler, worker pool, progress aggregator), **Queue Manager** (job queue, persistence), **State/Resume Manager** (checkpoints, manifests), and **UI Layer** (CLI mode, TUI mode). The critical insight is that all protocol-specific logic lives behind the FluxBackend trait - the transfer engine never contains `if protocol == SMB` conditionals.

**Major components:**
1. **FluxBackend trait** - unified interface for connect, list, stat, open_read, open_write, seek
2. **Transfer Engine** - orchestrates chunked parallel transfers with semaphore-based concurrency
3. **Progress Aggregator** - MPSC channel-based progress collection, decouples transfer from display
4. **State Manager** - checkpoint persistence enabling resume, stores chunk states and checksums
5. **Queue Manager** - job queue with priority, persistence, concurrency limiting

### Critical Pitfalls

1. **Destructive sync without safeguards** - Implement mandatory dry-run, threshold warnings (">X% files deleted"), soft-delete mode, and audit logging from day one. rsync's `--delete` is notorious for production data loss.

2. **Race conditions in parallel transfers** - Use single-writer-per-file principle, unique temp files per transfer (`file.tmp.{uuid}`), file locking, and post-transfer checksum verification. Silent corruption (correct size, wrong content) is the failure mode.

3. **Tokio async file I/O anti-patterns** - tokio::fs uses spawn_blocking internally, not true async. Batch operations, use BufWriter, always flush(), consider dedicated thread pool for file I/O.

4. **Cross-platform path handling** - Use std::path abstractions only, never string manipulation. Handle Unicode normalization (NFC vs NFD). Test edge-case filenames: spaces, Unicode, long paths, Windows reserved names.

5. **Resume logic corruption** - Store file identity metadata (size, mtime, ETag, checksum) and verify before resuming. Partial files should use `.partial` extension. SCP doesn't support resume; SFTP/HTTP Range does.

## Implications for Roadmap

Based on research, suggested phase structure:

### Phase 1: Foundation
**Rationale:** Everything else depends on these primitives - error types, config, data types, and the FluxBackend trait must be finalized before any implementation
**Delivers:** Core infrastructure: FluxError enum, FluxConfig struct, FileEntry/FileStat/TransferJob types, FluxBackend trait definition
**Addresses:** Cross-platform path handling (design it right from the start)
**Avoids:** Race conditions (synchronization primitives designed in), path fragility (proper abstractions)

### Phase 2: Local Backend + Single-File Transfer
**Rationale:** Local backend enables integration testing of higher layers without network complexity; proves parallel transfer model works
**Delivers:** Working file copy with parallel chunks, progress reporting, checksum verification
**Uses:** tokio, rayon (for checksums), indicatif
**Implements:** FluxBackend for local, Transfer Engine, Progress Aggregator, Chunk Scheduler
**Avoids:** Tokio file I/O anti-patterns (batch operations), checksum bottleneck (streaming verification)

### Phase 3: Network Protocol Backends
**Rationale:** Can be developed in parallel; each backend is independent behind FluxBackend trait
**Delivers:** SFTP, SMB, WebDAV support via remotefs ecosystem
**Uses:** remotefs-ssh, remotefs-smb, remotefs-webdav, pavao
**Avoids:** SMB version negotiation failures (test matrix), WebDAV stateless challenges (connection keep-alive)

### Phase 4: Queue, Resume, and State Management
**Rationale:** Requires working transfer engine to test against; enables long-running operations
**Delivers:** Job queue with persistence, resume interrupted transfers, checkpoint system
**Implements:** Queue Manager, State Manager, checkpoint persistence
**Avoids:** Resume logic corruption (verify file identity), resource exhaustion (connection pooling, handle budgets)

### Phase 5: CLI Polish
**Rationale:** User-facing quality pass; all features must exist before polishing CLI UX
**Delivers:** Full argument parsing, error messages, output formatting, dry-run preview
**Uses:** clap with derive API, anyhow for contextual errors
**Avoids:** Trailing slash confusion (normalize semantics), inadequate progress reporting

### Phase 6: TUI Mode
**Rationale:** Separate interface that can be developed in parallel with CLI polish; requires progress aggregator
**Delivers:** Interactive file browser, real-time progress, keyboard navigation
**Uses:** ratatui, crossterm
**Implements:** Immediate-mode rendering pattern, async event loop integration

### Phase 7: Sync Mode + Discovery
**Rationale:** Most complex feature; needs all infrastructure in place; highest risk
**Delivers:** Unidirectional sync, mDNS peer discovery, path aliases
**Uses:** mdns-sd for discovery
**Avoids:** Destructive delete without safeguards (mandatory dry-run, thresholds)

### Phase Ordering Rationale

- **Foundation first:** FluxBackend trait must be stable before any backend implementation; changing it later cascades through entire codebase
- **Local before network:** Proves architecture with simplest case; enables fast iteration without network flakiness
- **Transfer engine before queue:** Need working transfers to test queue behavior
- **CLI before TUI:** CLI is simpler, validates feature completeness before building interactive mode
- **Sync last:** Highest complexity, highest risk; needs robust transfer engine and state management

### Research Flags

Phases likely needing deeper research during planning:
- **Phase 3 (Protocol Backends):** SMB version negotiation edge cases, WebDAV proxy compatibility - test matrix needed
- **Phase 4 (Resume):** Protocol-specific resume behavior varies (SCP vs SFTP vs HTTP Range)
- **Phase 7 (Sync):** Conflict detection algorithms, delete safeguard thresholds - need user research

Phases with standard patterns (skip research-phase):
- **Phase 1 (Foundation):** Standard Rust patterns, well-documented
- **Phase 2 (Local + Transfer):** Tokio + Rayon integration is well-documented
- **Phase 5 (CLI):** clap derive API has extensive documentation and examples
- **Phase 6 (TUI):** ratatui documentation is excellent (1398 code snippets in Context7)

## Confidence Assessment

| Area | Confidence | Notes |
|------|------------|-------|
| Stack | HIGH | Context7 documentation (24K+ Tokio snippets), crates.io version verification, official docs |
| Features | HIGH | Verified against rsync, rclone, croc, LocalSend documentation and benchmarks |
| Architecture | HIGH | Based on rclone, hf_transfer, WinSCP patterns via DeepWiki analysis |
| Pitfalls | HIGH | Multiple verified sources: GitHub issues, official docs, production incident reports |

**Overall confidence:** HIGH

### Gaps to Address

- **SMB on Windows vs Linux:** libsmbclient dependency may have different behaviors; need platform-specific testing early
- **SCP support:** russh has open issue for SCP; SFTP preferred, SCP as fallback only if needed
- **Sparse file handling:** Not all protocols support sparse transfer; document limitations per backend
- **ACL/permission preservation:** Cross-platform semantics differ; need to define "best effort" scope

## Sources

### Primary (HIGH confidence)
- Context7 `/websites/rs_tokio_tokio` - 3805 code snippets, async patterns
- Context7 `/websites/ratatui_rs` - 1398 code snippets, TUI patterns
- Context7 `/websites/rs_clap` - 10324 code snippets, CLI parsing
- remotefs documentation (docs.rs) - protocol abstraction patterns
- Tokio official documentation - async runtime, channels, semaphores

### Secondary (MEDIUM confidence)
- [DeepWiki rclone architecture](https://deepwiki.com/rclone/rclone) - layered design patterns
- [DeepWiki hf_transfer](https://deepwiki.com/huggingface/hf_transfer) - Rust async chunked transfers
- [termscp blog](https://blog.veeso.dev/blog/en/a-journey-into-file-transfer-protocols-in-rust/) - remotefs design rationale
- [Jeff Geerling rsync vs rclone benchmark](https://www.jeffgeerling.com/blog/2025/4x-faster-network-file-sync-rclone-vs-rsync)

### Tertiary (LOW confidence)
- Various forum posts on timestamp handling, ACL semantics - need validation during implementation

---
*Research completed: 2026-02-16*
*Ready for roadmap: yes*
