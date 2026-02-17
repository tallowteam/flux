# Phase 5: Discovery & Security - Research

**Researched:** 2026-02-16
**Domain:** LAN service discovery (mDNS/DNS-SD), peer-to-peer TCP file transfer protocol, end-to-end encryption (X25519 + ChaCha20-Poly1305), trust-on-first-use device authentication
**Confidence:** HIGH

## Summary

Phase 5 introduces three major subsystems to Flux: (1) LAN device discovery via mDNS/DNS-SD so users can find other Flux instances on their network, (2) a custom TCP-based send/receive protocol enabling peer-to-peer file transfers between Flux instances, and (3) optional end-to-end encryption with trust-on-first-use (TOFU) device authentication.

The Rust ecosystem has mature, well-maintained libraries for all three domains. For mDNS, the `mdns-sd` crate (v0.18.0) provides both client browsing and server registration without async runtime dependencies, working cross-platform on Windows, Linux, and macOS. For encryption, the RustCrypto ecosystem provides `x25519-dalek` (v2.0.1) for key exchange and `chacha20poly1305` (v0.10.1) for authenticated encryption -- the same primitives used by the Noise protocol, WireGuard, and TLS 1.3. The existing `tokio` dependency (already at v1 with `full` features) provides `TcpListener`/`TcpStream` for the transfer protocol, and `tokio-util` provides `LengthDelimitedCodec` for message framing.

The architecture adds three new top-level modules (`src/discovery/`, `src/security/`, `src/net/`) alongside new CLI subcommands (`discover`, `send`, `receive`, `trust`). The existing `FluxBackend` trait is NOT extended -- the send/receive protocol operates independently as a peer-to-peer transfer mechanism, using its own framed TCP protocol rather than routing through the backend abstraction. This is the correct design because `FluxBackend` models filesystem-like access (stat, list_dir, open_read, open_write), whereas send/receive is a push-based transfer protocol between two cooperating Flux instances.

**Primary recommendation:** Use `mdns-sd` for discovery, `tokio` TCP for the transfer protocol with length-delimited framing and bincode serialization, and `x25519-dalek` + `chacha20poly1305` for optional encryption. Implement TOFU by storing device public keys in a JSON trust store file.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| DISC-01 | User can discover devices on LAN via mDNS/Bonjour | `mdns-sd` crate provides `ServiceDaemon::browse()` for querying `_flux._tcp.local.` services. Cross-platform (Win/Linux/macOS). No async runtime needed. |
| DISC-02 | User can see discovered devices with friendly names | `ServiceInfo` carries TXT properties where friendly name is stored. `gethostname` crate provides default device name. |
| DISC-03 | Tool can receive transfers from other Flux instances | Tokio `TcpListener` binds on a configurable port. `flux receive` command starts listener, registers mDNS service, and accepts incoming framed TCP connections. |
| SEC-01 | User can enable optional end-to-end encryption | X25519 key exchange produces shared secret; XChaCha20-Poly1305 AEAD encrypts each data frame. Activated via `--encrypt` flag. |
| SEC-02 | Tool uses trust-on-first-use for device authentication | First connection: display peer public key fingerprint, prompt user to accept. Accepted keys stored in `trusted_devices.json`. Subsequent connections: verify peer key matches stored key. |
| SEC-03 | User can view and manage trusted devices | `flux trust list` / `flux trust rm <device>` commands. Trust store is a simple JSON file in config directory. |
| CLI-03 | User can run send/receive commands | New `Commands::Send`, `Commands::Receive`, `Commands::Discover`, `Commands::Trust` variants in CLI args. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `mdns-sd` | 0.18.0 | mDNS service discovery (register + browse) | Pure Rust, no async runtime dependency, supports client + server, cross-platform (Win/Linux/macOS), actively maintained (68 releases), tested against Avahi/Bonjour |
| `chacha20poly1305` | 0.10.1 | AEAD authenticated encryption | RustCrypto project, constant-time, AVX2 acceleration, XChaCha20 variant with 192-bit nonces (safe random generation) |
| `x25519-dalek` | 2.0.1 | Elliptic curve Diffie-Hellman key exchange | Standard ECDH library, pure Rust, used widely in Noise/WireGuard implementations |
| `tokio` | 1 (existing) | Async TCP listener/stream for receive server | Already a dependency with `full` features. Provides TcpListener, TcpStream. |
| `tokio-util` | 0.7 | LengthDelimitedCodec for message framing | Official tokio companion crate, handles frame boundaries over TCP streams |
| `bincode` | 2.0 | Binary serialization of protocol messages | Compact, fast, zero-copy where possible, serde-compatible. Version 2 makes serde optional. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `gethostname` | 0.5 | Get machine hostname for friendly device name | Default device name in mDNS registration and display |
| `rand` | 0.9 | Cryptographic RNG for key generation and nonces | Required by x25519-dalek and chacha20poly1305 for key/nonce generation |
| `base64` | 0.22 | Encode public key fingerprints for display | TOFU fingerprint display to user |
| `futures` | 0.3 | Stream combinators for async protocol handling | Processing framed TCP message streams |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `mdns-sd` | `simple-mdns` | simple-mdns has sync and async variants but fewer downloads, less battle-tested |
| `mdns-sd` | `zeroconf` | Wraps system Bonjour/Avahi but requires native libraries installed |
| `mdns-sd` | `libmdns` | Server (responder) only -- no client browsing capability |
| `chacha20poly1305` | `aes-gcm` | AES-GCM requires hardware AES-NI for performance; ChaCha20 is fast in software |
| `bincode` | `postcard` | Postcard is designed for embedded/no_std; bincode is more established for network protocols |
| `bincode` | `serde_json` | JSON adds ~40% overhead for binary data framing; bincode is compact |
| Custom TCP | QUIC (`quinn`) | QUIC adds complexity (UDP, connection management); TCP is sufficient for LAN transfers |

