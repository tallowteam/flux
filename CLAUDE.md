# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

Flux is a Rust CLI tool for blazing-fast file transfer across local, SFTP, SMB, and WebDAV protocols. It features parallel chunked transfers, resumable transfers, BLAKE3 integrity verification, zstd compression, P2P encrypted sends via mDNS discovery, one-way directory sync, a ratatui-based TUI, and a transfer queue system.

Licensed under AGPL-3.0.

## Build & Test Commands

```bash
cargo build                    # Debug build
cargo build --release          # Release build (binary at target/release/flux)
cargo test                     # Run all tests (unit + integration)
cargo test <test_name>         # Run a single test by name
cargo test --test integration  # Run only the integration test file
cargo clippy                   # Lint
cargo fmt --check              # Check formatting
```

Building the SFTP backend requires OpenSSL development headers. On Debian/Ubuntu: `sudo apt install libssl-dev`. On macOS: `brew install openssl`.

On Windows, the vendored OpenSSL build often fails due to MSYS Perl conflicts in Git Bash. Use FireDaemon pre-built OpenSSL instead:

```bash
# Install once: winget install FireDaemon.OpenSSL
# Then build with:
OPENSSL_DIR="C:/Program Files/FireDaemon OpenSSL 3" OPENSSL_NO_VENDOR=1 cargo build
OPENSSL_DIR="C:/Program Files/FireDaemon OpenSSL 3" OPENSSL_NO_VENDOR=1 cargo test
```

## Security Hardening

The P2P network layer has been hardened against common attack vectors:

- **Crypto**: X25519 + XChaCha20-Poly1305 with BLAKE3 `derive_key` (domain-separated KDF). Key material zeroed via `zeroize` crate + `Drop` impls. Identity files saved with 0o600 permissions.
- **Wire protocol**: Bincode deserialization capped at 2 MB (`bincode::config::standard().with_limit::<{ 2 * 1024 * 1024 }>()`). Prevents OOM from malicious payloads.
- **Receiver**: Path traversal prevention (`sanitize_filename`), 4 GB max file size, 256 MB allocation cap, sequential chunk offset validation, data overflow checks, BLAKE3 checksum verification, encryption downgrade rejection, 30-min per-connection timeout.
- **Trust store**: Constant-time public key comparison via `subtle::ConstantTimeEq`. Corruption logged as warning, not silently reset.
- **Credentials**: `Auth` enum has custom `Debug` impl that redacts passwords. URL credentials stripped from history and logs via `strip_url_credentials()`.
- **Filesystem**: `WalkDir` uses `follow_links(false)` to prevent symlink attacks. Config directory created with 0o700 Unix permissions. SFTP refuses "root" as default username.

## Architecture

### Core Abstraction: `FluxBackend` Trait

The `FluxBackend` trait (`src/backend/mod.rs`) is the central abstraction. Every protocol (local, SFTP, SMB, WebDAV) implements the same synchronous trait (`stat`, `list_dir`, `open_read`, `open_write`, `create_dir_all`, `features`). The transfer engine is backend-agnostic.

Backend creation is routed through `create_backend()` which dispatches on `Protocol` variant.

### Protocol Detection

`protocol::detect_protocol()` (`src/protocol/parser.rs`) auto-detects the protocol from raw user input strings: SFTP URIs, UNC paths (SMB), HTTP/HTTPS URLs (WebDAV), or local filesystem paths. No flags needed -- users paste paths directly.

### Transfer Pipeline

`transfer::execute_copy()` (`src/transfer/mod.rs`) is the main entry point for all copy operations. The flow:
1. Resolve aliases -> detect protocols -> create backends
2. Build `TransferFilter` from `--exclude`/`--include` patterns
3. For single files: conflict resolution -> optional resume -> parallel chunked or sequential copy -> optional BLAKE3 verify
4. For directories: walkdir traversal with filtering -> per-file conflict/failure handling -> progress tracking
5. Record to transfer history on completion

