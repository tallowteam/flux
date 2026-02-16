# Feature Landscape: CLI File Transfer Tools

**Domain:** Cross-platform CLI file transfer (Flux)
**Researched:** 2026-02-16
**Overall Confidence:** HIGH

## Executive Summary

Modern CLI file transfer tools span a spectrum from traditional sync utilities (rsync) to cloud-focused tools (rclone) to ad-hoc peer-to-peer transfers (croc, magic-wormhole). The market is fragmented: rsync dominates server-to-server sync but suffers from single-threaded bottlenecks (maxes at ~350 MB/sec regardless of network capacity). Rclone addresses parallelization but lacks delta-transfer, making it inefficient for incremental backups. Peer-to-peer tools like croc and magic-wormhole excel at one-time transfers with strong encryption but lack sync capabilities. **No single tool combines parallel chunked transfers, delta sync, strong encryption, and modern UX (TUI mode, mDNS discovery).**

---

## Table Stakes

Features users expect. Missing = product feels incomplete or users leave immediately.

| Feature | Why Expected | Complexity | Dependencies | Notes |
|---------|--------------|------------|--------------|-------|
| **Resume interrupted transfers** | All competitors (rsync, rclone, croc, wget) support this. Users transfer large files over unreliable connections. | Medium | Requires tracking transfer state, partial file handling | RFC 3659 (2007) standardized FTP resume; modern expectation |
| **Progress display** | rsync `--progress`, rclone, croc all show real-time transfer status. Silent transfers feel broken. | Low | None | Include: percentage, speed, ETA, bytes transferred |
| **Cross-platform support** | LocalSend, rclone, croc all work on Windows/Linux/macOS. Single-platform = instant disqualification. | Medium | Platform abstraction layer | Rust's std handles most; edge cases in permissions, symlinks |
| **Recursive directory transfer** | rsync `-r`, rclone, all tools support this. Users transfer projects, not just files. | Low | None | Must handle deeply nested structures efficiently |
| **Exclude/include patterns** | rsync `--exclude`, rclone `--filter`. Users need to skip `.git`, `node_modules`, build artifacts. | Medium | Pattern matching engine | Support globs, consider `.gitignore` compatibility |
| **Basic compression** | rsync `-z`, rclone `--transfers` with compression. Saves bandwidth on compressible data. | Low | Compression library (zstd recommended) | Use zstd: better ratio than gzip, faster than zlib |
| **Integrity verification** | rsync checksums, rclone verifies by default. Corrupted transfers = data loss. | Medium | Hashing (SHA-256 or BLAKE3) | Verify both during and after transfer |
| **Verbose/quiet modes** | rsync `-v`, `--quiet`. Different contexts need different verbosity. | Low | Logging infrastructure | Machine-parseable output for scripting |
| **Dry run (preview)** | rsync `-n`/`--dry-run`. Users need to preview before committing to large operations. | Low | None | Critical for preventing accidental deletions in sync mode |
| **Bandwidth limiting** | rsync `--bwlimit`, rclone `--bwlimit`. Users share connections, need to avoid saturating links. | Medium | Token bucket or leaky bucket algorithm | Support time-based schedules (e.g., "1M during work hours, unlimited nights") |
| **Error handling/retry** | rclone `--retries`, rsync reconnect. Networks fail; tools must recover gracefully. | Medium | Connection state management | Exponential backoff, configurable retry count |

---

## Differentiators

Features that set product apart. Not universally expected, but highly valued when present.