**Installation:**
```bash
cargo add mdns-sd@0.18 chacha20poly1305@0.10 x25519-dalek@2 --features x25519-dalek/static_secrets
cargo add tokio-util@0.7 --features codec
cargo add bincode@2 --features serde
cargo add gethostname@0.5 rand@0.9 base64@0.22 futures@0.3
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── discovery/
│   ├── mod.rs           # pub mod mdns, service
│   ├── mdns.rs          # mDNS registration and browsing (wraps mdns-sd)
│   └── service.rs       # FluxService type, device identity
├── security/
│   ├── mod.rs           # pub mod crypto, trust
│   ├── crypto.rs        # Key generation, X25519 exchange, encrypt/decrypt frames
│   └── trust.rs         # TrustStore: load/save/verify trusted device keys
├── net/
│   ├── mod.rs           # pub mod protocol, sender, receiver
│   ├── protocol.rs      # Message types (Handshake, FileHeader, DataChunk, Ack, etc.)
│   ├── sender.rs        # Connect to peer, send file(s) via framed protocol
│   └── receiver.rs      # Listen for connections, receive file(s), write to disk
├── cli/
│   └── args.rs          # Extended with Send, Receive, Discover, Trust commands
├── main.rs              # Extended with new command dispatch
└── ... (existing modules unchanged)
```

### Pattern 1: Framed TCP Protocol with Length-Delimited Messages
**What:** All messages between Flux peers are serialized with bincode and framed with a 4-byte big-endian length prefix using `tokio-util::codec::LengthDelimitedCodec`.
**When to use:** All peer-to-peer communication over TCP.
**Example:**
```rust
// Source: tokio-util docs (https://docs.rs/tokio-util/latest/tokio_util/codec/length_delimited/)
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use tokio::net::TcpStream;
use futures::{SinkExt, StreamExt};

// Wrap a TCP stream with length-delimited framing
let stream = TcpStream::connect("192.168.1.50:9741").await?;
let mut framed = Framed::new(stream, LengthDelimitedCodec::new());

// Send a message (bincode-serialized bytes)
let msg = bincode::serialize(&FluxMessage::Handshake { ... })?;
framed.send(msg.into()).await?;

// Receive a message
if let Some(Ok(bytes)) = framed.next().await {
    let msg: FluxMessage = bincode::deserialize(&bytes)?;
}
```

