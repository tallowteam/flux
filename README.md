<p align="center">
  <br>
  <img src="assets/flux-logo.svg" alt="Flux" width="180">
  <br><br>
  <strong>Blazing-fast file transfer for the terminal.</strong>
  <br>
  <em>Paste any path — local, SFTP, SMB, WebDAV — it just works.</em>
  <br><br>
  <a href="https://github.com/tallowteam/flux/actions"><img src="https://img.shields.io/github/actions/workflow/status/tallowteam/flux/ci.yml?style=flat-square&label=CI" alt="CI"></a>
  <a href="https://github.com/tallowteam/flux/releases"><img src="https://img.shields.io/github/v/release/tallowteam/flux?style=flat-square&color=blue" alt="Release"></a>
  <a href="https://github.com/tallowteam/flux/blob/master/LICENSE"><img src="https://img.shields.io/badge/license-MIT-green?style=flat-square" alt="License"></a>
  <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/built%20with-Rust-E57324?style=flat-square&logo=rust&logoColor=white" alt="Built with Rust"></a>
</p>

---

<p align="center">
  <a href="#features">Features</a> &bull;
  <a href="#how-it-works">How It Works</a> &bull;
  <a href="#installation">Installation</a> &bull;
  <a href="#quick-start">Quick Start</a> &bull;
  <a href="#usage">Usage</a> &bull;
  <a href="#configuration">Configuration</a> &bull;
  <a href="#security">Security</a> &bull;
  <a href="#architecture">Architecture</a>
</p>

---

## Demo

```
$ flux cp video.mp4 sftp://nas@192.168.4.3/media/

  video.mp4
  ████████████████████████████████████████  100%
  2.4 GB / 2.4 GB  ·  847 MB/s  ·  00:02 elapsed

  ✓ Transfer complete  ·  BLAKE3 verified
```

```
$ flux discover

  Found 3 Flux devices on your network:

  NAME              HOST              PORT    VERSION
  gaming-pc         192.168.4.10      9741    0.1.0
  macbook-air       192.168.4.22      9741    0.1.0
  synology-nas      192.168.4.3       9741    0.1.0
```

```
$ flux send --encrypt project.zip @macbook-air

  → Connecting to macbook-air (192.168.4.22:9741)...
  → Key exchange: X25519 · Cipher: XChaCha20-Poly1305
  → Device trusted (first seen 2026-01-15)

  project.zip
  ████████████████████████████████████████  100%
  156 MB / 156 MB  ·  412 MB/s  ·  encrypted

  ✓ Sent to macbook-air
```

---

## Features

### Transfer Engine

- **Parallel chunked transfers** — splits large files across CPU cores for maximum throughput. Auto-tunes chunk count based on file size (2 chunks for 10 MB, up to 16 for 10 GB+), or set manually with `--chunks N`
- **Resume interrupted transfers** — crash mid-transfer? Run the same command with `--resume` and Flux picks up exactly where it left off, chunk by chunk, via JSON sidecar manifests
- **BLAKE3 integrity verification** — verify every byte arrived correctly with `--verify`. Supports whole-file and per-chunk checksums using the fastest cryptographic hash available
- **Zstandard compression** — enable `--compress` for text-heavy or repetitive data. Per-chunk compression means parallel decompression and chunk-level resume still work together
- **Bandwidth throttling** — limit transfer speed with `--limit 10MB/s` to keep your network usable during large transfers. Token-bucket algorithm with 2-second burst allowance

### Protocol Support

| Protocol | Syntax | Auth |
|----------|--------|------|
| **Local** | `/path/to/file` or `C:\path\to\file` | — |
| **SFTP** | `sftp://user@host/path` | SSH agent, key files, password |
| **SMB** | `\\server\share\path` | Windows credentials |
| **WebDAV** | `https://server/webdav/path` | Basic auth |

Flux **auto-detects** the protocol from the path — no flags or config needed. Paste a UNC path, an SFTP URI, or an HTTP URL and Flux routes it to the right backend automatically.