| Feature | Value Proposition | Complexity | Dependencies | Notes |
|---------|-------------------|------------|--------------|-------|
| **Parallel chunked transfers** | rsync's single-thread caps at ~350 MB/sec even on 10Gbps links. Rclone achieves 4x speedup via `--transfers` and `--multi-thread-streams`. Flux should parallelize from day one. | High | Async runtime, chunk coordination | mscp uses 64MB minimum chunk size; optimize based on file size and network RTT |
| **Multi-connection transfers** | Split large files across multiple TCP connections to overcome per-connection throughput limits (esp. for high-latency WANs). | High | Connection pooling, chunk reassembly | Distinct from multi-file parallelism; addresses single-large-file bottleneck |
| **Delta/incremental sync** | rsync's killer feature: only transfer changed blocks. Rclone lacks this, transferring whole files. Critical for backup workflows. | High | Rolling checksum (rsync algorithm), block-level diff | Significant implementation complexity; consider as Phase 2+ |
| **TUI mode** | termscp proves demand exists. Interactive file browsing, selection, real-time progress in rich terminal UI. | High | TUI library (ratatui in Rust) | Differentiator over rsync/rclone CLI-only; see yazi, ranger for UX patterns |
| **mDNS/Zeroconf discovery** | LocalSend's key feature: discover peers automatically on LAN. No IP addresses to exchange. | Medium | mDNS library (mdns-sd in Rust) | Enables "just works" LAN transfers without configuration |
| **Path aliases/bookmarks** | rclone's `alias` remote type. Power users frequently access same destinations. | Low | Config file management | Store named shortcuts: `flux send project1:` instead of full paths |
| **End-to-end encryption (PAKE)** | croc and magic-wormhole use PAKE for trustless encryption. No pre-shared keys needed. | High | PAKE library (SPAKE2), TLS | Differentiator for peer-to-peer mode; code phrase generation |
| **Human-readable code phrases** | croc/magic-wormhole use word lists (e.g., "7-guitarist-revenge"). Easier than exchanging IPs/ports. | Low | Word list, code generation | Memorable, typeable, tab-completable |
| **Real-time/watch mode** | Syncthing, lsyncd use inotify/FSEvents to sync on file changes. Continuous sync without polling. | High | Platform-specific file watchers | Linux: inotify; macOS: FSEvents; Windows: ReadDirectoryChangesW |
| **File versioning** | Syncthing keeps old versions on deletion/modification. Safety net for sync mode. | Medium | Version storage strategy | Consider: trash can, staggered (hourly/daily/weekly), external command |
| **Scheduling (built-in)** | Users currently combine rsync+cron. Built-in scheduler removes external dependency. | Medium | Scheduler/timer infrastructure | Include: one-time, recurring, bandwidth-aware schedules |
| **Conflict resolution** | Syncthing renames conflicts with timestamp suffix. Essential for bidirectional sync. | Medium | Requires sync mode | Detection + resolution strategy (rename, newest wins, manual) |

---

## Anti-Features

Features to explicitly NOT build. Adds complexity without proportional value, or conflicts with project goals.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| **Cloud storage backends** | rclone supports 70+ backends (S3, GDrive, etc.); enormous maintenance burden. Flux should focus on direct machine-to-machine transfers. | Integrate with rclone via subprocess for cloud needs, or document as out-of-scope |
| **GUI application** | Significant additional effort (cross-platform GUI frameworks are painful). TUI mode provides rich interaction while staying terminal-native. | Build excellent TUI; let third parties build GUIs (like crock for croc) |
| **Web interface** | Security surface area, additional dependencies (HTTP server, web framework). Syncthing has one but it's a separate concern. | TUI mode or headless daemon with socket control |
| **Bidirectional sync (initial release)** | Conflict resolution is hard. Syncthing spent years on this. High risk of data loss bugs. | Start with unidirectional (push/pull); add bidirectional as mature feature later |
| **FTP/SFTP protocol support** | Different protocols = different implementations. rsync/rclone already do this well. | Native Flux protocol only; recommend rclone for legacy protocols |
| **Windows share (SMB) mounting** | Platform-specific complexity; OS already handles this. | Transfer to/from mounted paths using OS-level mounts |
| **Email/notification integrations** | Scope creep; users can wrap Flux in scripts for notifications. | Exit codes + structured output (JSON) for scripting integration |
| **Encryption-at-rest for local files** | Out of scope for transfer tool; use OS disk encryption or dedicated tools. | Focus on encryption-in-transit only |
| **Plugin/extension system** | Premature abstraction; determine actual extension needs first through usage. | Build monolithic initially; extract extension points based on real demand |

---

## Feature Dependencies

```
Resume transfers ─────────────────────────┐
                                          ├─► Parallel chunks (chunks need tracking)
Integrity verification ───────────────────┘

Parallel chunks ──────────────────────────┬─► Multi-connection (extends parallelism)
                                          └─► Delta sync (chunks are diff units)

Progress display ─────────────────────────┬─► TUI mode (progress is TUI component)
                                          └─► Bandwidth limiting (needs throughput tracking)

mDNS discovery ───────────────────────────┬─► Path aliases (discovered peers as aliases)
                                          └─► Code phrases (alternative to mDNS for remote)

Sync mode (basic) ────────────────────────┬─► Watch mode (continuous sync)
                                          ├─► Scheduling (periodic sync)
                                          ├─► File versioning (safety for deletions)
                                          └─► Conflict resolution (bidirectional sync)

Exclude patterns ─────────────────────────►─► Sync mode (what to include/exclude)

E2E encryption ───────────────────────────►─► Code phrases (PAKE needs shared secret)
```

### Dependency Summary