### Pattern 2: mDNS Service Registration and Discovery
**What:** Each Flux receiver registers a `_flux._tcp.local.` mDNS service with its port and device name. Discoverers browse for this service type.
**When to use:** `flux discover` command and `flux receive` startup.
**Example:**
```rust
// Source: mdns-sd docs (https://docs.rs/mdns-sd/latest/mdns_sd/)
use mdns_sd::{ServiceDaemon, ServiceInfo, ServiceEvent};

const SERVICE_TYPE: &str = "_flux._tcp.local.";

// Registration (receiver side)
let mdns = ServiceDaemon::new()?;
let service = ServiceInfo::new(
    SERVICE_TYPE,
    "my-laptop",           // instance name (friendly)
    "my-laptop.local.",    // hostname
    "",                    // auto-detect IP
    9741,                  // port
    &[("version", "0.1"), ("pubkey", "<base64-key>")],
)?
.enable_addr_auto();
mdns.register(service)?;

// Discovery (discover side)
let mdns = ServiceDaemon::new()?;
let receiver = mdns.browse(SERVICE_TYPE)?;
while let Ok(event) = receiver.recv_timeout(Duration::from_secs(5)) {
    match event {
        ServiceEvent::ServiceResolved(info) => {
            println!("Found: {} at {}:{}", info.get_fullname(),
                     info.get_addresses().iter().next().unwrap(),
                     info.get_port());
        }
        _ => {}
    }
}
```

### Pattern 3: X25519 Key Exchange + XChaCha20-Poly1305 Encryption
**What:** Peers exchange ephemeral X25519 public keys during handshake. The shared secret is used to derive an XChaCha20-Poly1305 key for encrypting all subsequent data frames.
**When to use:** When `--encrypt` flag is set on send/receive.
**Example:**
```rust
// Source: x25519-dalek docs (https://docs.rs/x25519-dalek/latest/x25519_dalek/)
// Source: chacha20poly1305 docs (https://docs.rs/chacha20poly1305/latest/)
use x25519_dalek::{EphemeralSecret, PublicKey};
use chacha20poly1305::{XChaCha20Poly1305, aead::{Aead, KeyInit, OsRng}};

// Key exchange
let secret = EphemeralSecret::random_from_rng(OsRng);
let public = PublicKey::from(&secret);
// Send public key to peer, receive peer's public key
let shared_secret = secret.diffie_hellman(&peer_public);

// Derive encryption key (shared_secret.as_bytes() is 32 bytes = 256 bits)
let cipher = XChaCha20Poly1305::new(shared_secret.as_bytes().into());

// Encrypt a data frame
let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
let ciphertext = cipher.encrypt(&nonce, plaintext_data)?;

// Decrypt a data frame
let plaintext = cipher.decrypt(&nonce, ciphertext.as_ref())?;
```

### Pattern 4: Trust-on-First-Use (TOFU) Device Store
**What:** Device identity is a long-lived X25519 static key pair. On first connection, the user approves the peer's public key. On subsequent connections, the stored key is verified against the peer's presented key.
**When to use:** All encrypted peer-to-peer connections.
**Example:**
```rust
// Trust store format (trusted_devices.json in config dir)
{
    "devices": {
        "alice-laptop": {
            "public_key": "base64-encoded-32-bytes",
            "first_seen": "2026-02-16T10:30:00Z",
            "last_seen": "2026-02-16T14:22:00Z",
            "friendly_name": "Alice's Laptop"
        }
    }
}
```

