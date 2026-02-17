# Flux Audit Context Document

**Date:** 2026-02-17
**Scope:** Full codebase (~106K LOC, 60 source files, 12 modules)
**Method:** Trail of Bits audit-context-building skill (3-phase ultra-granular analysis)

This document captures deep architectural context built through line-by-line code analysis. It is intended as input for vulnerability discovery, threat modeling, and security review.

---

## Phase 1: Initial Orientation

### Module Map

| Module | Files | Purpose |
|--------|-------|---------|
| `security/` | `crypto.rs`, `trust.rs` | X25519 + XChaCha20-Poly1305 crypto, TOFU trust store |
| `net/` | `sender.rs`, `receiver.rs`, `protocol.rs`, `codephrase.rs` | TCP P2P transfer, wire protocol, code-phrase mode |
| `discovery/` | `mdns.rs`, `service.rs` | mDNS/Bonjour device discovery |
| `transfer/` | `mod.rs`, `parallel.rs`, `copy.rs`, `resume.rs`, `filter.rs`, `chunk.rs`, `checksum.rs`, `stats.rs`, `verify.rs` | File transfer engine, chunked I/O, resume, stats |
| `backend/` | `mod.rs`, `local.rs`, `sftp.rs`, `smb.rs`, `webdav.rs` | Protocol-agnostic backend trait + implementations |
| `sync/` | `mod.rs` | One-way directory sync engine |
| `config/` | `types.rs`, `aliases.rs`, `paths.rs` | Config loading, alias resolution, platform paths |
| `cli/` | `args.rs` | Clap derive-based CLI definition |
| `tui/` | `app.rs`, `dashboard.rs`, `browser.rs`, `queue_tab.rs`, `history_tab.rs` | Ratatui terminal UI |
| `progress/` | `bar.rs` | Indicatif progress bar factories |
| `error.rs` | -- | FluxError thiserror enum with suggestions |
| `main.rs` | -- | Command dispatch |

### Actors

1. **Local user** -- Invokes CLI commands (`cp`, `send`, `receive`, `sync`, `verify`, `trust`)
2. **Remote sender** -- TCP client connecting to receiver; supplies Handshake, FileHeader, DataChunks
3. **Remote receiver** -- TCP server accepting connections; validates, decrypts, writes files
4. **mDNS network** -- LAN-visible service advertisements (untrusted, spoofable)
5. **File system** -- Source/destination of all transfers (local, SFTP, SMB, WebDAV)

### Key State Variables

| Variable | Location | Type | Sensitivity |
|----------|----------|------|-------------|
| `DeviceIdentity.secret_key` | `crypto.rs:35` | `StaticSecret` (32 bytes) | **Critical** -- long-lived X25519 private key |
| `EncryptedChannel.cipher` | `crypto.rs:200` | `XChaCha20Poly1305` | **Critical** -- session encryption key |
| `TrustStore.devices` | `trust.rs:47` | `BTreeMap<String, TrustedDevice>` | **High** -- device trust decisions |
| `identity.json` | disk | JSON (base64 secret + public key) | **Critical** -- persisted private key |
| `trusted_devices.json` | disk | JSON trust store | **High** -- if corrupted, TOFU resets |
| `file_data` (in-memory) | `sender.rs:141`, `receiver.rs:356` | `Vec<u8>` | **Medium** -- entire file in RAM |

### Trust Boundaries

```
                    UNTRUSTED                         TRUSTED
                    ---------                         -------
 [Remote Peer] --TCP--> [receiver.rs:handle_connection] --> [local filesystem]
                  |              |
                  |    [protocol.rs:decode_message]  <-- bincode deser (2MB limit)
                  |              |
                  |    [crypto.rs:EncryptedChannel]  <-- decryption boundary
                  |              |
                  |    [trust.rs:TrustStore]         <-- TOFU identity check
                  |
 [mDNS network] --multicast--> [mdns.rs:discover_*]  <-- spoofable

 [Local user] --CLI--> [main.rs] --> [transfer/mod.rs] --> [backend/*]
                                                              |
                                                        [SFTP/SMB/WebDAV]
                                                        (network backends)
```

---

## Phase 2: Ultra-Granular Function Analysis

### 2.1 Cryptographic Module (`security/crypto.rs`)

#### `DeviceIdentity::generate()` (line 75)
- Creates `StaticSecret::random_from_rng(OsRng)` -- CSPRNG via OS entropy
- Derives `PublicKey` from secret key
- **Invariant**: Every identity has a valid X25519 keypair

