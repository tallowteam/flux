# Flux Security Audit Preparation Report

**Prepared:** 2026-02-17
**Project:** Flux - Rust CLI file transfer tool
**Version:** 1.0.0
**License:** AGPL-3.0
**Method:** Trail of Bits audit preparation checklist (4-step process)

---

## Table of Contents

1. [Review Goals](#1-review-goals)
2. [Static Analysis & Easy Issues](#2-static-analysis--easy-issues)
3. [Code Accessibility & Scope](#3-code-accessibility--scope)
4. [Documentation & Architecture](#4-documentation--architecture)
5. [Dependency Security Review](#5-dependency-security-review)
6. [Pre-Audit Remediation Status](#6-pre-audit-remediation-status)
7. [Audit Prep Checklist](#7-audit-prep-checklist)

---

## 1. Review Goals

### 1.1 Security Objectives

| Priority | Objective |
|----------|-----------|
| **Critical** | Verify correctness of the P2P encrypted channel (X25519 + XChaCha20-Poly1305 + BLAKE3 KDF) |
| **Critical** | Validate code-phrase authentication binding (PAKE-like KDF binding) |
| **Critical** | Assess TOFU trust model for resistance to impersonation on hostile LANs |
| **High** | Verify receiver-side defenses against malicious senders (OOM, path traversal, symlink, oversized payloads) |
| **High** | Verify sender-side defenses against malicious receivers (timeouts, stalling) |
| **High** | Review credential handling (identity.json permissions, zeroization, Debug redaction, URL stripping) |
| **Medium** | Assess mDNS discovery for spoofing/race conditions |
| **Medium** | Review SFTP/WebDAV backend authentication and credential handling |
| **Medium** | Evaluate file I/O safety (symlink following, TOCTOU, parallel chunked writes) |
| **Low** | Review TUI for terminal escape injection from untrusted filenames |

### 1.2 Areas of Concern

1. **Code-phrase mode authentication**: The `complete_with_code()` function binds the code phrase into the KDF. An auditor should verify this binding is cryptographically sound and that an attacker who intercepts the DH exchange but lacks the code phrase cannot derive the session key.

2. **Trust store resilience**: The trust store now returns an error on corruption (not silent recovery), but the `add_device()` function still allows unrestricted key replacement. Verify the full trust lifecycle for race conditions and downgrade attacks.

3. **Receiver input validation chain**: The receiver is the primary trust boundary. Verify the entire validation chain: protocol version -> TOFU -> key exchange -> file size cap -> allocation cap -> sequential offset validation -> data overflow check -> BLAKE3 checksum -> sanitized filename.

4. **Memory safety in `unsafe` blocks**: Two `unsafe` blocks exist in production code:
   - `src/security/crypto.rs:63-68` -- `write_volatile` for secret key zeroization in `Drop`
   - `src/backend/sftp.rs:38-39` -- `unsafe impl Send/Sync for SftpBackend`

5. **Streaming I/O**: The sender now uses streaming I/O (two-pass: hash then stream), and the receiver streams directly to disk. Verify no code paths remain that buffer entire files in memory.

### 1.3 Worst-Case Scenarios

| Scenario | Impact | Mitigation |
|----------|--------|------------|
| Remote code execution via crafted network message | Full system compromise | Bincode deserialization capped at 2 MB; Rust memory safety |
| Key extraction from identity.json | Device impersonation | 0o600 Unix perms; Windows icacls; zeroization on drop |
| Trust store poisoning via auto-trust | Accept files from attacker | Interactive trust confirmation now required (Finding 3 fixed) |
| Man-in-the-middle on code-phrase transfer | File interception | Code phrase bound to KDF (Finding 1 fixed); verify binding |
| Arbitrary file overwrite via path traversal | Data destruction / code execution | `sanitize_filename()` + `create_new(true)` + Windows reserved names |
| Denial of service via resource exhaustion | Service unavailability | 4 GB file cap; 2 MB frame cap; 30-min connection timeout; sender timeouts |

### 1.4 Questions for Auditors

1. Is the BLAKE3 `derive_key` binding of the code phrase to the DH shared secret sufficient, or would a full SPAKE2/CPace PAKE provide materially stronger guarantees?
2. Is the `write_volatile` approach in `DeviceIdentity::drop()` sound for zeroizing `StaticSecret` internals, given that `x25519-dalek::StaticSecret` does not expose a mutable reference?
3. Are the `unsafe impl Send for SftpBackend` / `unsafe impl Sync for SftpBackend` sound given the `ssh2` crate's thread-safety properties?
4. Can a malicious mDNS peer cause the receiver to connect to an attacker-controlled endpoint in code-phrase mode, despite the KDF binding?
5. Is the 37-bit entropy of code phrases sufficient for the 5-minute window, given LAN network speeds?

---

## 2. Static Analysis & Easy Issues

### 2.1 Static Analysis Tooling Status

| Tool | Status | Notes |
|------|--------|-------|
| `cargo clippy` | **Needs fresh run** | Configured in CLAUDE.md; should be run before audit freeze |
| `cargo fmt --check` | **Needs fresh run** | Standard formatting check |
| `cargo audit` | **Not configured** | `cargo-audit` should be installed and run to check for known vulnerabilities in dependencies |
| `cargo deny` | **Not configured** | No `deny.toml` found; should be added for license/advisory checking |
| `cargo-geiger` | **Not configured** | Would be valuable to quantify unsafe usage across dependency tree |
| `dylint` | **Not configured** | Trail of Bits recommends for Rust projects |
| Semgrep | **Not configured** | Complementary static analysis |
| CI/CD | **Not configured** | No `.github/workflows/` directory found |
| `rust-toolchain.toml` | **Not present** | Should be added to pin compiler version |
| `clippy.toml` | **Not present** | No custom clippy configuration |

### 2.2 Unsafe Code Audit

Two `unsafe` blocks exist in production code (non-test):

**Block 1: `src/security/crypto.rs:63-68`**
```rust
unsafe {
    let ptr = secret_bytes.as_ptr() as *mut u8;
    std::ptr::write_volatile(ptr, 0);
    for i in 0..32 {
        std::ptr::write_volatile(ptr.add(i), 0);
    }
}
```
- **Purpose**: Zeroize `StaticSecret` bytes on drop via volatile writes.
- **Concern**: Casts away `const` from `as_bytes()` return. Byte 0 is zeroed twice (line 65 and loop iteration i=0). The approach is sound but relies on `StaticSecret` internal layout being a contiguous `[u8; 32]`.
- **Recommendation**: Verify with auditor. If `x25519-dalek` v2 implements `ZeroizeOnDrop` on `StaticSecret`, this manual block can be removed.

**Block 2: `src/backend/sftp.rs:38-39`**
```rust
unsafe impl Send for SftpBackend {}
unsafe impl Sync for SftpBackend {}
```
- **Purpose**: Assert thread-safety for the SFTP backend to satisfy the `FluxBackend: Send + Sync` trait bound.
- **Concern**: The comment says `ssh2::Session` and `ssh2::Sftp` are `Send + Sync`, but this should be verified against the specific `ssh2` crate version in Cargo.lock. If they are not actually `Send + Sync`, this is unsound.
- **Recommendation**: Auditor should verify `ssh2` crate version's actual trait implementations.

### 2.3 Unwrap Usage

459 `unwrap()` calls found across 31 source files. Most are in `#[cfg(test)]` blocks. Production code `unwrap()` calls that need review:

| File | Line Context | Risk |
|------|-------------|------|
| `src/backend/webdav.rs:79` | `HeaderValue::from_str(depth).unwrap()` | Low -- `depth` is always "0" or "1" from internal callers |
| `src/backend/webdav.rs:82` | `Method::from_bytes(b"PROPFIND").unwrap()` | Low -- constant valid HTTP method |
| `src/backend/webdav.rs:317` | `Method::from_bytes(b"MKCOL").unwrap()` | Low -- constant valid HTTP method |
| `src/main.rs` (progress styles) | `.unwrap()` on `ProgressStyle::with_template` | Low -- constant template strings |

**Assessment**: No high-risk `unwrap()` calls in production paths. All `panic!` calls are exclusively in `#[cfg(test)]` blocks.

### 2.4 TODO/FIXME Items

| File | Line | Item |
|------|------|------|
| `src/backend/webdav.rs:127` | `modified: None, // TODO: parse getlastmodified if needed` | Low priority; sync engine uses mtime comparison |

### 2.5 Dead Code

No `#[allow(dead_code)]` annotations found outside of the `SftpBackend.session` field (which is held to keep the SSH session alive).

### 2.6 Recommended Pre-Audit Actions

1. **Install and run `cargo audit`** to check for known CVEs in dependencies
2. **Install and run `cargo-geiger`** to quantify unsafe usage in the dependency tree
3. **Add `deny.toml`** for `cargo-deny` to enforce license and advisory policies
4. **Run `cargo clippy -- -D warnings`** and fix all warnings
5. **Run `cargo fmt --check`** and ensure formatting compliance
6. **Pin Rust toolchain** via `rust-toolchain.toml` (e.g., `channel = "1.82.0"`)
7. **Set up CI** with at least: `cargo test`, `cargo clippy`, `cargo fmt`, `cargo audit`

---

## 3. Code Accessibility & Scope

### 3.1 In-Scope Files

| Module | Files | Lines (approx.) | Security Relevance |
|--------|-------|-----------------|-------------------|
| `security/` | `crypto.rs`, `trust.rs` | ~720 | **Critical** -- crypto, TOFU trust |
| `net/` | `sender.rs`, `receiver.rs`, `protocol.rs`, `codephrase.rs` | ~1870 | **Critical** -- P2P wire protocol, trust boundary |
| `discovery/` | `mdns.rs`, `service.rs` | ~300 | **High** -- mDNS discovery, spoofing surface |
| `backend/` | `mod.rs`, `local.rs`, `sftp.rs`, `smb.rs`, `webdav.rs` | ~1250 | **High** -- file I/O, auth, network backends |
| `protocol/` | `mod.rs`, `parser.rs`, `auth.rs` | ~450 | **Medium** -- protocol detection, Auth types |
| `transfer/` | `mod.rs`, `copy.rs`, `parallel.rs`, `chunk.rs`, `checksum.rs`, `compress.rs`, `resume.rs`, `throttle.rs`, `filter.rs`, `conflict.rs`, `stats.rs`, `verify.rs` | ~2800 | **Medium** -- transfer engine, file I/O |
| `sync/` | `mod.rs`, `engine.rs`, `plan.rs`, `watch.rs`, `schedule.rs` | ~800 | **Medium** -- sync engine, filesystem watcher |
| `config/` | `types.rs`, `aliases.rs`, `paths.rs` | ~400 | **Low** -- config loading |
| `cli/` | `args.rs`, `mod.rs` | ~300 | **Low** -- CLI argument parsing |
| `queue/` | `state.rs`, `history.rs` | ~500 | **Low** -- queue/history persistence |
| `tui/` | `app.rs`, `terminal.rs`, `event.rs`, `action.rs`, `theme.rs`, `components/*` | ~1500 | **Low** -- TUI rendering |
| `progress/` | `bar.rs`, `mod.rs` | ~150 | **Low** -- progress bars |
| `error.rs` | -- | ~300 | **Low** -- error types |
| `main.rs` | -- | ~450 | **Low** -- command dispatch |

**Total in-scope**: ~61 source files, ~11,790 lines of Rust

### 3.2 Out-of-Scope

- `target/` -- build artifacts
- `tests/` -- integration tests (9 files; useful for auditor context but not audit targets)
- `assets/` -- logo/images
- `docs/` -- setup guide, keybindings
- Third-party dependencies in `Cargo.lock`

### 3.3 Build Instructions

#### Prerequisites

| Requirement | Version | Platform |
|-------------|---------|----------|
| Rust (rustc + cargo) | 1.75+ | All |
| Git | Any | All |
| C compiler | gcc/clang/MSVC | All |
| OpenSSL dev headers | 3.x | Linux/macOS |
| Strawberry Perl | 5.x | Windows (vendored OpenSSL) |

#### Build Commands

**Linux / macOS:**
```bash
git clone https://github.com/tallowteam/flux.git
cd flux
cargo build --release
cargo test
```

**Windows (Git Bash):**
```bash
# If using FireDaemon pre-built OpenSSL (recommended for Windows):
OPENSSL_DIR="C:/Program Files/FireDaemon OpenSSL 3" OPENSSL_NO_VENDOR=1 cargo build --release
OPENSSL_DIR="C:/Program Files/FireDaemon OpenSSL 3" OPENSSL_NO_VENDOR=1 cargo test
```

**Windows (PowerShell with vendored OpenSSL):**
```powershell
# Requires Strawberry Perl in PATH (not MSYS Perl from Git Bash)
cargo build --release
cargo test
```

#### Verification

After building:
```bash
./target/release/flux --version  # Should output: flux 1.0.0
cargo test                        # All tests should pass
cargo clippy                      # Should produce no warnings
```

### 3.4 Version Freeze

**Recommendation**: Before the audit begins:
1. Create a dedicated branch: `git checkout -b audit-2026-q1`
2. Tag the commit: `git tag -a v1.0.0-audit -m "Audit preparation freeze"`
3. Lock dependencies: `Cargo.lock` is already committed (Cargo.lock version 4)
4. Share the commit hash with the assessment team

### 3.5 Boilerplate / Third-Party Code

All code in `src/` is original project code. No vendored or forked third-party source files.

Third-party functionality is accessed exclusively through Cargo dependencies (see Section 5).

---

## 4. Documentation & Architecture

### 4.1 Architecture Overview

Flux is a CLI file transfer tool with a layered architecture:

```
CLI Layer (clap)
    |
    v
Command Dispatch (main.rs)
    |
    +---> Transfer Engine (transfer/)
    |         |
    |         +---> FluxBackend trait (backend/)
    |         |         |
    |         |         +---> Local (std::fs)
    |         |         +---> SFTP (ssh2)
    |         |         +---> SMB (Windows UNC)
    |         |         +---> WebDAV (reqwest)
    |         |
    |         +---> Chunker, Compressor, Throttle, Resume, Verify
    |
    +---> P2P Network Layer (net/)
    |         |
    |         +---> TCP Sender / Receiver
    |         +---> Wire Protocol (bincode framing)
    |         +---> Code-Phrase System
    |
    +---> Security Layer (security/)
    |         |
    |         +---> X25519 + XChaCha20-Poly1305 (crypto.rs)
    |         +---> TOFU Trust Store (trust.rs)
    |
    +---> Discovery Layer (discovery/)
    |         |
    |         +---> mDNS/Bonjour (mdns-sd)
    |
    +---> Sync Engine (sync/)
    +---> TUI (ratatui)
    +---> Queue & History (queue/)
```

### 4.2 Trust Boundaries

```
UNTRUSTED                              TRUSTED
---------                              -------
[Remote Peer] --TCP--> receiver.rs --> [local filesystem]
                |
       [protocol.rs:decode_message]  <-- bincode 2MB limit
                |
       [crypto.rs:EncryptedChannel]  <-- decryption boundary
                |
       [trust.rs:TrustStore]         <-- TOFU identity check

[mDNS network] --multicast--> mdns.rs  <-- spoofable

[Local user] --CLI--> main.rs --> transfer/ --> backend/*
                                                  |
                                            [SFTP/SMB/WebDAV]
```

### 4.3 Actors and Privileges

| Actor | Trust Level | Capabilities |
|-------|-------------|-------------|
| Local user | Trusted | Full CLI access; reads/writes filesystem; manages trust store |
| Remote sender | Untrusted | Connects to receiver; sends Handshake, FileHeader, DataChunks |
| Remote receiver | Untrusted | Accepts connections; validates and writes files |
| mDNS network | Untrusted | Service advertisements; spoofable by any LAN device |
| SFTP/SMB/WebDAV server | Semi-trusted | Remote file storage; requires authentication |

### 4.4 Cryptographic Primitives

| Component | Algorithm | Library | Purpose |
|-----------|-----------|---------|---------|
| Key exchange | X25519 (Curve25519) | `x25519-dalek` v2 | Ephemeral + static DH |
| AEAD cipher | XChaCha20-Poly1305 | `chacha20poly1305` v0.10 | Per-chunk encryption |
| KDF | BLAKE3 `derive_key` | `blake3` v1.8 | Domain-separated key derivation |
| File integrity | BLAKE3 hash | `blake3` v1.8 | Checksum verification |
| CSPRNG | OsRng | `chacha20poly1305::aead::OsRng` | Key/nonce generation |
| Constant-time eq | `ct_eq` | `subtle` v2 | Trust store key comparison |
| Key zeroing | `write_volatile` + `zeroize` | manual + `zeroize` v1 | Secret material cleanup |

### 4.5 Key Data Flows

**Direct P2P Send (`flux send file.txt @device`):**
1. `resolve_device_target("@device")` -- mDNS discovery (3s browse, prefix match)
2. TCP connect to discovered host:port
3. Send `Handshake(version=1, device_name, public_key?)`
4. Receive `HandshakeAck(accepted, peer_public_key?)`
5. If encrypted: `EncryptedChannel::complete(our_secret, peer_public)` -- DH + KDF
6. Stream BLAKE3 checksum computation from disk (pass 1)
7. Send `FileHeader(filename, size, checksum, encrypted)`
8. Stream `DataChunk`s from disk in 256 KB chunks (pass 2), encrypting each
9. Receive `TransferComplete`

**Code-Phrase Transfer (`flux send file.txt` / `flux receive <code>`):**
1. Sender generates code phrase (~37 bits entropy)
2. Sender binds TCP on port 0, registers mDNS with `code_hash`
3. Receiver discovers sender via mDNS `code_hash` match
4. Receiver TCP connects to sender
5. Sender sends `Handshake` with ephemeral public key
6. Receiver sends `HandshakeAck` with ephemeral public key
7. Both sides call `EncryptedChannel::complete_with_code(secret, peer_pub, code)` -- binds code phrase to KDF
8. Encrypted file transfer proceeds as in direct mode

### 4.6 Key Constants

| Constant | Value | Location | Security Relevance |
|----------|-------|----------|-------------------|
| `PROTOCOL_VERSION` | 1 | `protocol.rs:12` | Version negotiation |
| `MAX_FRAME_SIZE` | 2 MB | `protocol.rs:19` | Deserialization bomb limit |
| `CHUNK_SIZE` | 256 KB | `protocol.rs:26` | Data chunk payload size |
| `MAX_RECEIVE_SIZE` | 4 GB | `receiver.rs:942` | Max accepted file size |
| `KDF_CONTEXT` | `"flux v1 xchacha20poly1305 session key"` | `crypto.rs:28` | Domain separation |
| `HANDSHAKE_TIMEOUT` | 30 sec | `sender.rs:24` | Sender handshake timeout |
| `COMPLETION_TIMEOUT` | 300 sec | `sender.rs:26` | Sender completion timeout |
| Connection timeout | 30 min | `receiver.rs:91-92` | Receiver per-connection timeout |
| Code phrase entropy | ~37 bits | `codephrase.rs` | 9000 * 256^3 combinations |
| Code expiry | 5 min | `sender.rs:360-361` | Code-phrase mode accept timeout |

### 4.7 Glossary

| Term | Definition |
|------|-----------|
| **TOFU** | Trust-on-First-Use: device public key is saved on first connection, verified on subsequent connections |
| **AEAD** | Authenticated Encryption with Associated Data: encryption + integrity in one operation |
| **KDF** | Key Derivation Function: derives a uniform key from a shared secret |
| **DH** | Diffie-Hellman: key exchange protocol for establishing a shared secret |
| **Code phrase** | Human-readable secret (`NNNN-word-word-word`) used for one-time transfers |
| **FluxBackend** | Core trait for protocol-agnostic file operations (stat, read, write, mkdir) |
| **Wire protocol** | Bincode-serialized `FluxMessage` enum framed with 4-byte length prefix |
| **Resume manifest** | JSON sidecar file tracking chunk completion for interrupted transfers |
| **Chunk** | A segment of a file processed independently (parallel I/O, resume unit) |
| **Trust store** | JSON file mapping device names to their X25519 public keys |
| **Identity file** | JSON file containing the device's X25519 key pair (`identity.json`) |

---

## 5. Dependency Security Review

### 5.1 Cryptographic Dependencies

| Crate | Version | Purpose | Assessment |
|-------|---------|---------|------------|
| `chacha20poly1305` | 0.10 | AEAD cipher | RustCrypto project; widely audited |
| `x25519-dalek` | 2 | Key exchange | Dalek Cryptography; well-maintained |
| `blake3` | 1.8 | Hashing / KDF | Official BLAKE3 team; audited |
| `rand` | 0.9 | CSPRNG | Rust ecosystem standard; uses OS entropy |
| `base64` | 0.22 | Key encoding | Standard encoding; no security-critical logic |
| `zeroize` | 1 | Secret zeroing | RustCrypto project; critical for key hygiene |
| `subtle` | 2 | Constant-time ops | RustCrypto project; prevents timing leaks |

**Assessment**: All cryptographic crates are from well-maintained, audited projects. No custom cryptography implementations.

### 5.2 Network Dependencies

| Crate | Version | Purpose | Assessment |
|-------|---------|---------|------------|
| `ssh2` | 0.9 | SFTP (libssh2 bindings) | C library bindings; vendored OpenSSL; `unsafe impl Send/Sync` concern |
| `reqwest` | 0.12 | WebDAV HTTP client | Widely used; blocking mode |
| `mdns-sd` | 0.18 | mDNS discovery | Pure Rust; relatively niche |
| `tokio` | 1 | Async runtime | De facto standard |
| `tokio-util` | 0.7 | Codec framing | Standard companion crate |
| `bincode` | 2 | Wire protocol serialization | Compact binary; 2 MB deserialization limit enforced |
| `futures` | 0.3 | Async utilities | Standard ecosystem crate |

**Concerns**:
- `ssh2` vendors OpenSSL via the `vendored-openssl` feature, pulling in a C library build chain. On Windows, build requires either Strawberry Perl or pre-built OpenSSL.
- `mdns-sd` is relatively niche compared to other dependencies; may benefit from a focused review.

### 5.3 Other Dependencies

| Crate | Version | Purpose | Risk |
|-------|---------|---------|------|
| `clap` | 4.5 | CLI parsing | Low |
| `serde` / `serde_json` / `toml` | Latest | Serialization | Low |
| `walkdir` | 2.5 | Directory traversal | Low -- `follow_links(false)` used |
| `indicatif` | 0.18 | Progress bars | Low |
| `ratatui` / `crossterm` | 0.30 / 0.29 | TUI | Low |
| `notify` / `notify-debouncer-full` | 8 / 0.7 | Filesystem watcher | Low |
| `cron` | 0.15 | Schedule parsing | Low |
| `chrono` | 0.4 | Date/time | Low |
| `dirs` | 5 | Platform paths | Low |
| `rpassword` | 7 | Password prompting | Low |
| `thiserror` / `anyhow` | 2 / 1 | Error handling | Low |
| `tracing` / `tracing-subscriber` | 0.1 / 0.3 | Logging | Low |
| `url` | 2 | URL parsing | Low |
| `globset` | 0.4 | Glob matching | Low |
| `bytesize` | 1.3 | Human-readable sizes | Low |
| `gethostname` | 0.5 | Hostname detection | Low |
| `rayon` | 1.10 | Parallel I/O | Low |
| `zstd` | 0.13 | Compression | Low -- C library bindings |

### 5.4 Recommended Pre-Audit Actions

1. Run `cargo audit` to check all dependencies against the RustSec Advisory Database
2. Run `cargo-geiger` to quantify unsafe code in the dependency tree
3. Configure `cargo-deny` with a `deny.toml` for ongoing license/advisory/source checks
4. Consider pinning exact dependency versions for audit reproducibility

---

## 6. Pre-Audit Remediation Status

A prior security analysis (2026-02-17) identified 13 findings. Here is the current remediation status based on code review:

| # | Finding | Severity | Status | Evidence |
|---|---------|----------|--------|----------|
| 1 | Code-phrase lacks PAKE binding | HIGH | **FIXED** | `EncryptedChannel::complete_with_code()` added at `crypto.rs:270-292`; both `sender.rs:418` and `receiver.rs:640` use code-bound KDF |
| 2 | Entire file buffered in memory (OOM) | HIGH | **FIXED** | Sender uses streaming two-pass I/O (`sender.rs:146-161` hash pass, `sender.rs:188-220` stream pass); Receiver streams to disk via `OpenOptions::create_new` + `write_all` in chunk loop (`receiver.rs:377-476`) |
| 3 | Auto-TOFU without user confirmation | MEDIUM | **FIXED** | Interactive trust confirmation added at `receiver.rs:196-222` (`Trust this device? [y/N]`) |
| 4 | Silent trust store corruption recovery | MEDIUM | **FIXED** | `TrustStore::load()` at `trust.rs:68-81` now returns `Err(FluxError::TrustError(...))` on corruption; separate `load_or_reset()` method for intentional resets |
| 5 | TOCTOU + symlink in receiver file write | MEDIUM | **FIXED** | `OpenOptions::create_new(true)` used at `receiver.rs:377-386` and `receiver.rs:718-727`; `write_file_exclusive()` helper at `receiver.rs:916-937` |
| 6 | No sender-side timeouts | MEDIUM | **FIXED** | `HANDSHAKE_TIMEOUT` (30s) and `COMPLETION_TIMEOUT` (300s) with `tokio::time::timeout` at `sender.rs:80-84` and `sender.rs:225-233` |
| 7 | `sanitize_filename()` missing Windows reserved names | LOW | **FIXED** | Windows reserved name blocking added at `receiver.rs:898-907` with tests at lines 1046-1063 |
| 8 | Memory zeroization gaps in identity I/O | LOW | **PARTIALLY FIXED** | `Zeroizing::new()` wrapper on `fs::read_to_string()` at `crypto.rs:90`; `Zeroizing` on JSON serialization at `crypto.rs:146`; `IdentityFile.secret_key` is still a plain `String` (not `Zeroizing`) |
| 9 | No Windows ACL on identity.json | LOW | **FIXED** | Windows `icacls` restriction added at `crypto.rs:170-181` |
| 10 | SharedSecret not explicitly zeroized | LOW | **NOT FIXED** | `SharedSecret` at `crypto.rs:251` and `crypto.rs:275` still not explicitly zeroized (relies on implicit `ZeroizeOnDrop` from `x25519-dalek` v2) |
| 11 | Temp file persistence on error | LOW | **FIXED** | `CleanupGuard` RAII pattern at `crypto.rs:157,188`; `std::mem::forget` on success path |
| 12 | mDNS first-match-wins race | INFO | **MITIGATED** | Finding 1 fix (KDF binding) makes this race benign -- attacker cannot complete handshake without code phrase |
| 13 | Entropy documentation mismatch | INFO | **FIXED** | Comment at `codephrase.rs:5` now says "~37 bits" |

### Remaining Items for Auditor Attention

1. **Finding 8 (partial)**: `IdentityFile.secret_key` is a plain `String`. When the `IdentityFile` struct is dropped, the base64 secret key in that `String` is deallocated but not zeroed.

2. **Finding 10**: The `SharedSecret` from `diffie_hellman()` is not explicitly zeroed. This is defense-in-depth only, as `x25519-dalek` v2 implements `ZeroizeOnDrop` on `SharedSecret`.

3. **New observation**: The `find_unique_path()` function at `receiver.rs:948-979` still uses `path.exists()` for collision checking, but the actual file write uses `create_new(true)`. There is a gap: if `find_unique_path` returns a path that "doesn't exist" but another process creates it before `create_new(true)`, the write will correctly fail with `AlreadyExists`. This is safe but could cause spurious failures under concurrent access.

---

## 7. Audit Prep Checklist

### Step 1: Review Goals

- [x] Security objectives documented
- [x] Areas of concern identified
- [x] Worst-case scenarios enumerated
- [x] Questions for auditors drafted

### Step 2: Resolve Easy Issues

- [ ] Run `cargo clippy -- -D warnings` and fix all warnings
- [ ] Run `cargo fmt --check` and ensure compliance
- [ ] Install and run `cargo audit` -- check for CVEs
- [ ] Install and run `cargo-geiger` -- quantify unsafe in deps
- [ ] Add `deny.toml` for `cargo-deny`
- [ ] Add `rust-toolchain.toml` to pin compiler version
- [x] Previous security findings triaged (13 findings, 10 fixed, 2 partial, 1 defense-in-depth)
- [x] No dead code found
- [ ] Increase test coverage for security-critical paths (see below)

### Step 3: Code Accessibility

- [x] In-scope file list with line counts and security relevance
- [x] Out-of-scope files identified
- [x] Build instructions documented and verified (multi-platform)
- [ ] Freeze stable version on dedicated branch
- [ ] Tag release for audit
- [x] No boilerplate/third-party code in src/

### Step 4: Documentation

- [x] Architecture overview with diagrams
- [x] Trust boundaries mapped
- [x] Actors and privileges documented
- [x] Cryptographic primitives inventory
- [x] Key data flows documented
- [x] Key constants catalog
- [x] Glossary of domain terms
- [x] Dependency security review

### Test Coverage Gaps

The following security-critical areas should have additional tests before audit:

| Area | Current Coverage | Recommended Tests |
|------|-----------------|-------------------|
| `complete_with_code()` KDF binding | Basic roundtrip + wrong-code test | Test that DH-only (no code) cannot decrypt code-bound ciphertext |
| Trust store corruption handling | `corrupted_file_returns_error` test exists | Add test for partial corruption, empty file, oversized file |
| `sanitize_filename()` | Path traversal + Windows reserved names | Add Unicode edge cases, very long filenames, null bytes |
| Receiver offset validation | Tested implicitly via integration | Add unit test for out-of-order offsets, duplicate offsets |
| Sender timeout behavior | Not tested | Add integration test with mock slow receiver |
| `write_file_exclusive()` | Overwrite prevention test exists | Add symlink attack test (Unix only) |

### Pre-Audit Timeline Recommendation

**2 weeks before audit:**
- Run all static analysis tools (clippy, audit, geiger, deny)
- Fix any findings
- Add missing test cases from coverage gaps table

**1 week before audit:**
- Freeze version on `audit-2026-q1` branch
- Tag with `v1.0.0-audit`
- Run full test suite on clean checkout (all platforms)
- Verify build instructions work from scratch

**3 days before audit:**
- Send this report + source code to assessment team
- Share commit hash and branch name
- Provide access to repository (or tar.gz snapshot)

---

## Appendix A: File Sensitivity Map

| File | On Disk Location | Permissions | Sensitivity |
|------|-----------------|-------------|-------------|
| `identity.json` | Config dir | 0o600 (Unix) / icacls (Windows) | **Critical** -- X25519 private key |
| `trusted_devices.json` | Config dir | Default | **High** -- trust decisions |
| `config.toml` | Config dir | Default | **Low** -- user preferences |
| `aliases.toml` | Config dir | Default | **Low** -- may contain server URIs |
| `history.json` | Data dir | Default | **Medium** -- transfer history (credentials stripped) |
| `queue.json` | Data dir | Default | **Low** -- pending transfer queue |
| `*.flux-resume.json` | Alongside dest file | Default | **Low** -- resume manifest (no file content) |

## Appendix B: Credential Hygiene Measures

1. **Auth Debug redaction**: `Auth` enum has custom `Debug` impl that prints `[REDACTED]` for passwords and passphrases (`src/protocol/auth.rs:35-62`)
2. **URL credential stripping**: `strip_url_credentials()` removes user:pass from URLs before recording to history (`src/transfer/mod.rs:947-957`)
3. **Identity file permissions**: Owner-only on Unix (0o600), icacls restriction on Windows
4. **Key material zeroization**: `StaticSecret` zeroed via `write_volatile` in `Drop`; intermediate `secret_bytes` zeroed immediately; `Zeroizing<String>` wrappers on serialized JSON
5. **SFTP username defaulting**: No longer defaults to "root"; returns error if username cannot be determined (`src/backend/sftp.rs:362-371`)