### Pattern 5: Protocol Message Types
**What:** Define a small set of message types for the transfer protocol.
**When to use:** All peer communication.
**Example:**
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub enum FluxMessage {
    /// Initial handshake: identify, negotiate encryption
    Handshake {
        version: u8,
        device_name: String,
        public_key: Option<[u8; 32]>,  // Present when encryption requested
    },
    /// Handshake response: accept or reject
    HandshakeAck {
        accepted: bool,
        public_key: Option<[u8; 32]>,  // Peer's key for key exchange
        reason: Option<String>,
    },
    /// File metadata before data transfer
    FileHeader {
        filename: String,
        size: u64,
        checksum: Option<String>,  // BLAKE3 hash if --verify
        encrypted: bool,
    },
    /// File data chunk
    DataChunk {
        offset: u64,
        data: Vec<u8>,
        nonce: Option<[u8; 24]>,  // XChaCha20 nonce when encrypted
    },
    /// Transfer complete acknowledgement
    TransferComplete {
        filename: String,
        bytes_received: u64,
        checksum_verified: Option<bool>,
    },
    /// Error during transfer
    Error {
        message: String,
    },
}
```

### Anti-Patterns to Avoid
- **Blocking mDNS in the main thread:** The mdns-sd daemon runs its own thread; never call blocking operations on the daemon thread. Use the receiver channel pattern.
- **Reusing nonces:** Each encrypted frame MUST have a unique nonce. Use `XChaCha20Poly1305::generate_nonce()` with OsRng for each frame -- the 192-bit nonce space makes random nonce collision astronomically unlikely.
- **Raw TCP without framing:** TCP is a byte stream, not a message stream. Always use LengthDelimitedCodec or equivalent to handle message boundaries.
- **Storing private keys in plain text:** Device static private keys should be stored with filesystem permissions (0600) and ideally in the OS keychain, but for v1, file-based storage with restricted permissions is acceptable.
- **Mixing sync and async:** The existing codebase is primarily synchronous. The receiver server needs tokio async for TCP, but discovery and trust management can remain sync. Use `tokio::runtime::Runtime::new()` or the existing `#[tokio::main]` approach for the async boundary.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| mDNS packet construction | Custom UDP multicast with DNS record parsing | `mdns-sd` ServiceDaemon | mDNS is deceptively complex: packet compression, TTL, cache invalidation, multi-interface, IPv4+IPv6 |
| Message framing over TCP | Manual length-prefix parsing with byte buffers | `tokio-util` LengthDelimitedCodec | Edge cases: partial reads, buffer management, max frame size limits |
| Authenticated encryption | Custom encrypt-then-MAC or rolling your own AEAD | `chacha20poly1305` XChaCha20Poly1305 | Cryptographic code must be constant-time; any mistake is a security vulnerability |
| Key exchange | Manual Diffie-Hellman with raw math | `x25519-dalek` EphemeralSecret/PublicKey | Side-channel attacks, constant-time operations, validated implementations |
| Binary serialization | Custom byte packing/unpacking for protocol messages | `bincode` with serde Serialize/Deserialize | Endianness, alignment, backwards compatibility, fuzzing-resistant parsing |
| Hostname detection | `std::process::Command("hostname")` | `gethostname` crate | Cross-platform differences (Windows vs Unix), OsString handling |

**Key insight:** This phase touches cryptography and network protocols -- two domains where hand-rolled solutions are not just slower to build, but actively dangerous. Use audited, established libraries for all crypto operations.

## Common Pitfalls

### Pitfall 1: Firewall Blocking mDNS and Transfer Port
**What goes wrong:** mDNS uses UDP multicast on port 5353. The transfer server uses a TCP port. Both may be blocked by OS firewalls (Windows Firewall, macOS firewall, iptables).
**Why it happens:** Default firewall rules often block incoming connections and multicast.
**How to avoid:** Document firewall requirements clearly. Use a well-known port (e.g., 9741) for the transfer server. On Windows, the first run may trigger a firewall prompt -- handle this gracefully with clear error messages. Consider falling back to direct IP:port connection if mDNS discovery fails.
**Warning signs:** `flux discover` finds nothing; `flux receive` starts but nobody can connect.