#### `DeviceIdentity::load_or_create()` (line 86)
- Loads `identity.json`, decodes base64 secret key into `[u8; 32]`
- Intermediate `secret_bytes` zeroed via `zeroize()` at line 109
- Verifies stored public key matches derived public key (corruption check)
- **Invariant**: Loaded identity is self-consistent (pubkey matches secret)

#### `DeviceIdentity::drop()` (line 56)
- Uses `unsafe { write_volatile }` to zero the secret key bytes
- Casts away const from `as_bytes()` return to get mutable pointer
- Zeroes byte 0 twice (once at line 65, again in loop at i=0 on line 66)
- **Observation**: This is a manual reimplementation of what `zeroize` does. The `StaticSecret` type from `x25519-dalek` does not implement `Zeroize`, forcing this manual approach.

#### `DeviceIdentity::save()` (line 136)
- Atomic write: tmp file + rename
- Unix: sets 0o600 permissions on tmp file before rename
- **Observation**: No Windows permission restriction (ACL). On Windows, the identity file is world-readable.

#### `EncryptedChannel::initiate()` (line 206)
- Creates `EphemeralSecret::random_from_rng(OsRng)` + derives `PublicKey`
- Returns `(EphemeralSecret, PublicKey)` -- caller sends public key to peer

#### `EncryptedChannel::complete()` (line 219)
- Performs X25519 DH: `secret.diffie_hellman(peer_public)`
- KDF: `blake3::derive_key("flux v1 xchacha20poly1305 session key", shared.as_bytes())`
- Creates `XChaCha20Poly1305` cipher from derived key
- Zeroes `derived_key` immediately after cipher creation (line 228)
- **Invariant**: Raw DH output never used directly as key; always passed through domain-separated KDF
- **Observation**: The `shared` secret (DH output) is NOT explicitly zeroed. It's on the stack and will be dropped, but not zeroized. `x25519-dalek` `SharedSecret` does not implement `Zeroize`.

#### `EncryptedChannel::encrypt()` (line 235)
- Generates random 24-byte nonce via `XChaCha20Poly1305::generate_nonce(&mut OsRng)`
- Returns `(ciphertext, nonce)` -- 192-bit XChaCha20 nonce space makes random nonces safe
- **Invariant**: Every encryption uses a fresh random nonce (no counter, no reuse)

#### `EncryptedChannel::decrypt()` (line 245)
- Standard AEAD decrypt; returns error on auth tag failure
- No timing leak -- AEAD comparison is constant-time (library guarantee)

### 2.2 Trust Store (`security/trust.rs`)

#### `TrustStore::load()` (line 56)
- Reads `trusted_devices.json` from config dir
- **Corrupted file handling**: `serde_json::from_str` failure triggers `tracing::warn!` and returns empty store
- **Observation**: A corrupted trust store silently resets all trust decisions. An attacker who can corrupt this file forces re-TOFU of all devices.

#### `TrustStore::is_trusted()` (line 118)
- `BTreeMap::get()` lookup by device name (NOT constant-time on name)
- **Length check**: `stored.len() == provided.len()` -- not constant-time but all base64 keys are 44 chars, so length is not secret
- **Key comparison**: `stored.ct_eq(provided)` via `subtle::ConstantTimeEq` -- constant-time
- Returns `Trusted`, `Unknown`, or `KeyChanged`
- **Invariant**: Key content comparison is always constant-time

#### `TrustStore::add_device()` (line 141)
- Updates existing device or inserts new
- `first_seen` preserved on update; `last_seen` updated
- **Observation**: No cap on number of devices. A malicious peer flooding unique device names could grow the trust store unboundedly.

#### `TrustStore::save()` (line 92)
- Atomic write: tmp file + rename
- **Observation**: No fsync/sync_all before rename. Crash between write and rename could leave no file.

### 2.3 Wire Protocol (`net/protocol.rs`)

#### Constants
- `PROTOCOL_VERSION = 1` -- single-byte version
- `MAX_FRAME_SIZE = 2 MB` -- bincode deserialization limit
- `CHUNK_SIZE = 256 KB` -- data chunk payload size

#### `FluxMessage` enum (line 38)
- 6 variants: `Handshake`, `HandshakeAck`, `FileHeader`, `DataChunk`, `TransferComplete`, `Error`
- **Observation**: `device_name` in `Handshake` is an unbounded `String`. Combined with 2MB frame limit, this is bounded to ~2MB but could be used for memory allocation within that limit.
- **Observation**: `DataChunk.data` is `Vec<u8>` -- bounded by frame limit, not by `CHUNK_SIZE`. A malicious sender could send chunks larger than 256KB (up to ~2MB).

