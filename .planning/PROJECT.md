# Flux

## What This Is

A blazing-fast CLI file transfer tool built in Rust. Transfers files between local drives, network shares (SMB), SFTP servers, and WebDAV endpoints at maximum network speed. Cross-platform (Windows/Linux/macOS). Simple commands for quick transfers, optional TUI mode for real-time dashboards. Beats Windows Explorer and basic cp/rsync through parallel chunking, multi-connection streaming, and intelligent compression.

## Core Value

Transfer files at maximum network speed with zero friction — paste any path, it just works.

## Requirements

### Validated

(None yet — ship to validate)

### Active

- [ ] Fast transfer engine with parallel chunks and multi-connection streaming
- [ ] CLI commands (rsync-style: `flux cp src dest`)
- [ ] TUI mode with real-time speed, ETA, progress visualization
- [ ] Auto-detect protocol from path format (SMB, SFTP, WebDAV, local)
- [ ] Named path aliases (`flux add nas \\server\share`)
- [ ] Default source/destination configuration
- [ ] Path history with reuse (`flux history`, `flux cp !3`)
- [ ] mDNS/Bonjour device discovery on LAN
- [ ] Resume interrupted transfers
- [ ] Checksum verification for integrity
- [ ] Transfer queue management
- [ ] Configurable conflict handling (overwrite/skip/rename/ask)
- [ ] Configurable failure handling (retry/pause/skip)
- [ ] Smart compression (on for text, off for media)
- [ ] Optional end-to-end encryption
- [ ] Trust-on-first-use device authentication
- [ ] Scheduled transfers (cron-like)
- [ ] Bandwidth limiting
- [ ] Transfer history and logs
- [ ] Sync mode (continuous folder mirroring)
- [ ] Cross-platform: Windows, Linux, macOS

### Out of Scope

- GUI application — CLI/TUI only
- Mobile (Android/iOS) — desktop platforms only
- Cloud storage APIs (Dropbox, Google Drive, S3) — focus on direct transfers
- Web interface — terminal only

## Context

User is frustrated with Windows Explorer's slow file copy speeds, especially over network to NAS/servers. Existing tools like robocopy and rsync exist but lack modern UX (TUI, smart defaults, path aliases). The goal is a tool that "just works" — paste any path format and transfer at wire speed.

Target use cases:
- PC to NAS (Synology at 192.168.4.3)
- PC to PC on same network
- Local to external drives
- SFTP to remote servers

Network environments vary: gigabit wired, WiFi 6, mixed — tool should adapt and let user configure.

## Constraints

- **Tech stack**: Rust — non-negotiable for performance and cross-platform native binaries
- **Interface**: Terminal only (CLI + TUI) — no Electron, no web UI
- **Protocols**: Must support SMB/CIFS, SFTP/SCP, WebDAV, and local filesystem
- **Platforms**: Windows, Linux, macOS — all three from v1

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| Rust + no GUI framework | Maximum transfer speed, small binary, true cross-platform | — Pending |
| Trust-on-first-use auth | Simple UX like SSH, no pairing codes needed | — Pending |
| TUI optional, not default | Keep CLI fast for scripts, TUI for interactive use | — Pending |
| Auto-detect protocol | User shouldn't think about SMB vs SFTP, just paste path | — Pending |
| Named aliases over config files | `flux add` is faster than editing YAML | — Pending |

---
*Last updated: 2026-02-16 after initialization*
