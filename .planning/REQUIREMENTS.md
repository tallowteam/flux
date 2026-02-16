# Requirements: Flux

**Defined:** 2026-02-16
**Core Value:** Transfer files at maximum network speed with zero friction

## v1 Requirements

Requirements for initial release. Each maps to roadmap phases.

### Core Transfer (CORE)

- [ ] **CORE-01**: User can copy single file from source to destination path
- [ ] **CORE-02**: User can copy directory recursively preserving structure
- [ ] **CORE-03**: User can resume interrupted transfer from where it stopped
- [ ] **CORE-04**: User can see real-time progress (percentage, speed, ETA, bytes)
- [ ] **CORE-05**: User can verify transfer integrity via checksum (BLAKE3)
- [ ] **CORE-06**: User can exclude files/folders using glob patterns
- [ ] **CORE-07**: User can include only matching files using glob patterns
- [ ] **CORE-08**: User can enable compression for transfers (zstd)
- [ ] **CORE-09**: Tool works on Windows, Linux, and macOS

### Performance (PERF)

- [ ] **PERF-01**: User can transfer files using parallel chunks (configurable count)
- [ ] **PERF-02**: User can transfer large files using multiple TCP connections
- [ ] **PERF-03**: User can limit bandwidth usage (KB/s, MB/s)
- [ ] **PERF-04**: Tool auto-detects optimal chunk size based on file size and network

### Protocol Support (PROT)

- [ ] **PROT-01**: User can transfer to/from local filesystem paths
- [ ] **PROT-02**: User can transfer to/from SMB/CIFS network shares
- [ ] **PROT-03**: User can transfer to/from SFTP servers
- [ ] **PROT-04**: User can transfer to/from WebDAV endpoints
- [ ] **PROT-05**: Tool auto-detects protocol from path format (no explicit flags needed)

### Path Management (PATH)

- [ ] **PATH-01**: User can save named path aliases (`flux add nas \\server\share`)
- [ ] **PATH-02**: User can use aliases in commands (`flux cp file.txt nas:`)
- [ ] **PATH-03**: User can set default destination for quick sends
- [ ] **PATH-04**: User can view and reuse path history (`flux history`)
- [ ] **PATH-05**: User can list and remove saved aliases

### Discovery (DISC)

- [ ] **DISC-01**: User can discover devices on LAN via mDNS/Bonjour
- [ ] **DISC-02**: User can see discovered devices with friendly names
- [ ] **DISC-03**: Tool can receive transfers from other Flux instances

### Queue & State (QUEUE)

- [ ] **QUEUE-01**: User can queue multiple transfers
- [ ] **QUEUE-02**: User can view queue status
- [ ] **QUEUE-03**: User can pause/resume individual transfers
- [ ] **QUEUE-04**: User can cancel transfers
- [ ] **QUEUE-05**: User can view transfer history/logs

### Configuration (CONF)

- [ ] **CONF-01**: User can configure conflict handling (overwrite/skip/rename/ask)
- [ ] **CONF-02**: User can configure failure handling (retry/pause/skip)
- [ ] **CONF-03**: User can configure retry count and backoff
- [ ] **CONF-04**: User can set verbosity level (quiet/normal/verbose)
- [ ] **CONF-05**: User can preview operations with dry-run mode

### Security (SEC)

- [ ] **SEC-01**: User can enable optional end-to-end encryption
- [ ] **SEC-02**: Tool uses trust-on-first-use for device authentication
- [ ] **SEC-03**: User can view and manage trusted devices

### CLI Interface (CLI)

- [ ] **CLI-01**: User can run simple commands (`flux cp src dest`)
- [ ] **CLI-02**: User can run sync commands (`flux sync src dest`)
- [ ] **CLI-03**: User can run send/receive commands (`flux send file`, `flux receive`)
- [ ] **CLI-04**: Tool provides helpful error messages with suggested fixes
- [ ] **CLI-05**: Tool supports shell completion (bash, zsh, fish, powershell)

### TUI Mode (TUI)

- [ ] **TUI-01**: User can launch interactive TUI mode (`flux --tui` or `flux ui`)
- [ ] **TUI-02**: User can see real-time transfer dashboard with graphs
- [ ] **TUI-03**: User can browse and select files in TUI
- [ ] **TUI-04**: User can manage queue from TUI
- [ ] **TUI-05**: User can switch between transfers view and history view