#### `decode_message()` (line 130)
- Uses `bincode::config::standard().with_limit::<{ 2 * 1024 * 1024 }>()`
- This is the primary defense against deserialization bombs
- **Invariant**: No single message can cause allocation > 2MB during deserialization

### 2.4 Code Phrase System (`net/codephrase.rs`)

#### `generate()` (line 49)
- Uses `rand::rng()` (CSPRNG on all platforms)
- Format: `NNNN-word-word-word` (1000-9999, 256-word list)
- Entropy: 9000 * 256^3 = ~1.5 * 10^11 combinations (~37.2 bits)
- **Observation**: 37 bits of entropy is brute-forceable. At 1000 attempts/sec, exhaustive search takes ~1.7 days. No rate limiting exists on the receiver side.

#### `code_hash()` (line 102)
- `blake3::hash(code.as_bytes())` truncated to first 16 hex chars (64 bits)
- Used as mDNS TXT property for sender-receiver matching
- **Observation**: 64-bit hash prefix. Collision probability for random codes is negligible, but a targeted attacker could precompute matching hashes offline.
- **Observation**: The code phrase is NOT used as a PAKE input. It only serves for mDNS discovery. The actual encryption uses ephemeral X25519 -- there is NO authentication binding between the code phrase and the encrypted channel. Anyone who can respond to the mDNS query can impersonate the sender.

### 2.5 Sender (`net/sender.rs`)

#### `send_file()` (line 33)
- TCP client connecting to receiver
- **Entire file read into memory** at line 141: `std::fs::read(file_path)` -- OOM risk for large files
- Computes BLAKE3 checksum of full file in memory
- Sends handshake, waits for ack, sends file header, streams chunks
- **Observation**: No timeout on `framed.next().await` when waiting for HandshakeAck (line 75) or TransferComplete (line 206). A malicious receiver could stall the sender indefinitely.

#### `send_with_code()` (line 252)
- Sender becomes TCP server (binds `0.0.0.0:0`)
- Registers mDNS with `code_hash` TXT property
- Accepts exactly one connection with 5-minute timeout
- Always encrypted (no `--encrypt` flag needed)
- **Observation**: After accepting one connection, the sender processes it without verifying the connector knows the code phrase. The code phrase only serves for mDNS discovery; any TCP client that connects to the port gets the handshake.

#### `resolve_device_target()` (line 507)
- `@devicename` format triggers mDNS discovery
- Case-insensitive prefix matching: `d.name.to_lowercase().starts_with(&name_lower)`
- **Observation**: Prefix matching means `@a` matches `alice-laptop`. A spoofed mDNS service named `alice-evil` would also match `@alice`. First match wins -- race condition.

### 2.6 Receiver (`net/receiver.rs`)

#### `start_receiver()` (line 34)
- Binds TCP `0.0.0.0:{port}`, registers mDNS
- Each connection gets 30-minute timeout via `tokio::time::timeout`
- Spawns a new task per connection
- **Observation**: No connection rate limiting. An attacker could open many connections to exhaust resources (each allocates up to 256MB for file data).

#### `handle_connection()` (line 114)
- **Primary trust boundary** -- all input from remote peer is untrusted
- Protocol flow: Handshake -> TOFU check -> KeyExchange -> FileHeader -> DataChunks -> TransferComplete

**Handshake validation (lines 128-167):**
- Verifies `PROTOCOL_VERSION` match (rejects with HandshakeAck if mismatch)
- Extracts `peer_device_name` and `peer_public_key`
- **Observation**: `peer_device_name` is attacker-controlled, used as BTreeMap key in trust store

**TOFU check (lines 170-284):**
- `TrustStatus::Trusted` -- verify known key matches (constant-time)
- `TrustStatus::Unknown` -- **auto-trust** (v1 behavior, line 196). Adds to trust store and saves.
- `TrustStatus::KeyChanged` -- reject with SSH-style warning banner
- **Encryption downgrade prevention** (line 253): If receiver is NOT in encrypt mode but sender offers encryption, connection is rejected. Prevents silent downgrade.
- **Observation**: Auto-trust means any first-time connector is trusted without user confirmation.