### Peer-to-Peer

- **Zero-config device discovery** — find other Flux instances on your LAN instantly via mDNS/Bonjour (`_flux._tcp.local.`)
- **Direct device-to-device sends** — `flux send file.zip @laptop` transfers directly over TCP, no intermediate server
- **End-to-end encryption** — optional `--encrypt` flag enables X25519 key exchange + XChaCha20-Poly1305 AEAD cipher. 192-bit random nonces, no counters needed
- **Trust-on-first-use (TOFU)** — like SSH: first connection saves the device key, subsequent connections verify it. Key changes trigger a warning

### Sync Mode

- **One-way directory sync** — `flux sync src/ dest/` mirrors source to destination, only transferring changed files (mtime + size comparison)
- **Watch mode** — `flux sync --watch src/ dest/` monitors for filesystem changes and syncs continuously with debounced 2-second batching
- **Scheduled sync** — `flux sync --schedule "*/5 * * * *" src/ dest/` runs sync on a cron schedule
- **Safe deletes** — `--delete` removes orphan files in dest, but refuses to wipe dest if source is empty (override with `--force`)
- **Rsync semantics** — trailing slash on source (`src/`) copies contents; no slash (`src`) copies the directory itself

### User Experience

- **Path aliases** — `flux add nas \\server\share` then `flux cp file.txt nas:backups/` — save frequently used paths and reference them by name
- **Transfer queue** — queue up multiple transfers with `flux queue add` and run them all at once with `flux queue run`. Pause, resume, and cancel individual jobs
- **Transfer history** — `flux history` shows your recent transfers with timestamps, sizes, speeds, and pass/fail status
- **Interactive TUI** — `flux ui` launches a full terminal dashboard built with [ratatui](https://ratatui.rs/), featuring a file browser, queue manager, and transfer history viewer
- **Shell completions** — `flux completions bash|zsh|fish|powershell` generates completions for your shell
- **Smart error messages** — every error includes context and suggestions. "Connection refused? Check that the target device is running `flux receive`"
- **Glob filtering** — `--exclude "*.log" --include "*.rs"` with full glob pattern support via the `globset` crate

---

## How It Works

### Architecture Overview

```
┌─────────────────────────────────────────────────────────────────┐
│                         flux CLI                                │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  ┌──────────────┐  │
│  │    cp     │  │   send   │  │   sync    │  │     ui       │  │
│  │  (copy)   │  │ (p2p tx) │  │ (mirror)  │  │   (ratatui)  │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  └──────────────┘  │
│       │              │              │                            │
│  ┌────▼──────────────▼──────────────▼────────────────────────┐  │
│  │                  Transfer Engine                          │  │
│  │  ┌─────────┐ ┌──────────┐ ┌────────┐ ┌────────────────┐  │  │
│  │  │ Chunker │ │ Compress │ │ Resume │ │ Verify (BLAKE3)│  │  │
│  │  └─────────┘ └──────────┘ └────────┘ └────────────────┘  │  │
│  │  ┌──────────────┐  ┌──────────────────────────────────┐   │  │
│  │  │   Throttle   │  │   Parallel I/O (rayon threads)   │   │  │
│  │  └──────────────┘  └──────────────────────────────────┘   │  │
│  └───────────────────────────┬───────────────────────────────┘  │
│                              │                                  │
│  ┌───────────────────────────▼───────────────────────────────┐  │
│  │                    FluxBackend Trait                       │  │
│  │                                                           │  │
│  │   ┌───────┐  ┌──────┐  ┌──────┐  ┌────────┐             │  │
│  │   │ Local │  │ SFTP │  │ SMB  │  │ WebDAV │             │  │
│  │   │std::fs│  │ ssh2 │  │ UNC  │  │reqwest │             │  │
│  │   └───────┘  └──────┘  └──────┘  └────────┘             │  │
│  └───────────────────────────────────────────────────────────┘  │
│                                                                 │
│  ┌──────────────────────┐  ┌─────────────────────────────────┐  │
│  │   Security Layer     │  │     Discovery Layer             │  │
│  │  X25519 + XChaCha20  │  │  mDNS/Bonjour + TOFU trust     │  │
│  └──────────────────────┘  └─────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

### The FluxBackend Trait

Every protocol implements the same trait — the transfer engine doesn't know or care whether it's writing to a local SSD or an SFTP server on another continent:

```rust
pub trait FluxBackend: Send + Sync {
    fn stat(&self, path: &str) -> Result<FileStat, FluxError>;
    fn list_dir(&self, path: &str) -> Result<Vec<FileEntry>, FluxError>;
    fn open_read(&self, path: &str) -> Result<Box<dyn Read + Send>, FluxError>;
    fn open_write(&self, path: &str) -> Result<Box<dyn Write + Send>, FluxError>;
    fn create_dir_all(&self, path: &str) -> Result<(), FluxError>;
    fn features(&self) -> BackendFeatures;
}
```

### Transfer Pipeline

When you run `flux cp large-file.bin dest/`, here's what happens:

```
1. Protocol Detection
   "sftp://user@host/file" → SFTP backend
   "\\server\share\file"   → SMB backend
   "/local/path"           → Local backend

2. Pre-flight
   ├── Check source exists, get file size
   ├── Resolve aliases ("nas:docs/" → "\\server\share\docs\")
   ├── Check for existing resume manifest
   └── Apply conflict strategy (overwrite/skip/rename/ask)

3. Transfer
   ├── Auto-detect chunk count based on file size
   ├── Spawn rayon thread pool for parallel chunks
   ├── Each chunk: read → [compress] → [throttle] → write
   ├── Per-chunk BLAKE3 checksums (if --verify)
   └── Update resume manifest after each chunk

4. Post-flight
   ├── Whole-file BLAKE3 verification (if --verify)
   ├── Clean up resume manifest on success
   └── Record to transfer history
```

### Encryption Flow

When `--encrypt` is enabled for peer-to-peer transfers:

```
Sender                              Receiver
  │                                    │
  │   1. Generate ephemeral X25519     │
  │      key pair                      │
  │                                    │
  │ ──── public key ──────────────────▶│
  │                                    │
  │◀──── public key ─────────────────  │  2. Generate ephemeral
  │                                    │     key pair
  │   3. Diffie-Hellman shared secret  │
  │      (both sides compute same key) │
  │                                    │
  │   4. XChaCha20-Poly1305 AEAD       │
  │ ══════ encrypted chunks ═════════▶ │
  │      (random 24-byte nonce each)   │
  │                                    │
  │   5. TOFU verification             │
  │      (trust store check)           │
```

### Sync Engine

```
flux sync --watch --delete src/ dest/

  ┌─────────────────────────────┐
  │   Filesystem Watcher        │  notify + debouncer (2s)
  │   (notify-debouncer-full)   │
  └──────────┬──────────────────┘
             │ change detected
  ┌──────────▼──────────────────┐
  │   Compute Sync Plan         │  Compare mtime + size
  │   (2s FAT32 tolerance)      │  with 2-second tolerance
  │                             │  for FAT32 timestamps
  │   Actions:                  │
  │   ├── Copy (new files)      │
  │   ├── Update (changed)      │
  │   ├── Delete (orphans)      │
  │   └── Skip (unchanged)     │
  └──────────┬──────────────────┘
             │
  ┌──────────▼──────────────────┐
  │   Execute via Transfer      │  Same engine as `flux cp`
  │   Engine                    │  (parallel, verified, etc.)
  └─────────────────────────────┘
```

---

## Installation

### From Source (Rust toolchain required)

```bash
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
```

The binary will be at `target/release/flux` (or `target\release\flux.exe` on Windows).

### Add to PATH

<details>
<summary><strong>Linux / macOS</strong></summary>

```bash
sudo cp target/release/flux /usr/local/bin/
```

Or add to your shell profile:
```bash
export PATH="$PATH:/path/to/flux/target/release"
```
</details>

<details>
<summary><strong>Windows</strong></summary>

```powershell
# Copy to a directory in your PATH, or add the build directory:
$env:PATH += ";C:\path\to\flux\target\release"

# To make permanent, add via System Properties > Environment Variables
```
</details>

### Shell Completions

```bash
# Bash
flux completions bash > ~/.local/share/bash-completion/completions/flux

# Zsh
flux completions zsh > ~/.zfunc/_flux

# Fish
flux completions fish > ~/.config/fish/completions/flux.fish

# PowerShell
flux completions powershell >> $PROFILE
```

---

## Quick Start

### Copy a file

```bash
flux cp document.pdf /mnt/backup/
```

### Copy a directory recursively

```bash
flux cp -r ./project/ /mnt/nas/projects/
```

### Transfer to a remote server

```bash
# SFTP
flux cp report.xlsx sftp://user@server/documents/

# SMB (Windows share)
flux cp report.xlsx '\\fileserver\shared\documents\'

# WebDAV
flux cp report.xlsx https://cloud.example.com/remote.php/webdav/documents/
```

### Send a file to another device on your network

```bash
# On the receiving machine:
flux receive

# On the sending machine:
flux send photo.jpg @laptop
```

### Sync two directories

```bash
flux sync ~/Documents/ /mnt/backup/documents/ --delete --verify
```

---

## Usage

### `flux cp` — Copy files and directories

```bash
# Basic copy
flux cp source.txt dest.txt

# Recursive with progress
flux cp -r ./src/ ./backup/

# With verification and compression
flux cp --verify --compress large-archive.tar.gz sftp://backup-server/archives/

# Parallel chunks (auto or manual)
flux cp --chunks 8 database.dump /mnt/fast-ssd/

# Resume an interrupted transfer
flux cp --resume big-file.iso /mnt/external/

# Bandwidth-limited transfer
flux cp --limit 50MB/s ./video/ nas:media/

# Dry run — see what would happen
flux cp -r --dry-run --exclude "*.log" --exclude "node_modules" ./project/ /backup/

# Conflict handling
flux cp -r --on-conflict rename ./downloads/ /mnt/archive/
```

### `flux send` / `flux receive` — Peer-to-peer transfers

```bash
# Discover devices on your network
flux discover

# Send with end-to-end encryption
flux send --encrypt secrets.zip @gaming-pc

# Receive into a specific directory
flux receive -o ~/Downloads/ --encrypt

# Advertise with a custom device name
flux receive --name "work-laptop" --encrypt
```

### `flux sync` — One-way directory sync

```bash
# One-time sync
flux sync ~/projects/ /mnt/nas/projects/

# Watch for changes and sync continuously
flux sync --watch ~/Documents/ /mnt/backup/docs/

# Scheduled sync (every 5 minutes)
flux sync --schedule "*/5 * * * *" ~/work/ sftp://server/backup/

# Sync with deletion of orphan files
flux sync --delete --verify src/ dest/

# Preview changes without executing
flux sync --dry-run --delete src/ dest/

# Exclude patterns
flux sync --exclude "*.tmp" --exclude ".git" src/ dest/
```

### `flux add` / `flux alias` — Path aliases

```bash
# Save an alias
flux add nas '\\synology\shared'
flux add server sftp://deploy@prod.example.com/var/www

# Use aliases in any command
flux cp -r ./dist/ server:releases/v2.0/
flux sync ~/photos/ nas:photos/ --watch

# List all aliases
flux alias

# Remove an alias
flux alias rm old-server
```

### `flux queue` — Transfer queue

```bash
# Queue multiple transfers
flux queue add ./file1.zip /backup/ --verify
flux queue add -r ./project/ sftp://server/projects/ --compress
flux queue add ./data.csv nas:imports/

# List queued transfers
flux queue

# Run all pending transfers
flux queue run

# Manage individual jobs
flux queue pause 3
flux queue resume 3
flux queue cancel 5

# Clean up finished entries
flux queue clear
```

### `flux history` — Transfer history

```bash
# Show recent transfers
flux history

# Show last 50
flux history -n 50

# Clear history
flux history --clear
```

### `flux trust` — Device trust management

```bash
# List trusted devices
flux trust

# Remove a device
flux trust rm old-laptop
```

### `flux ui` — Interactive TUI

```bash
flux ui
```

Launches a full-screen terminal interface with four tabs:

| Tab | Key | Description |
|-----|-----|-------------|
| Dashboard | `1` | Active transfer status with speed sparkline |
| File Browser | `2` | Navigate directories, select files for transfer |
| Queue | `3` | View and manage transfer queue (p/r/c to pause/resume/cancel) |
| History | `4` | Browse transfer history |

Press `q` or `Esc` to exit. `Tab` to switch tabs. Arrow keys to navigate.

---

## Configuration

### Config File

**Location:** `~/.config/flux/config.toml` (Linux/macOS) or `%APPDATA%\flux\config.toml` (Windows)

```toml
# Verbosity: quiet, normal, verbose, trace
verbosity = "normal"

# What to do when destination file exists: overwrite, skip, rename, ask
conflict = "ask"

# What to do when a transfer fails: retry, skip, pause
failure = "retry"

# Number of retry attempts (for failure = "retry")
retry_count = 3

# Initial retry delay in milliseconds (doubles each attempt)
retry_backoff_ms = 1000

# Default destination path (optional)
# default_destination = "/mnt/backup"

# Maximum history entries (FIFO eviction when exceeded)
history_limit = 1000
```

### CLI Flags Reference

| Flag | Short | Description | Default |
|------|-------|-------------|---------|
| `--recursive` | `-r` | Copy directories recursively | off |
| `--verify` | | BLAKE3 checksum verification | off |
| `--compress` | | Enable zstd compression | off |
| `--resume` | | Resume interrupted transfer | off |
| `--chunks <N>` | | Parallel chunk count (0 = auto) | `0` |
| `--limit <BW>` | | Bandwidth limit (e.g., `10MB/s`) | unlimited |
| `--exclude <PAT>` | | Exclude glob pattern (repeatable) | none |
| `--include <PAT>` | | Include glob pattern (repeatable) | none |
| `--on-conflict` | | `overwrite` / `skip` / `rename` / `ask` | `ask` |
| `--on-error` | | `retry` / `skip` / `pause` | `retry` |
| `--dry-run` | | Preview without executing | off |
| `--encrypt` | | E2E encryption (send/receive) | off |
| `--verbose` | `-v` | Increase verbosity (`-vv` for trace) | normal |
| `--quiet` | `-q` | Suppress output except errors | off |

### Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Override log filter (e.g., `flux=debug`) |
| `FLUX_CONFIG_DIR` | Custom config directory |
| `FLUX_DATA_DIR` | Custom data directory |

### Data Files

| File | Location | Purpose |
|------|----------|---------|
| `config.toml` | Config dir | User preferences |
| `aliases.toml` | Config dir | Saved path aliases |
| `identity.json` | Config dir | Device key pair (auto-generated) |
| `trusted_devices.json` | Config dir | TOFU trust store |
| `queue.json` | Data dir | Transfer queue state |
| `history.json` | Data dir | Transfer history |

---

## Security

### Encryption

Flux uses modern, well-audited cryptographic primitives:

| Component | Algorithm | Purpose |
|-----------|-----------|---------|
| Key exchange | X25519 (Curve25519) | Ephemeral Diffie-Hellman per session |
| Symmetric cipher | XChaCha20-Poly1305 | AEAD encryption with 192-bit nonces |
| File integrity | BLAKE3 | Checksum verification |

- **No custom cryptography** — all primitives come from audited Rust crates (`chacha20poly1305`, `x25519-dalek`, `blake3`)
- **Ephemeral keys per session** — even if one session is compromised, others remain secure (forward secrecy)
- **Random nonces** — XChaCha20's 192-bit nonce space makes random generation safe without counters
- **No certificates or PKI** — TOFU model eliminates certificate management overhead

### Trust Model

Flux uses Trust-on-First-Use (TOFU), the same model as SSH:

1. **First connection** — device public key is saved automatically
2. **Subsequent connections** — key is verified against the stored value
3. **Key change detected** — connection is refused with a warning (possible impersonation)

Manage trusted devices with `flux trust list` and `flux trust rm <name>`.

### Identity

Your device identity (X25519 key pair) is generated automatically on first use and stored in `~/.config/flux/identity.json`. The private key never leaves your machine and is never transmitted.

> [!IMPORTANT]
> The identity file contains your private key. Treat it like an SSH private key — do not share it or commit it to version control.

---

## Architecture

### Project Structure

```
src/
├── main.rs                 # Entry point, CLI dispatch
├── cli/
│   └── args.rs             # Clap derive definitions
├── backend/
│   ├── mod.rs              # FluxBackend trait
│   ├── local.rs            # Local filesystem (std::fs)
│   ├── sftp.rs             # SFTP via ssh2/libssh2
│   ├── smb.rs              # SMB via Windows UNC paths
│   └── webdav.rs           # WebDAV via reqwest HTTP
├── transfer/
│   ├── mod.rs              # Transfer orchestration
│   ├── copy.rs             # Single-file copy with progress
│   ├── chunk.rs            # Chunk planning and auto-tuning
│   ├── parallel.rs         # Rayon-based parallel I/O
│   ├── checksum.rs         # BLAKE3 hashing
│   ├── compress.rs         # Zstd compression
│   ├── resume.rs           # Resume manifests
│   ├── throttle.rs         # Token-bucket bandwidth control
│   ├── filter.rs           # Glob include/exclude
│   └── conflict.rs         # Conflict resolution
├── protocol/
│   ├── mod.rs              # Protocol enum
│   ├── parser.rs           # Auto-detection from path strings
│   └── auth.rs             # Authentication types
├── config/
│   ├── types.rs            # FluxConfig, enums
│   ├── aliases.rs          # AliasStore (TOML-backed)
│   └── paths.rs            # Platform-specific directories
├── queue/
│   ├── state.rs            # QueueStore (JSON-backed)
│   └── history.rs          # HistoryStore with FIFO cap
├── discovery/
│   ├── mdns.rs             # mDNS service registration/browsing
│   └── service.rs          # DiscoveredDevice types
├── net/
│   ├── protocol.rs         # Wire protocol (bincode framing)
│   ├── sender.rs           # TCP send with handshake
│   └── receiver.rs         # TCP receive with mDNS
├── security/
│   ├── crypto.rs           # X25519 identity, XChaCha20 channel
│   └── trust.rs            # TOFU trust store
├── sync/
│   ├── plan.rs             # SyncAction, SyncPlan
│   ├── engine.rs           # Sync execution
│   ├── watch.rs            # Filesystem watcher (notify)
│   └── schedule.rs         # Cron-based scheduling
├── tui/
│   ├── app.rs              # TUI application loop
│   ├── terminal.rs         # Terminal setup/teardown
│   ├── event.rs            # Async event handling
│   ├── action.rs           # User action dispatch
│   ├── theme.rs            # Colors and styling
│   └── components/
│       ├── dashboard.rs    # Transfer dashboard
│       ├── file_browser.rs # Directory navigation
│       ├── queue_view.rs   # Queue management
│       ├── history_view.rs # History display
│       └── status_bar.rs   # Status/help bar
├── progress/
│   └── bar.rs              # indicatif progress bars
└── error.rs                # FluxError enum with suggestions
```

### Key Design Decisions

| Decision | Rationale |
|----------|-----------|
| Synchronous `FluxBackend` trait | Network backends (ssh2, reqwest) use blocking I/O; async would add complexity without benefit since transfers are inherently sequential per-file |
| `rayon` for parallelism | Data-parallel chunk processing maps perfectly to rayon's work-stealing thread pool |
| Per-chunk compression | Enables parallel decompression and makes resume work correctly with compressed transfers |
| Token-bucket throttling | Allows short bursts (2s) for better utilization while maintaining average rate limit |
| JSON sidecar manifests | Human-readable, debuggable, and trivially portable — no database dependency |
| TOFU over certificates | Zero setup required — matches the "just works" philosophy |
| Auto protocol detection | Users paste paths from file explorers, terminals, docs — Flux adapts to whatever format they use |

### Chunk Auto-Tuning

| File Size | Chunks | Why |
|-----------|--------|-----|
| < 10 MB | 1 | Thread overhead exceeds benefit |
| 10 – 100 MB | 2 | Light parallelism, low overhead |
| 100 MB – 1 GB | 4 | Good parallelism for typical files |
| 1 – 10 GB | 8 | Saturate most disk I/O |
| > 10 GB | 16 | Maximum parallelism (capped at CPU count) |

---

## Limitations

Being honest about what Flux doesn't do (yet):

- **No two-way sync** — `flux sync` is one-way (source → destination). Use git or Syncthing for bidirectional sync
- **No cloud storage APIs** — no native S3, Google Drive, or OneDrive support. Use WebDAV mount points as a workaround
- **WebDAV buffers in memory** — the WebDAV backend buffers writes in RAM before flushing. Very large files over WebDAV may use significant memory
- **SMB on Linux/macOS** — SMB support currently requires Windows. On Linux/macOS, mount the share with `mount.cifs` first and use local paths
- **No GUI** — Flux is terminal-only by design. The TUI provides interactivity, but there's no graphical interface
- **Single-connection network backends** — SFTP/SMB/WebDAV backends don't support parallel chunks (only local-to-local transfers benefit from parallelism)

---

## Tech Stack

| Crate | Purpose |
|-------|---------|
| [clap](https://crates.io/crates/clap) | CLI argument parsing with derive macros |
| [tokio](https://crates.io/crates/tokio) | Async runtime (TUI events, scheduling) |
| [rayon](https://crates.io/crates/rayon) | Data-parallel chunk processing |
| [indicatif](https://crates.io/crates/indicatif) | Terminal progress bars |
| [ratatui](https://crates.io/crates/ratatui) | Terminal user interface |
| [ssh2](https://crates.io/crates/ssh2) | SFTP via libssh2 |
| [reqwest](https://crates.io/crates/reqwest) | HTTP client for WebDAV |
| [blake3](https://crates.io/crates/blake3) | Fastest cryptographic hash |
| [zstd](https://crates.io/crates/zstd) | Zstandard compression |
| [chacha20poly1305](https://crates.io/crates/chacha20poly1305) | AEAD symmetric encryption |
| [x25519-dalek](https://crates.io/crates/x25519-dalek) | Elliptic curve Diffie-Hellman |
| [mdns-sd](https://crates.io/crates/mdns-sd) | mDNS/Bonjour service discovery |
| [notify](https://crates.io/crates/notify) | Cross-platform filesystem watcher |
| [globset](https://crates.io/crates/globset) | Fast glob pattern matching |
| [walkdir](https://crates.io/crates/walkdir) | Recursive directory traversal |

---

## Contributing

Contributions are welcome! Please:

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run the test suite (`cargo test`)
5. Commit your changes (`git commit -m 'Add amazing feature'`)
6. Push to the branch (`git push origin feature/amazing-feature`)
7. Open a Pull Request

### Building from Source

```bash
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build
cargo test
```

> [!NOTE]
> Building the SFTP backend requires OpenSSL development headers (or uses vendored OpenSSL on Windows). On Debian/Ubuntu: `sudo apt install libssl-dev`. On macOS: `brew install openssl`.

---

## License

MIT License. See [LICENSE](LICENSE) for details.

---

<p align="center">
  <strong>Built by <a href="https://github.com/tallowteam">tallowteam</a></strong>
  <br>
  <sub>Made with Rust, caffeine, and too many terminal windows.</sub>
</p>