| Feature | Blocked By | Blocks |
|---------|-----------|--------|
| Parallel chunks | Progress display, Resume | Multi-connection, Delta sync |
| TUI mode | Progress display | None |
| mDNS discovery | None | Path aliases (optionally) |
| Sync mode | Exclude patterns | Watch mode, Scheduling, Versioning, Conflicts |
| E2E encryption | None | Code phrases |
| Delta sync | Parallel chunks | None (advanced feature) |
| Watch mode | Sync mode | None |
| Bidirectional sync | Conflict resolution, Versioning | None |

---

## MVP Recommendation

**Phase 1: Core Transfer (Table Stakes)**

1. Single-file and recursive directory transfer
2. Progress display with ETA
3. Resume interrupted transfers
4. Exclude/include patterns (glob-based)
5. Basic compression (zstd)
6. Integrity verification (checksums)
7. Cross-platform (Windows, Linux, macOS)
8. Verbose/quiet modes
9. Bandwidth limiting

**Phase 2: Performance Differentiators**

1. Parallel chunked transfers
2. Multi-connection for large files
3. TUI mode (interactive)
4. mDNS discovery for LAN
5. Path aliases/bookmarks

**Phase 3: Sync & Security**

1. Sync mode (unidirectional)
2. Delta/incremental transfers
3. Dry run preview
4. End-to-end encryption (PAKE)
5. Human-readable code phrases

**Phase 4: Advanced Sync**

1. Watch mode (real-time)
2. Scheduling
3. File versioning
4. Conflict detection (not full bidirectional)

**Defer Indefinitely:**
- Bidirectional sync: High complexity, high risk; leave for v2.0+
- Cloud backends: Use rclone; don't reinvent
- GUI: Let community build wrappers

---

## Complexity Estimates

| Complexity | Features |
|------------|----------|
| **Low** | Progress display, verbose modes, exclude patterns, path aliases, dry run, code phrases |
| **Medium** | Resume transfers, compression, bandwidth limiting, checksums, mDNS discovery, scheduling, file versioning, conflict detection |
| **High** | Parallel chunks, multi-connection, TUI mode, delta sync, watch mode, E2E encryption (PAKE), bidirectional sync |

---

## Sources

### Primary Comparisons
- [Rclone vs. Rsync - Pure Storage](https://blog.purestorage.com/purely-technical/rclone-vs-rsync/)
- [4x faster network file sync with rclone vs rsync - Jeff Geerling](https://www.jeffgeerling.com/blog/2025/4x-faster-network-file-sync-rclone-vs-rsync/)
- [rsync vs Rclone Comparison 2026 - Appmus](https://appmus.com/vs/rsync-vs-rclone)

### Tool Documentation
- [rsync man page](https://linux.die.net/man/1/rsync)
- [rclone documentation](https://rclone.org/docs/)
- [croc GitHub](https://github.com/schollz/croc)
- [magic-wormhole documentation](https://magic-wormhole.readthedocs.io/)
- [Syncthing documentation](https://docs.syncthing.net/)
- [LocalSend](https://localsend.org/)

### Technical Deep-Dives
- [Multi-threaded scp (mscp) - ACM](https://dl.acm.org/doi/fullHtml/10.1145/3569951.3597582)
- [Zstandard compression - Facebook Engineering](https://engineering.fb.com/2016/08/31/core-infra/smaller-and-faster-data-compression-with-zstandard/)
- [LZ4 vs Zstd - TrueNAS Community](https://www.truenas.com/community/threads/lz4-vs-zstd.89400/)
- [mDNS/Zeroconf - Wikipedia](https://en.wikipedia.org/wiki/Zero-configuration_networking)
- [LocalSend Network Discovery - DeepWiki](https://deepwiki.com/localsend/localsend/2.6-network-discovery)
- [termscp - GitHub](https://github.com/veeso/termscp)
- [Magic-Wormhole PAKE Analysis - Oreate AI](https://www.oreateai.com/blog/magicwormhole-an-analysis-of-a-secure-file-transfer-tool-based-on-pake-encryption/55d32f464fc9e83cb88134c0cc51fffe)

### Performance & Optimization
- [Block Level Sync - Cloudwards 2026](https://www.cloudwards.net/block-level-file-copying/)
- [Parallel rsync methods - Resilio](https://www.resilio.com/blog/parallel-rsync-methods-and-alternatives)
- [Compression algorithms comparison - LinuxReviews](https://linuxreviews.org/Comparison_of_Compression_Algorithms)

**Confidence Levels:**
- Table Stakes: HIGH (well-documented across all major tools)
- Differentiators: HIGH (verified against tool documentation and benchmarks)
- Anti-Features: MEDIUM (based on architectural judgment; may need revision based on user feedback)