**File receive (lines 350-427):**
- `MAX_RECEIVE_SIZE = 4 GB` (line 834)
- Pre-allocation capped at 256 MB (line 355): `file_size.min(256 * 1024 * 1024)`
- Sequential offset validation (line 373): `offset != expected_offset` -> reject
- Data overflow check (line 400): `received_bytes + chunk_len > file_size` -> reject
- **Entire file buffered in memory** (`Vec<u8>`) before writing to disk
- **Observation**: A sender claiming `file_size = 4GB` causes 256MB allocation + potential growth up to 4GB via `extend_from_slice`.

**Checksum verification (lines 432-453):**
- BLAKE3 hash of received data compared to expected checksum from FileHeader
- Mismatch sends Error message and returns error
- **Invariant**: If checksum provided, file is verified before writing

**File output (lines 333, 456):**
- `find_unique_path()` creates auto-renamed path if file exists
- `sanitize_filename()` strips directory components and leading dots
- **Observation**: `find_unique_path()` has TOCTOU between `exists()` check and `fs::write()`. Another process could create the file between check and write.
- **Observation**: `sanitize_filename()` does not handle Windows reserved names (`CON`, `PRN`, `AUX`, `NUL`, `COM1`-`COM9`, `LPT1`-`LPT9`).

#### `receive_with_code()` (line 497)
- Code-phrase mode: receiver is TCP client, always encrypted
- Discovers sender via mDNS `code_hash` match
- No TOFU check (code-phrase implies intent to connect)
- Same file receive logic as `handle_connection()` (duplicated code)

### 2.7 Transfer Engine (`transfer/mod.rs`)

#### `execute_copy()` (~line 50)
- Main entry point for all `flux cp` operations
- Flow: alias resolution -> protocol detection -> backend creation -> single/dir copy
- **Observation**: `strip_url_credentials()` removes user:pass from URLs before history recording -- good credential hygiene

#### `copy_with_failure_handling()`
- Retry with exponential backoff
- **Observation**: Backoff uses `1 << retry` shift. If `max_retries` is very large (>63), this causes u64 overflow/panic. Current default max_retries is small, but the code does not cap the shift.

### 2.8 Parallel Copy (`transfer/parallel.rs`)

#### `parallel_copy_chunked()`
- Uses `rayon::par_iter` for parallel chunk reads/writes
- Cross-platform `read_at`/`write_at` (Unix pread/pwrite, Windows seek_read/seek_write)
- Pre-allocates destination file to full size via `set_len()`
- Per-chunk BLAKE3 hashing when verify enabled
- **Observation**: Pre-allocation to full file size could fail on low-disk systems. Error is handled.

### 2.9 Discovery (`discovery/mdns.rs`, `discovery/service.rs`)

#### `register_flux_service()` (mdns.rs:27)
- Registers `_flux._tcp.local.` with TXT properties (version, pubkey, code_hash)
- **Observation**: Public key is broadcast in cleartext via mDNS. Anyone on the LAN can see it.

#### `discover_by_code_hash()` (mdns.rs:77)
- Returns first matching service -- **race condition** if multiple services advertise same hash
- 30-second timeout (hardcoded at caller)

#### `sanitize_device_name()` (service.rs)
- DNS label sanitization: ASCII only, max 63 chars, collapse hyphens
- **Observation**: Only applied to OUR device name for registration. Names received from mDNS are NOT sanitized before use as trust store keys.

---

## Phase 3: Global System Understanding

### 3.1 State & Invariant Reconstruction

**Multi-function invariants:**

1. **Key material lifecycle**: `StaticSecret` created from OsRng -> stored base64 in identity.json (0o600 perms) -> loaded with intermediate zeroing -> `Drop` impl zeroes via `write_volatile`. Ephemeral secrets created from OsRng -> consumed by `diffie_hellman()` (moved, not copied) -> derived key zeroed after cipher creation.

2. **Wire protocol safety**: Every message decoded through `decode_message()` with 2MB bincode limit. Frame-level enforcement via `LengthDelimitedCodec::max_frame_length(MAX_FRAME_SIZE)`. Double protection: codec rejects oversized frames, bincode rejects oversized allocations.

3. **Receive-side validation chain**: Protocol version check -> TOFU/key exchange -> file size check (4GB) -> allocation cap (256MB) -> sequential offset validation -> data overflow check -> BLAKE3 checksum -> sanitized filename -> unique path. Each check is fail-fast (return Err).

4. **Encryption mode consistency**: If receiver is `--encrypt` and sender has no key -> reject. If receiver is NOT `--encrypt` and sender offers key -> reject. This prevents silent encryption downgrade in both directions.

5. **Code-phrase mode always encrypted**: `send_with_code()` and `receive_with_code()` always create EncryptedChannel regardless of flags.

### 3.2 End-to-End Workflow Reconstruction

