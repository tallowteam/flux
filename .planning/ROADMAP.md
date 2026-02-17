# Roadmap: Flux

## Overview

Flux delivers a blazing-fast CLI file transfer tool in Rust, progressing from a working local transfer engine through network protocol support, user experience polish, peer discovery, interactive TUI, and finally sync capabilities. Each phase builds on the previous, with the core FluxBackend trait abstraction established in Phase 1 enabling all subsequent protocol implementations to share the same transfer engine.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [ ] **Phase 1: Foundation** - CLI skeleton, core types, local backend, basic single-file transfer
- [ ] **Phase 2: Performance** - Parallel chunks, multi-connection, resume, compression, verification
- [ ] **Phase 3: Network Protocols** - SFTP, SMB, WebDAV backends via remotefs ecosystem
- [ ] **Phase 4: User Experience** - Path aliases, queue management, configuration, shell completion
- [ ] **Phase 5: Discovery & Security** - mDNS peer discovery, send/receive commands, encryption, trust
- [ ] **Phase 6: TUI Mode** - Interactive dashboard, file browser, queue management
- [ ] **Phase 7: Sync Mode** - One-way sync, watch mode, scheduling

## Phase Details

### Phase 1: Foundation
**Goal**: User can transfer files between local paths with real-time progress
**Depends on**: Nothing (first phase)
**Requirements**: CORE-01, CORE-02, CORE-04, CORE-06, CORE-07, CORE-09, PROT-01, CONF-04, CLI-01, CLI-04
**Success Criteria** (what must be TRUE):
  1. User can run `flux cp source.txt dest.txt` and file is copied correctly
  2. User can run `flux cp -r folder/ dest/` and directory structure is preserved
  3. User can see real-time progress (percentage, speed, ETA) during transfer
  4. User can exclude files with `--exclude "*.log"` glob patterns
  5. Helpful error messages appear when paths are invalid or access denied
**Plans**: 3 plans

Plans:
- [ ] 01-01-PLAN.md — Project scaffolding, CLI parsing, error types, config, verbosity
- [ ] 01-02-PLAN.md — FluxBackend trait, LocalBackend, single-file copy with progress
- [ ] 01-03-PLAN.md — Glob filtering, recursive directory copy, integration tests

### Phase 2: Performance
**Goal**: Transfers saturate network bandwidth through parallelization and smart compression
**Depends on**: Phase 1
**Requirements**: CORE-03, CORE-05, CORE-08, PERF-01, PERF-02, PERF-03, PERF-04
**Success Criteria** (what must be TRUE):
  1. User can transfer large files with parallel chunks (configurable via `--chunks N`)
  2. User can resume interrupted transfers without re-transferring completed portions
  3. User can verify transfer integrity with `--verify` flag (BLAKE3 checksum)
  4. User can enable compression with `--compress` for text-heavy transfers
  5. User can limit bandwidth with `--limit 10MB/s`
**Plans**: 3 plans

Plans:
- [ ] 02-01-PLAN.md — Dependencies, CLI flags, chunk types, positional I/O primitives
- [ ] 02-02-PLAN.md — Parallel chunked copy with rayon, BLAKE3 integrity verification
- [ ] 02-03-PLAN.md — Resume manifests, zstd compression, bandwidth throttling

### Phase 3: Network Protocols
**Goal**: User can transfer files to/from SFTP, SMB, and WebDAV endpoints using same commands
**Depends on**: Phase 2
**Requirements**: PROT-02, PROT-03, PROT-04, PROT-05
**Success Criteria** (what must be TRUE):
  1. User can transfer to SMB share with `flux cp file.txt \\server\share\`
  2. User can transfer to SFTP server with `flux cp file.txt sftp://user@host/path/`
  3. User can transfer to WebDAV endpoint with `flux cp file.txt https://server/webdav/`
  4. Tool auto-detects protocol from path format (no `--protocol` flag needed)
**Plans**: 4 plans