### Pitfall 2: Async/Sync Boundary Confusion
**What goes wrong:** The existing codebase is synchronous. The TCP server for `flux receive` needs async (tokio). Mixing sync and async incorrectly causes panics ("Cannot start a runtime from within a runtime") or deadlocks.
**Why it happens:** Tokio runtime cannot be nested. Calling `.block_on()` inside an async context panics.
**How to avoid:** Create the tokio runtime at the top level in `main.rs` for the `receive` command. Use `#[tokio::main]` or `Runtime::new()` exclusively at the entry point. Keep discovery sync (mdns-sd handles its own threading). For `send`, use a short-lived runtime for the TCP connection.
**Warning signs:** "Cannot start a runtime from within a runtime" panic.

### Pitfall 3: mDNS Service Type Must End with `.local.`
**What goes wrong:** Service discovery silently fails or returns no results.
**Why it happens:** RFC 6763 requires service types in the format `_service._tcp.local.` including the trailing dot and `.local.` suffix.
**How to avoid:** Define `SERVICE_TYPE` as a constant: `const SERVICE_TYPE: &str = "_flux._tcp.local.";` and use it everywhere.
**Warning signs:** Browse returns no events even when services are registered on the same network.

### Pitfall 4: Large File Transfer Memory Pressure
**What goes wrong:** Encrypting entire files in memory before sending causes OOM on large files.
**Why it happens:** AEAD encryption operates on discrete chunks. Naively encrypting the whole file at once requires holding the entire file in memory.
**How to avoid:** Encrypt per-chunk (e.g., 256KB or 1MB chunks). Each chunk gets its own nonce. The DataChunk message carries the encrypted chunk + nonce. This enables streaming encryption with bounded memory usage.
**Warning signs:** Memory usage spikes proportional to file size during encrypted transfers.

### Pitfall 5: Device Identity Key Loss/Rotation
**What goes wrong:** If the device identity key file is deleted or corrupted, all TOFU relationships break (other devices no longer recognize this device).
**Why it happens:** The TOFU model binds trust to a specific public key. If the key changes, peers see a "key mismatch" warning.
**How to avoid:** Store identity keys in the config directory with a clear filename (`identity_key.json`). Provide a `flux trust reset` command that generates new keys and warns about consequences. On key mismatch, show a clear SSH-style warning ("WARNING: DEVICE IDENTIFICATION HAS CHANGED!").
**Warning signs:** Users reporting "device not trusted" errors after reinstall or config migration.

### Pitfall 6: Port Conflicts on Receiver
**What goes wrong:** `flux receive` fails to bind because another instance or application is using the port.
**Why it happens:** Fixed port without fallback.
**How to avoid:** Use a default port (9741) but support `--port` flag. If the default port is occupied, try a few alternatives or use port 0 (OS-assigned) and advertise the actual port via mDNS. Always report the actual bound port to the user.
**Warning signs:** "Address already in use" error on `flux receive`.

## Code Examples

Verified patterns from official sources:

### mDNS Service Registration (Receiver Side)
```rust
// Source: https://docs.rs/mdns-sd/latest/mdns_sd/
use mdns_sd::{ServiceDaemon, ServiceInfo};
use gethostname::gethostname;

const SERVICE_TYPE: &str = "_flux._tcp.local.";
const DEFAULT_PORT: u16 = 9741;

pub fn register_flux_service(port: u16, device_name: &str) -> Result<ServiceDaemon, FluxError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to create mDNS daemon: {}", e)))?;

    let hostname = gethostname().to_string_lossy().to_string();
    let host_label = format!("{}.local.", hostname);

    let service = ServiceInfo::new(
        SERVICE_TYPE,
        device_name,
        &host_label,
        "",  // auto-detect IP addresses
        port,
        &[("version", env!("CARGO_PKG_VERSION"))],
    )
    .map_err(|e| FluxError::DiscoveryError(format!("Invalid service info: {}", e)))?
    .enable_addr_auto();

    mdns.register(service)
        .map_err(|e| FluxError::DiscoveryError(format!("Failed to register service: {}", e)))?;

    Ok(mdns)
}
```