#### Workflow 1: Direct P2P Send (`flux send file.txt @device`)

```
1. resolve_device_target("@device")
   -> discover_flux_devices(3s mDNS browse)
   -> case-insensitive prefix match -> (host, port)
2. send_file(host, port, file_path, encrypt, device_name)
   -> TCP connect
   -> Send Handshake(version=1, device_name, public_key?)
   -> Recv HandshakeAck
      -> if encrypt: complete key exchange -> EncryptedChannel
   -> fs::read(file_path) [ENTIRE FILE IN MEMORY]
   -> blake3::hash(file_data) -> checksum
   -> Send FileHeader(filename, size, checksum, encrypted)
   -> Loop: Send DataChunk(offset, data, nonce?) [256KB chunks]
   -> Recv TransferComplete
   -> Print stats
```

#### Workflow 2: Code-Phrase Receive (`flux receive 1234-ace-bad-car`)

```
1. codephrase::validate(code)
2. codephrase::code_hash(code) -> 16 hex chars
3. discover_by_code_hash(hash, 30s)
   -> mDNS browse for _flux._tcp.local.
   -> Match code_hash TXT property
   -> Return first match (host, port)
4. TCP connect to sender
5. Recv Handshake from sender
6. Generate ephemeral keypair
7. Send HandshakeAck with our public key
8. Complete key exchange -> EncryptedChannel
9. Recv FileHeader -> validate size < 4GB
10. Loop: Recv DataChunk -> decrypt -> validate offset/overflow -> append
11. BLAKE3 verify if checksum provided
12. fs::write(sanitized_path, file_data)
13. Send TransferComplete
```

#### Workflow 3: Local Directory Copy (`flux cp -r src/ dest/`)

```
1. resolve_alias(src) -> detect_protocol -> create_backend (local)
2. resolve_alias(dest) -> detect_protocol -> create_backend (local)
3. Build TransferFilter from --exclude/--include
4. First walkdir pass: count files + total bytes (for progress)
5. Second walkdir pass: for each file:
   a. Conflict resolution (skip/overwrite/rename/ask based on --on-conflict)
   b. copy_with_failure_handling():
      -> parallel_copy_chunked() if file > 10MB
      -> copy_file_with_progress() if <= 10MB
      -> optional BLAKE3 verify
   c. Collect errors (non-fatal per file)
6. Print TransferStats summary
```

### 3.3 Complexity & Fragility Clusters

Ranked by risk (highest first):

**Cluster 1: Code-Phrase Authentication Gap**
- Files: `codephrase.rs`, `sender.rs:send_with_code()`, `receiver.rs:receive_with_code()`, `mdns.rs:discover_by_code_hash()`
- The code phrase is used ONLY for mDNS discovery. It is NOT used as PAKE input. The ephemeral X25519 key exchange has no authentication binding to the code phrase. An attacker on the same LAN who observes the mDNS advertisement (or races to respond to it) can complete the key exchange without knowing the code phrase.
- Entropy: ~37 bits (brute-forceable in days, but mDNS makes this irrelevant since the hash is broadcast)

**Cluster 2: Memory Exhaustion via File Buffering**
- Files: `sender.rs:141` (`fs::read`), `receiver.rs:356` (`Vec::with_capacity` + `extend_from_slice`)
- Both sender and receiver load the ENTIRE file into memory. Sender does `fs::read(file_path)` with no size check. Receiver pre-allocates up to 256MB and can grow to 4GB.
- A 4GB transfer requires 4GB+ RAM on both sides (plus encryption overhead).

**Cluster 3: TOFU Auto-Trust + Device Name Spoofing**
- Files: `receiver.rs:188-201`, `trust.rs:141`, `sender.rs:520-523`
- Auto-trust on first connection (v1 behavior) + case-insensitive prefix matching for device resolution + attacker-controlled device names in Handshake messages
- An attacker can register an mDNS service with a name that prefix-matches a legitimate device, get auto-trusted, then receive files intended for the real device.

**Cluster 4: Receiver File Write Race Conditions**
- Files: `receiver.rs:840-871` (`find_unique_path`), `receiver.rs:814-829` (`sanitize_filename`)
- TOCTOU between `exists()` and `fs::write()` in `find_unique_path()`
- Missing Windows reserved name handling in `sanitize_filename()`
- No protection against symlink attacks on the output directory

**Cluster 5: Missing Timeouts on Sender Side**
- Files: `sender.rs:75` (HandshakeAck wait), `sender.rs:206` (TransferComplete wait)
- No timeout on `framed.next().await` -- a malicious receiver can stall the sender indefinitely
- Receiver has 30-min timeout per connection, but sender has none