Plans:
- [ ] 03-01-PLAN.md — Protocol detection, CLI args String migration, backend factory
- [ ] 03-02-PLAN.md — SFTP backend with ssh2 crate
- [ ] 03-03-PLAN.md — SMB backend with platform-conditional support (Windows native + pavao)
- [ ] 03-04-PLAN.md — WebDAV backend with reqwest_dav sync-over-async bridge

### Phase 4: User Experience
**Goal**: User can manage paths, queues, and configuration for efficient repeated transfers
**Depends on**: Phase 3
**Requirements**: PATH-01, PATH-02, PATH-03, PATH-04, PATH-05, QUEUE-01, QUEUE-02, QUEUE-03, QUEUE-04, QUEUE-05, CONF-01, CONF-02, CONF-03, CONF-05, CLI-05
**Success Criteria** (what must be TRUE):
  1. User can save alias with `flux add nas \\server\share` and use with `flux cp file nas:`
  2. User can queue multiple transfers and view queue status with `flux queue`
  3. User can pause/resume/cancel individual transfers in queue
  4. User can configure conflict handling (overwrite/skip/rename/ask) in config or flags
  5. User can preview operations with `--dry-run` before executing
**Plans**: 4 plans

Plans:
- [ ] 04-01-PLAN.md — Path alias system: config dirs, AliasStore, alias CLI, alias resolution in transfers
- [ ] 04-02-PLAN.md — Configuration & conflict handling: FluxConfig, ConflictStrategy, FailureStrategy, dry-run
- [ ] 04-03-PLAN.md — Transfer queue: QueueStore, queue CLI (add/list/pause/resume/cancel/run)
- [ ] 04-04-PLAN.md — History, shell completions, and integration tests

### Phase 5: Discovery & Security
**Goal**: User can discover LAN devices and transfer securely with encryption
**Depends on**: Phase 4
**Requirements**: DISC-01, DISC-02, DISC-03, SEC-01, SEC-02, SEC-03, CLI-03
**Success Criteria** (what must be TRUE):
  1. User can discover devices with `flux discover` showing friendly names
  2. User can send file to discovered device with `flux send file.txt @devicename`
  3. User can receive transfers with `flux receive` (listens for incoming)
  4. User can enable encryption with `--encrypt` for sensitive transfers
  5. Trust-on-first-use authentication works (device remembered after first connection)
**Plans**: TBD

Plans:
- [ ] 05-01: TBD
- [ ] 05-02: TBD

### Phase 6: TUI Mode
**Goal**: User can interactively browse, select, and monitor transfers in a visual dashboard
**Depends on**: Phase 5
**Requirements**: TUI-01, TUI-02, TUI-03, TUI-04, TUI-05, CLI-02
**Success Criteria** (what must be TRUE):
  1. User can launch TUI with `flux --tui` or `flux ui`
  2. User can see real-time transfer progress with speed graphs
  3. User can browse local and remote files using keyboard navigation
  4. User can manage queue (pause/resume/cancel) from TUI
  5. User can switch between active transfers view and history view
**Plans**: TBD

Plans:
- [ ] 06-01: TBD
- [ ] 06-02: TBD

### Phase 7: Sync Mode
**Goal**: User can keep directories synchronized with one-way mirroring
**Depends on**: Phase 6
**Requirements**: SYNC-01, SYNC-02, SYNC-03, SYNC-04
**Success Criteria** (what must be TRUE):
  1. User can sync directories with `flux sync source/ dest/` (one-way mirror)
  2. User can preview sync changes with `flux sync --dry-run`
  3. User can schedule recurring syncs with cron-like syntax
  4. User can enable watch mode for continuous sync (`flux sync --watch`)
**Plans**: TBD

Plans:
- [ ] 07-01: TBD
- [ ] 07-02: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5 -> 6 -> 7

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Foundation | 0/3 | Planned | - |
| 2. Performance | 0/3 | Planned | - |
| 3. Network Protocols | 0/TBD | Not started | - |
| 4. User Experience | 0/4 | Planned | - |
| 5. Discovery & Security | 0/TBD | Not started | - |
| 6. TUI Mode | 0/TBD | Not started | - |
| 7. Sync Mode | 0/TBD | Not started | - |

---
*Roadmap created: 2026-02-16*
*Last updated: 2026-02-16*