### Sync Mode (SYNC)

- [ ] **SYNC-01**: User can sync directories (one-way mirror)
- [ ] **SYNC-02**: User can preview sync changes before applying (dry-run)
- [ ] **SYNC-03**: User can schedule recurring syncs
- [ ] **SYNC-04**: User can enable watch mode for continuous sync

## v2 Requirements

Deferred to future release. Tracked but not in current roadmap.

### Advanced Sync

- **SYNC-V2-01**: Delta/incremental transfers (only changed blocks)
- **SYNC-V2-02**: Bidirectional sync with conflict resolution
- **SYNC-V2-03**: File versioning (keep old versions on change/delete)

### Advanced Security

- **SEC-V2-01**: PAKE encryption with human-readable code phrases
- **SEC-V2-02**: QR code pairing for device trust

### Advanced Features

- **ADV-01**: Plugin/extension system
- **ADV-02**: Web dashboard for headless servers

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Cloud storage backends (S3, GDrive) | rclone does this excellently; use rclone for cloud |
| GUI application | CLI/TUI only; community can build GUI wrappers |
| Mobile apps (Android/iOS) | Desktop platforms only for v1 |
| FTP protocol | Legacy; use rclone for FTP |
| Email/notification integrations | Use exit codes + JSON output for scripting |
| Encryption-at-rest | Focus on transfer; use OS disk encryption |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| CORE-01 | Phase 1 | Pending |
| CORE-02 | Phase 1 | Pending |
| CORE-03 | Phase 2 | Pending |
| CORE-04 | Phase 1 | Pending |
| CORE-05 | Phase 2 | Pending |
| CORE-06 | Phase 1 | Pending |
| CORE-07 | Phase 1 | Pending |
| CORE-08 | Phase 2 | Pending |
| CORE-09 | Phase 1 | Pending |
| PERF-01 | Phase 2 | Pending |
| PERF-02 | Phase 2 | Pending |
| PERF-03 | Phase 2 | Pending |
| PERF-04 | Phase 2 | Pending |
| PROT-01 | Phase 1 | Pending |
| PROT-02 | Phase 3 | Pending |
| PROT-03 | Phase 3 | Pending |
| PROT-04 | Phase 3 | Pending |
| PROT-05 | Phase 3 | Pending |
| PATH-01 | Phase 4 | Pending |
| PATH-02 | Phase 4 | Pending |
| PATH-03 | Phase 4 | Pending |
| PATH-04 | Phase 4 | Pending |
| PATH-05 | Phase 4 | Pending |
| DISC-01 | Phase 5 | Pending |
| DISC-02 | Phase 5 | Pending |
| DISC-03 | Phase 5 | Pending |
| QUEUE-01 | Phase 4 | Pending |
| QUEUE-02 | Phase 4 | Pending |
| QUEUE-03 | Phase 4 | Pending |
| QUEUE-04 | Phase 4 | Pending |
| QUEUE-05 | Phase 4 | Pending |
| CONF-01 | Phase 4 | Pending |
| CONF-02 | Phase 4 | Pending |
| CONF-03 | Phase 4 | Pending |
| CONF-04 | Phase 1 | Pending |
| CONF-05 | Phase 4 | Pending |
| SEC-01 | Phase 5 | Pending |
| SEC-02 | Phase 5 | Pending |
| SEC-03 | Phase 5 | Pending |
| CLI-01 | Phase 1 | Pending |
| CLI-02 | Phase 6 | Pending |
| CLI-03 | Phase 5 | Pending |
| CLI-04 | Phase 1 | Pending |
| CLI-05 | Phase 4 | Pending |
| TUI-01 | Phase 6 | Pending |
| TUI-02 | Phase 6 | Pending |
| TUI-03 | Phase 6 | Pending |
| TUI-04 | Phase 6 | Pending |
| TUI-05 | Phase 6 | Pending |
| SYNC-01 | Phase 7 | Pending |
| SYNC-02 | Phase 7 | Pending |
| SYNC-03 | Phase 7 | Pending |
| SYNC-04 | Phase 7 | Pending |

**Coverage:**
- v1 requirements: 53 total
- Mapped to phases: 53
- Unmapped: 0

---
*Requirements defined: 2026-02-16*
*Last updated: 2026-02-16 after roadmap creation*