### mDNS Service Discovery (Discover Command)
```rust
// Source: https://docs.rs/mdns-sd/latest/mdns_sd/
use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::time::Duration;

pub fn discover_flux_devices(timeout_secs: u64) -> Result<Vec<DiscoveredDevice>, FluxError> {
    let mdns = ServiceDaemon::new()
        .map_err(|e| FluxError::DiscoveryError(format!("{}", e)))?;

    let receiver = mdns.browse(SERVICE_TYPE)
        .map_err(|e| FluxError::DiscoveryError(format!("{}", e)))?;

    let mut devices = Vec::new();
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs);

    while std::time::Instant::now() < deadline {
        match receiver.recv_timeout(Duration::from_millis(500)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let addrs: Vec<_> = info.get_addresses().iter().copied().collect();
                if let Some(addr) = addrs.first() {
                    devices.push(DiscoveredDevice {
                        name: info.get_fullname().to_string(),
                        host: addr.to_string(),
                        port: info.get_port(),
                        version: info.get_properties()
                            .get("version")
                            .map(|v| v.val_str().to_string()),
                    });
                }
            }
            Ok(_) => {} // SearchStarted, ServiceFound (unresolved), etc.
            Err(_) => {} // Timeout, continue
        }
    }

    mdns.shutdown().ok();
    Ok(devices)
}
```

### TCP Transfer Protocol Setup
```rust
// Source: https://docs.rs/tokio-util/latest/tokio_util/codec/length_delimited/
use tokio::net::{TcpListener, TcpStream};
use tokio_util::codec::{Framed, LengthDelimitedCodec};
use futures::{SinkExt, StreamExt};

// Receiver: listen for incoming connections
async fn start_receiver(port: u16) -> Result<(), FluxError> {
    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await
        .map_err(|e| FluxError::Io { source: e })?;

    eprintln!("Listening on port {}...", port);

    loop {
        let (stream, peer_addr) = listener.accept().await
            .map_err(|e| FluxError::Io { source: e })?;
        eprintln!("Connection from {}", peer_addr);

        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                eprintln!("Transfer error: {}", e);
            }
        });
    }
}

async fn handle_connection(stream: TcpStream) -> Result<(), FluxError> {
    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(2 * 1024 * 1024)  // 2MB max frame
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // Read handshake
    // ... protocol handling
    Ok(())
}

// Sender: connect to peer
async fn send_to_peer(host: &str, port: u16) -> Result<(), FluxError> {
    let stream = TcpStream::connect(format!("{}:{}", host, port)).await
        .map_err(|e| FluxError::ConnectionFailed {
            protocol: "flux".to_string(),
            host: host.to_string(),
            reason: e.to_string(),
        })?;

    let codec = LengthDelimitedCodec::builder()
        .max_frame_length(2 * 1024 * 1024)
        .new_codec();
    let mut framed = Framed::new(stream, codec);

    // Send handshake, file headers, data chunks...
    Ok(())
}
```