CLI flags override `config.toml` values (loaded lazily via `config::types::load_config()`).

### Chunk Auto-Tuning

`transfer::chunk::auto_chunk_count()` scales parallelism by file size: 1 chunk (<10MB), 2 (10-100MB), 4 (100MB-1GB), 8 (1-10GB), 16 (>10GB, capped at CPU count). Parallel I/O uses `rayon`.

When `--limit` (bandwidth throttling) is set, transfers fall back to single-chunk sequential copy with a `ThrottledReader` (token-bucket algorithm).

### P2P Network Layer

- `net/sender.rs` and `net/receiver.rs`: TCP-based direct file transfer with bincode wire protocol
- `discovery/mdns.rs`: mDNS/Bonjour service discovery (`_flux._tcp.local.`)
- `security/crypto.rs`: X25519 key exchange + XChaCha20-Poly1305 AEAD encryption
- `security/trust.rs`: TOFU (Trust-on-First-Use) device key store

### Sync Engine

`sync::execute_sync()` dispatches to one of three modes: one-shot sync, `--watch` (filesystem watcher via `notify-debouncer-full` with 2s debounce), or `--schedule` (cron-based via `cron` crate). Sync uses mtime+size comparison with 2-second FAT32 tolerance. `--delete` removes orphans but refuses to wipe dest if source is empty (unless `--force`). Trailing-slash semantics follow rsync conventions.

### TUI

`tui/app.rs` is the main ratatui application loop with four tabs: Dashboard, File Browser, Queue, History. Uses `crossterm` for terminal events. Launched via `flux ui` or `--tui` flag.

### Error Handling

`FluxError` (`src/error.rs`) is a `thiserror`-based enum. Every variant has a `suggestion()` method returning user-facing hints. Errors display to stderr; stdout stays clean for data output. Tracing logs also go to stderr.

### Config & State

- Config dir (platform-specific via `dirs` crate): `config.toml`, `aliases.toml`, `identity.json`, `trusted_devices.json`
- Data dir: `queue.json`, `history.json`
- Resume manifests: JSON sidecar files alongside the destination file

### CLI Structure

CLI is defined with `clap` derive macros in `src/cli/args.rs`. Commands: `cp`, `add`, `alias`, `queue`, `history`, `completions`, `discover`, `send`, `receive`, `trust`, `ui`, `sync`. Global flags: `--verbose`/`-v`, `--quiet`/`-q`, `--tui`.

## Key Patterns

- **Synchronous `FluxBackend`**: Network backends use blocking I/O. Tokio is used for TUI events, mDNS, and scheduling -- not for file I/O.
- **CLI flags override config**: `on_conflict`/`on_error` CLI args take precedence over `config.toml` values.
- **Alias resolution before protocol detection**: `config::aliases::resolve_alias()` expands aliases like `nas:backups/` before `detect_protocol()` runs.
- **`TransferResult` for directory copies**: Individual file errors are collected, not fatal. The directory copy continues and reports all errors at the end.
- **Progress to stderr, data to stdout**: `eprintln!` for user messages, `println!` for machine-readable output (alias lists, history tables, etc.).

## Test Structure

- `tests/integration.rs`: Core copy operations (single file, directory, recursive, filters, conflict strategies)
- `tests/integration_phase2.rs`: Chunking, compression, resume, verification, throttling
- `tests/phase4_integration.rs`: Aliases, queue, history, completions
- `tests/phase5_integration.rs`: Discovery, send/receive, encryption, trust
- `tests/protocol_detection.rs`: Protocol auto-detection from path strings
- `tests/sftp_backend.rs`, `tests/smb_backend.rs`, `tests/webdav_backend.rs`: Network backend tests
- `tests/sync_tests.rs`: Sync engine tests

Integration tests use `assert_cmd` + `tempfile` for CLI testing. Helper: `flux()` returns `Command::cargo_bin("flux")`.