### 3.4 Security-Relevant Observations

These are factual observations from the code analysis, not vulnerability classifications:

1. **No PAKE binding in code-phrase mode**: The code phrase authenticates discovery (mDNS hash match) but NOT the encrypted channel. The X25519 key exchange is unauthenticated. This is the most significant architectural observation.

2. **Shared secret not zeroed**: In `EncryptedChannel::complete()`, the `SharedSecret` from `diffie_hellman()` is not explicitly zeroed. `x25519-dalek::SharedSecret` does not implement `Zeroize`. The derived key IS zeroed.

3. **Entire-file-in-memory design**: Both sender (`fs::read`) and receiver (`Vec<u8>` buffer) hold the complete file in memory. No streaming I/O for the P2P path (unlike the local copy path which uses chunked/parallel I/O).

4. **Auto-trust in v1**: `TrustStatus::Unknown` results in automatic trust + save. No interactive confirmation. Future versions should prompt.

5. **No sender-side timeouts**: `send_file()` and `send_with_code()` have no timeouts on network reads. The receiver has a 30-minute timeout but the sender can block forever.

6. **mDNS first-match-wins**: `discover_by_code_hash()` returns the first matching service. If an attacker registers a service with the same code_hash before the legitimate sender, the receiver connects to the attacker.

7. **Trust store key is device name (attacker-controlled)**: The `device_name` from the Handshake message is used directly as the BTreeMap key in the trust store. An attacker can choose any name, including names of legitimate devices.

8. **Windows identity file permissions**: `DeviceIdentity::save()` only sets Unix permissions (0o600). On Windows, no ACL restrictions are applied to `identity.json`.

9. **Retry shift overflow**: `copy_with_failure_handling()` uses `1 << retry` for exponential backoff. Values > 63 would cause panic on u64 shift. Current defaults are safe but the code doesn't guard against configuration changes.

10. **Sanitize gap**: `sanitize_filename()` strips path traversal but doesn't handle Windows reserved names (`CON`, `PRN`, `NUL`, etc.) or control characters in filenames.

---

## Phase 2 Supplement: Deep Crypto Module Analysis

The following findings were produced by ultra-granular line-by-line analysis of `security/crypto.rs` (421 lines). They expand on the Phase 2 observations above with specific memory-safety and operational-security details.

### Memory Zeroization Gaps in `DeviceIdentity::save()`

1. **JSON string not zeroized (lines 145-147)**: `serde_json::to_string_pretty(&file)` produces a `String` containing the base64-encoded secret key. Standard `String` does NOT implement `Zeroize` -- the heap allocation persists in memory after deallocation until overwritten by a future allocation. If the process is memory-dumped, the secret is recoverable.

2. **IdentityFile struct not zeroized (lines 140-143)**: The `IdentityFile.secret_key` field is a `String` (base64 secret key). When `file` goes out of scope, the string is deallocated but not zeroed. Same exposure as above.

3. **JSON string read from disk not zeroized (line 90 in `load_or_create`)**: The `data` variable from `fs::read_to_string()` holds the raw JSON with the base64 secret key. Never explicitly zeroed.

**Recommendation**: Use `zeroize::Zeroizing<String>` or derive `Zeroize` + `ZeroizeOnDrop` on `IdentityFile`.

### Temporary File Persistence on Error

In `save()` (lines 149-165), if `set_permissions()` (Unix) or `rename()` fails after `fs::write()` succeeds, the temporary file `identity.json.tmp` is left on disk containing the full secret key. The filename is predictable.

**Recommendation**: Use `tempfile::NamedTempFile` with automatic cleanup, or add a cleanup guard.

### `DeviceIdentity::drop()` Implementation Details

- The `write_volatile` loop (lines 63-68) zeroes byte 0 twice (once explicitly at line 65, then again in the `0..32` loop). Redundant but harmless.
- The approach is sound: `write_volatile` maps to LLVM's `volatile` store instruction, which cannot be optimized away.
- However, `x25519_dalek::StaticSecret` may implement `Zeroize` in newer versions. If so, deriving `ZeroizeOnDrop` on `DeviceIdentity` and removing the manual unsafe block would be cleaner.

### `EncryptedChannel::complete()` -- SharedSecret Lifetime