### Encryption Setup
```rust
// Source: https://docs.rs/x25519-dalek/latest/x25519_dalek/
// Source: https://docs.rs/chacha20poly1305/latest/chacha20poly1305/
use x25519_dalek::{EphemeralSecret, PublicKey};
use chacha20poly1305::{XChaCha20Poly1305, aead::{Aead, KeyInit, OsRng}};

pub struct EncryptedChannel {
    cipher: XChaCha20Poly1305,
}

impl EncryptedChannel {
    /// Perform key exchange and create encrypted channel.
    /// Returns (channel, our_public_key) -- send public key to peer.
    pub fn initiate() -> (EphemeralSecret, PublicKey) {
        let secret = EphemeralSecret::random_from_rng(OsRng);
        let public = PublicKey::from(&secret);
        (secret, public)
    }

    /// Complete key exchange with peer's public key.
    pub fn complete(secret: EphemeralSecret, peer_public: &PublicKey) -> Self {
        let shared = secret.diffie_hellman(peer_public);
        let cipher = XChaCha20Poly1305::new(shared.as_bytes().into());
        Self { cipher }
    }

    /// Encrypt a data chunk. Returns (ciphertext, nonce).
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<(Vec<u8>, [u8; 24]), FluxError> {
        let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
        let ciphertext = self.cipher.encrypt(&nonce, plaintext)
            .map_err(|e| FluxError::EncryptionError(format!("Encrypt failed: {}", e)))?;
        Ok((ciphertext, nonce.into()))
    }

    /// Decrypt a data chunk.
    pub fn decrypt(&self, ciphertext: &[u8], nonce: &[u8; 24]) -> Result<Vec<u8>, FluxError> {
        self.cipher.decrypt(nonce.into(), ciphertext)
            .map_err(|e| FluxError::EncryptionError(format!("Decrypt failed: {}", e)))
    }
}
```

### Trust Store Management
```rust
use serde::{Serialize, Deserialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Serialize, Deserialize, Default)]
pub struct TrustStore {
    pub devices: BTreeMap<String, TrustedDevice>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct TrustedDevice {
    pub public_key: String,  // base64-encoded
    pub first_seen: chrono::DateTime<chrono::Utc>,
    pub last_seen: chrono::DateTime<chrono::Utc>,
    pub friendly_name: String,
}

impl TrustStore {
    pub fn load(config_dir: &Path) -> Result<Self, FluxError> {
        let path = config_dir.join("trusted_devices.json");
        if path.exists() {
            let data = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&data)?)
        } else {
            Ok(Self::default())
        }
    }

    pub fn is_trusted(&self, device_name: &str, public_key: &[u8; 32]) -> TrustStatus {
        match self.devices.get(device_name) {
            None => TrustStatus::Unknown,
            Some(device) => {
                let stored_key = base64::decode(&device.public_key).unwrap_or_default();
                if stored_key == public_key.as_ref() {
                    TrustStatus::Trusted
                } else {
                    TrustStatus::KeyChanged  // WARNING: possible impersonation
                }
            }
        }
    }
}

pub enum TrustStatus {
    Trusted,
    Unknown,
    KeyChanged,
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| System Bonjour/Avahi wrappers (zeroconf) | Pure Rust mDNS (mdns-sd) | 2023+ | No system dependency, simpler cross-compilation |
| AES-GCM for symmetric encryption | ChaCha20-Poly1305 | WireGuard/TLS 1.3 era | Better software performance without AES-NI, simpler implementation |
| Manual key exchange (RSA) | X25519 ECDH | Signal Protocol era | 32-byte keys, faster, simpler, forward secrecy |
| Custom binary protocols | Length-delimited + bincode | tokio-util maturity | Standard framing, less error-prone |
| x25519-dalek 1.x | x25519-dalek 2.0.1 | 2024-02 | API changes: `random_from_rng()` replaces `new()`, `static_secrets` feature flag |
| bincode 1.x | bincode 2.0 | 2024+ | Serde now optional, new API with `encode`/`decode`, migration needed if using v1 patterns |

**Deprecated/outdated:**
- `rust-crypto` crate: Abandoned. Use RustCrypto org crates instead.
- `x25519-dalek` 1.x: Significant API changes in 2.x. Use 2.0.1.
- `bincode` 1.x: Still works but v2 is the current recommended version.

## Open Questions

1. **Port Selection Strategy**
   - What we know: A fixed default port (e.g., 9741) is simple. mDNS advertises the actual port.
   - What's unclear: Should we use port 0 (OS-assigned) by default for maximum compatibility, or a fixed port for firewall rule predictability?
   - Recommendation: Use a fixed default (9741) with `--port` override and clear documentation. If binding fails, suggest the `--port` flag in the error message.

2. **Async Runtime Integration**
   - What we know: The existing codebase is synchronous with `tokio` as a dependency. `flux receive` needs async for concurrent connections.
   - What's unclear: Whether to convert `main()` to `#[tokio::main]` or use `Runtime::new()` only for commands that need it.
   - Recommendation: Use `Runtime::new()` locally for `send` and `receive` commands only. This avoids changing the sync nature of all other commands. The `main()` function stays sync.