- The `SharedSecret` from `diffie_hellman()` (line 220) is not explicitly zeroed. It relies on implicit `Drop` behavior from x25519-dalek, which does implement `Zeroize` on `SharedSecret`.
- The derived key IS explicitly zeroed at line 228 (`derived_key.zeroize()`).
- **Recommendation**: Add explicit `shared.zeroize()` for clarity and defense-in-depth.

### `load_or_create()` TOCTOU and Error Handling

- Between `path.exists()` (line 89) and `fs::read_to_string()`, another process could delete or modify the file. The read will fail gracefully (error path), so this is low severity.
- `unwrap_or_default()` on public key base64 decode (line 115) silently returns empty `Vec` on invalid base64, which will always fail the comparison. Correct behavior but not explicit -- should return an error.
- **TOCTOU on create path**: Between checking `!exists()` and calling `save()`, another process could create the file. Both processes would succeed but with different identities. Fix: use `OpenOptions::create_new(true)`.

## Phase 2 Supplement: Deep Receiver Trust Boundary Analysis

### TOFU Device Name Collision Attack (HIGH severity)

**Location**: `receiver.rs:188-200` (`handle_connection()`)

Concrete attack scenario:
1. Device `"alice-laptop"` with key `KEY_A` is trusted and saved in victim's trust store.
2. Attacker connects as `"alice-laptop"` with key `KEY_B`. This is `TrustStatus::Unknown` (different name+key combo -- actually it's `KeyChanged` since name matches but key differs).

**Correction**: The `KeyChanged` path correctly rejects the connection. However, the auto-trust path (line 196) has a subtler issue: if the attacker uses a *new* device name (e.g., `"alice-laptop2"`), they are auto-trusted. The issue is that auto-trust writes to the trust store without user confirmation, polluting it with attacker-controlled entries. On a hostile LAN, an attacker can flood the trust store with entries.

### Optional Checksum in Unencrypted Mode (MODERATE)

**Location**: `receiver.rs:432-453`

When a sender omits the BLAKE3 checksum (`FileHeader { checksum: None, ... }`), the receiver **skips all integrity verification** in unencrypted mode. A MITM on the network can flip bits without detection. In encrypted mode, Poly1305 authentication provides integrity, so this only affects unencrypted transfers.

**Recommendation**: Require checksum for unencrypted mode; make it optional only for encrypted transfers where Poly1305 already provides authentication.

### TOCTOU Symlink Attack Scenario

**Location**: `receiver.rs:840-871` (`find_unique_path()`) and `receiver.rs:456` (`fs::write()`)

Detailed attack:
1. Attacker creates symlink: `output_dir/secret.txt` -> `/etc/passwd`
2. Receiver calls `find_unique_path("secret.txt")`, which returns `output_dir/secret.txt` (exists check passes, but it's a symlink)
3. Actually: `find_unique_path` would see the symlink as existing and try `secret_1.txt`. BUT if the attacker times the symlink creation AFTER `find_unique_path` returns but BEFORE `fs::write`, the attack succeeds.

**Fix**: Use `OpenOptions::new().write(true).create_new(true)` for atomic create-if-not-exists. Also add symlink detection before write.

### Receiver Binds All Interfaces

`start_receiver()` binds `0.0.0.0:{port}` -- all network interfaces. No source IP filtering. On a multi-homed machine connected to both trusted and untrusted networks, attackers on any interface can connect.

### Code-Phrase Mode: Implicit Authentication

In `receive_with_code()`, the code phrase is NOT explicitly verified during the handshake. Authentication is implicit: the code phrase is only used for mDNS discovery (hash matching). The actual encrypted channel uses ephemeral X25519 with no binding to the code phrase. An attacker who discovers the sender via mDNS (by learning the code hash) can complete the key exchange -- they just won't be able to send valid encrypted chunks without knowing the legitimate sender's ephemeral key. This is secure against passive attackers but the authentication semantics should be documented explicitly.

---

## Phase 2 Supplement: Deep Trust Store & Codephrase Analysis

### Trust Store (`trust.rs`) -- Additional Findings

1. **Silent corruption recovery is HIGH severity (line 68-80)**: When `trusted_devices.json` is corrupted, `load()` logs a warning and returns an empty store. This means an attacker who can modify the filesystem can reset all trust relationships, forcing re-TOFU of all devices. The attacker can then present impersonated keys during re-verification. **Recommendation**: Return `Err` on corruption; require explicit `--reset-trust` flag to recover. Log as ERROR, not WARN.

2. **Unrestricted key replacement in `add_device()` (line 144)**: Any caller can replace an existing device's public key without audit trail or user confirmation. After a `KeyChanged` detection, a buggy or compromised caller could silently call `add_device()` with an attacker's key, overwriting the legitimate key. **Recommendation**: Add an `overwrite_key` boolean parameter; require separate confirmation for key updates.

3. **No input validation in `add_device()`**: No check that `public_key` is valid base64 (44 chars for X25519), or that `name` is non-empty. An attacker-controlled device name could be arbitrary Unicode. **Recommendation**: Validate key format and restrict names to ASCII.

4. **Windows rename atomicity in `save()` (line 102)**: `std::fs::rename()` on Windows fails if the destination already exists (`AlreadyExists` error). Concurrent calls to `save()` will fail. **Recommendation**: Use remove-then-rename pattern on Windows, or use platform-specific atomic APIs.

5. **No file permission verification on `load()`**: The trust store file permissions are never checked. On Unix, a world-readable `trusted_devices.json` leaks which devices the user trusts. **Recommendation**: Verify 0o600 permissions on load (Unix).

6. **Device name BTreeMap lookup is timing-dependent**: `devices.get(device_name)` is O(log n) and timing reveals whether a name exists. Since device names are typically public (mDNS hostnames), this is low severity.

### Codephrase (`codephrase.rs`) -- Additional Findings

1. **Entropy is 37.2 bits, not 38**: The comment says "~38 bits" but actual calculation is log2(9000) + 3*log2(256) = 13.14 + 24 = 37.14 bits. Documentation fix needed.

2. **Rate limiting absent on receiver side**: With 37 bits and no rate limiting, an attacker could attempt ~1.5 million guesses in the 5-minute code-phrase window. At typical network speeds this is infeasible, but on a fast LAN it could narrow the search space. The receiver (`receive_with_code`) does not implement connection rate limiting or lockout. **Recommendation**: Add exponential backoff or lockout after N failed code-phrase connections.

3. **Word list quality is good**: All 256 words are unique, lowercase ASCII, 2-5 chars. No obvious homophones. CSPRNG quality from `rand::rng()` (ChaCha20-based) is sound.

---

## Appendix: Key Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `PROTOCOL_VERSION` | 1 | `protocol.rs:12` | Wire protocol version |
| `MAX_FRAME_SIZE` | 2 MB | `protocol.rs:19` | Max bincode frame / deser limit |
| `CHUNK_SIZE` | 256 KB | `protocol.rs:26` | Data chunk payload size |
| `MAX_RECEIVE_SIZE` | 4 GB | `receiver.rs:834` | Max file size receiver accepts |
| `KDF_CONTEXT` | `"flux v1 xchacha20poly1305 session key"` | `crypto.rs:28` | BLAKE3 KDF domain separation |
| `SERVICE_TYPE` | `"_flux._tcp.local."` | `service.rs` | mDNS service type |
| `DEFAULT_PORT` | 9741 | `service.rs` | Default TCP port |
| `WORD_LIST` | 256 words | `codephrase.rs:14` | Code phrase dictionary |

## Appendix: Cryptographic Primitives

| Primitive | Algorithm | Library | Usage |
|-----------|-----------|---------|-------|
| Key exchange | X25519 | `x25519-dalek` | Ephemeral + static DH |
| AEAD cipher | XChaCha20-Poly1305 | `chacha20poly1305` | Per-chunk encryption |
| KDF | BLAKE3 `derive_key` | `blake3` | Domain-separated key derivation |
| Integrity | BLAKE3 hash | `blake3` | File checksums, code-phrase hash |
| CSPRNG | OsRng | `chacha20poly1305::aead::OsRng` | Key generation, nonce generation |
| Constant-time eq | `ct_eq` | `subtle` | Trust store key comparison |
| Key zeroing | `write_volatile` + `zeroize` | manual + `zeroize` | Secret material cleanup |
| Password hashing | -- | -- | Not used (no password auth) |

## Appendix: File Sensitivity Map

| File | On Disk | Permissions | Sensitivity |
|------|---------|-------------|-------------|
| `identity.json` | Config dir | 0o600 (Unix only) | **Critical** -- X25519 private key |
| `trusted_devices.json` | Config dir | Default | **High** -- trust decisions |
| `config.toml` | Config dir | Default | **Low** -- user preferences |
| `aliases.toml` | Config dir | Default | **Low** -- may contain server URIs |
| `history.json` | Data dir | Default | **Medium** -- transfer history (credentials stripped) |
| `queue.json` | Data dir | Default | **Low** -- pending transfer queue |
| `*.flux-resume` | Alongside dest | Default | **Low** -- resume manifest (no file content) |