3. **Device Identity Key Generation Timing**
   - What we know: TOFU requires a persistent device identity key pair.
   - What's unclear: Should the key be generated on first `flux receive` / first encrypted transfer, or on first run of any flux command?
   - Recommendation: Generate lazily on first use of a security feature (`receive`, `send --encrypt`). Store in config dir as `identity.json`. This avoids generating keys for users who never use discovery/security features.

4. **Transfer Protocol Direction**
   - What we know: The success criteria shows `flux send file.txt @devicename` (push model).
   - What's unclear: Whether the receiver should also support a pull model (receiver requests specific files from sender).
   - Recommendation: v1 implements push-only (sender initiates, receiver accepts). Pull model deferred to future enhancement. This matches the CLI semantics: `flux send` = push, `flux receive` = listen.

5. **Multiple File Transfers**
   - What we know: `flux send file.txt @device` sends one file. Users may want to send directories.
   - What's unclear: Should `flux send -r dir/ @device` be supported in Phase 5?
   - Recommendation: Support single files in the initial implementation. Directory send can be added as a fast follow by sending multiple FileHeader+DataChunk sequences. The protocol message design already supports this.

## Sources

### Primary (HIGH confidence)
- [mdns-sd GitHub](https://github.com/keepsimple1/mdns-sd) - v0.18.0, features, platform support, examples
- [mdns-sd docs.rs](https://docs.rs/mdns-sd/latest/mdns_sd/) - Full API: ServiceDaemon, ServiceInfo, ServiceEvent
- [chacha20poly1305 docs.rs](https://docs.rs/chacha20poly1305) - v0.10.1, API, encrypt/decrypt examples, nonce handling
- [x25519-dalek docs.rs](https://docs.rs/x25519-dalek/latest/x25519_dalek/) - v2.0.1, EphemeralSecret, PublicKey, SharedSecret, key exchange example
- [tokio-util LengthDelimitedCodec docs](https://docs.rs/tokio-util/latest/tokio_util/codec/length_delimited/) - Framing protocol
- [tokio TcpListener docs](https://docs.rs/tokio/latest/tokio/net/struct.TcpListener.html) - TCP server patterns
- [bincode docs.rs](https://docs.rs/bincode/latest/bincode/) - v2/3 serialization API

### Secondary (MEDIUM confidence)
- [Magic Wormhole file transfer protocol docs](https://magic-wormhole.readthedocs.io/en/latest/file-transfer-protocol.html) - Transit architecture pattern (direct TCP, relay fallback)
- [gethostname crates.io](https://crates.io/crates/gethostname) - Cross-platform hostname, 3M+ monthly downloads
- [Trust on first use - Wikipedia](https://en.wikipedia.org/wiki/Trust_on_first_use) - TOFU authentication scheme description

### Tertiary (LOW confidence)
- [rustic-secure-transfer GitHub](https://github.com/bivav/rustic-secure-transfer) - Reference implementation of secure P2P file transfer in Rust (project status/quality unverified)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All recommended crates are well-established, actively maintained, verified via docs.rs and GitHub. mdns-sd has 68 releases, chacha20poly1305 is RustCrypto org, x25519-dalek is dalek-cryptography org.
- Architecture: HIGH - Pattern follows established approaches (framed TCP, ECDH key exchange, AEAD encryption). Similar to WireGuard/Signal/Magic Wormhole architectures.
- Pitfalls: HIGH - Identified from real documentation, known mDNS quirks, and crypto best practices from RustCrypto docs.

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (30 days - stable domain, mature libraries)
