# Phase 2: Performance - Research

**Researched:** 2026-02-16
**Domain:** Parallel chunked file I/O, compression, checksumming, bandwidth limiting, resumable transfers
**Confidence:** HIGH

## Summary

Phase 2 transforms Flux from a simple sequential file copier into a high-performance transfer engine. The core challenge is splitting files into chunks that can be read/written in parallel, tracking chunk completion for resumability, verifying integrity with BLAKE3 checksums, optionally compressing with zstd, and throttling bandwidth. All of this must integrate with the existing synchronous `FluxBackend` trait and `copy_file_with_progress` infrastructure from Phase 1.

The Rust ecosystem has mature, battle-tested crates for every component: `blake3` for checksumming (with optional rayon-based parallelism), `zstd` for streaming compression/decompression, and simple token-bucket wrappers for bandwidth limiting. The parallel chunked I/O pattern uses `pread`/`pwrite` (Unix `read_at`/`write_at`, Windows `seek_read`/`seek_write`) which allow multiple threads to read/write different file regions concurrently without fighting over a shared file cursor. Resumability requires a persistent manifest (serde JSON) tracking per-chunk completion state.

**Primary recommendation:** Use `std::os::unix::fs::FileExt::read_at` / `std::os::windows::fs::FileExt::seek_read` for cross-platform positional I/O, `rayon` or `std::thread` for chunk parallelism, `blake3` for checksums, `zstd` for compression, and a hand-rolled token-bucket `Read`/`Write` wrapper for bandwidth limiting (simpler than pulling in a dependency for this narrow use case).

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-03 | Resume interrupted transfer from where it stopped | Chunk manifest pattern: serialize per-chunk completion state to a `.flux-manifest.json` sidecar file. On resume, read manifest, skip completed chunks, resume from first incomplete chunk. |
| CORE-05 | Verify transfer integrity via checksum (BLAKE3) | `blake3` crate v1.8 provides incremental `Hasher` with `update()` + `finalize()`. Hash each chunk individually AND compute a whole-file hash. Store hashes in manifest for resume verification. |
| CORE-08 | Enable compression for transfers (zstd) | `zstd` crate v0.13 provides `stream::Encoder`/`Decoder` wrapping `Read`/`Write`. Wrap chunk writer in `zstd::Encoder` on send side, chunk reader in `zstd::Decoder` on receive side. Level 3 (default) is good for text-heavy content. |
| PERF-01 | Transfer files using parallel chunks (configurable count) | Use positional I/O (`read_at`/`seek_read`) to read chunks from different file offsets in parallel threads. Configurable via `--chunks N` CLI flag. |
| PERF-02 | Transfer large files using multiple TCP connections | Architecture for parallel chunk I/O in Phase 2 directly enables multiple TCP connections in Phase 3. Each chunk becomes an independent transfer unit. Phase 2 establishes the `ChunkPlan` and `TransferPlan` types that Phase 3 wraps with TCP streams. |
| PERF-03 | Limit bandwidth usage (KB/s, MB/s) | Token-bucket `ThrottledReader`/`ThrottledWriter` wrappers around `std::io::Read`/`Write`. Parse human-readable bandwidth strings like `10MB/s`, `500KB/s`. |
| PERF-04 | Auto-detect optimal chunk size based on file size and network | Tiered heuristic based on file size. Small files (<10MB): no chunking. Medium (10MB-1GB): 4MB chunks. Large (1-10GB): 16MB chunks. Very large (>10GB): 64MB chunks. Cap chunk count at thread count * 2. |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| [blake3](https://crates.io/crates/blake3) | 1.8 | File/chunk integrity checksumming | Official BLAKE3 implementation. 14x faster than SHA-256. SIMD-accelerated. Incremental hashing API. Merkle tree internally (parallelizable). |
| [zstd](https://crates.io/crates/zstd) | 0.13 | Streaming compression/decompression | Bindings to Facebook's zstd. Best ratio-to-speed tradeoff. `Encoder`/`Decoder` wrap `Read`/`Write` directly. 190M+ downloads. |
| [serde_json](https://crates.io/crates/serde_json) | 1.0 | Manifest serialization for resume state | Already have `serde` in Cargo.toml. JSON manifests are human-debuggable. |
| [rayon](https://crates.io/crates/rayon) | 1.10 | Thread pool for parallel chunk I/O | De facto standard for data parallelism in Rust. Work-stealing thread pool. Integrates well with iterator patterns. |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| [bytesize](https://crates.io/crates/bytesize) | 1.3 | Parse human-readable byte sizes (10MB, 500KB) | Parsing `--limit` and displaying transfer sizes. Handles KB/MB/GB/TB. |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| rayon | std::thread::scope | Fewer dependencies, but manual work distribution. Rayon's work-stealing is better for uneven chunk sizes. |
| rayon | tokio::task::spawn_blocking | Already have tokio, but spawn_blocking's thread pool is designed for blocking I/O waits, not CPU-bound parallel work. Rayon is purpose-built for this. |
| serde_json (manifest) | bincode or TOML | JSON is human-readable for debugging resume state. Minor perf difference irrelevant for metadata. |
| bytesize | hand-rolled parser | bytesize handles edge cases (units, case sensitivity) that are annoying to get right. |
| stream_limiter crate | hand-rolled ThrottledWriter | stream_limiter has low adoption. A token-bucket Read/Write wrapper is ~50 lines and avoids a dependency for such a narrow feature. Recommend hand-rolling. |

**Installation:**
```bash
cargo add blake3
cargo add zstd
cargo add serde_json
cargo add rayon
cargo add bytesize
```

## Architecture Patterns

### Recommended Module Structure

```
src/
├── transfer/
│   ├── mod.rs              # execute_copy dispatcher (exists, extend)
│   ├── copy.rs             # copy_file_with_progress (exists, extend)
│   ├── filter.rs           # TransferFilter (exists, unchanged)
│   ├── chunk.rs            # NEW: ChunkPlan, chunk_file(), auto_chunk_size()
│   ├── parallel.rs         # NEW: parallel_copy_chunked(), per-chunk I/O
│   ├── checksum.rs         # NEW: hash_file(), hash_chunk(), verify_file()
│   ├── compress.rs         # NEW: CompressedReader/Writer wrappers
│   ├── throttle.rs         # NEW: ThrottledReader/ThrottledWriter
│   └── resume.rs           # NEW: TransferManifest, load/save, resume logic
├── cli/
│   └── args.rs             # Extend CpArgs with --chunks, --verify, --compress, --limit
└── error.rs                # Add ChecksumMismatch, ResumeError variants
```

### Pattern 1: Chunk Plan + Positional I/O

**What:** Split a file into a `Vec<ChunkPlan>` describing byte ranges, then use `read_at`/`write_at` (or `seek_read`/`seek_write` on Windows) to read/write each chunk independently from parallel threads.

**When to use:** Any file transfer where `--chunks N` is specified or auto-detected chunk count > 1.

**Example:**
```rust
use std::sync::Arc;

/// Describes one chunk of a file to transfer.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ChunkPlan {
    pub index: usize,
    pub offset: u64,
    pub length: u64,
    pub completed: bool,
    pub checksum: Option<String>,  // BLAKE3 hex hash of this chunk
}

/// Plan for transferring an entire file in chunks.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TransferPlan {
    pub source_path: String,
    pub dest_path: String,
    pub total_size: u64,
    pub file_checksum: Option<String>,  // whole-file BLAKE3
    pub chunks: Vec<ChunkPlan>,
}

/// Split a file into N chunks.
pub fn chunk_file(total_size: u64, chunk_count: usize) -> Vec<ChunkPlan> {
    let chunk_size = total_size / chunk_count as u64;
    let remainder = total_size % chunk_count as u64;
    let mut chunks = Vec::with_capacity(chunk_count);
    let mut offset = 0u64;

    for i in 0..chunk_count {
        // Last chunk absorbs the remainder
        let length = if i == chunk_count - 1 {
            chunk_size + remainder
        } else {
            chunk_size
        };
        chunks.push(ChunkPlan {
            index: i,
            offset,
            length,
            completed: false,
            checksum: None,
        });
        offset += length;
    }
    chunks
}
```

### Pattern 2: Cross-Platform Positional Read/Write

**What:** Use OS-specific traits for reading/writing at specific offsets without moving the file cursor. This is the key enabler for parallel chunk I/O.

**When to use:** All parallel chunk operations.

**Example:**
```rust
use std::fs::File;
use std::io;

/// Read exactly `length` bytes from `file` starting at `offset`.
/// Cross-platform: uses pread on Unix, seek_read on Windows.
pub fn read_at(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.read_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_read(buf, offset)
    }
}

/// Write `buf` to `file` at `offset`.
pub fn write_at(file: &File, offset: u64, buf: &[u8]) -> io::Result<usize> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::FileExt;
        file.write_at(buf, offset)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::FileExt;
        file.seek_write(buf, offset)
    }
}
```

**Thread safety note:** On Unix, `read_at` (pread) is fully thread-safe -- multiple threads can call it concurrently on the same `File` with different offsets without any synchronization. On Windows, `seek_read` updates the file cursor as a side effect, but since each thread specifies its own offset, the cursor position is irrelevant for correctness of positional reads. The `File` can be shared via `Arc<File>` (no Mutex needed for reads).

### Pattern 3: Parallel Chunk Transfer with Rayon

**What:** Use rayon's parallel iterator to process multiple chunks concurrently.

**When to use:** When chunk_count > 1.

**Example:**
```rust
use rayon::prelude::*;
use std::sync::Arc;

pub fn parallel_copy_chunked(
    src_file: Arc<File>,
    dst_file: Arc<File>,
    chunks: &mut [ChunkPlan],
    progress: &ProgressBar,
) -> Result<(), FluxError> {
    let buf_size = 256 * 1024; // 256KB read buffer per chunk

    chunks.par_iter_mut()
        .filter(|chunk| !chunk.completed)
        .try_for_each(|chunk| -> Result<(), FluxError> {
            let mut buf = vec![0u8; buf_size];
            let mut remaining = chunk.length;
            let mut chunk_offset = chunk.offset;
            let mut hasher = blake3::Hasher::new();

            while remaining > 0 {
                let to_read = std::cmp::min(remaining, buf_size as u64) as usize;
                let n = read_at(&src_file, chunk_offset, &mut buf[..to_read])?;
                if n == 0 { break; }

                write_at(&dst_file, chunk_offset, &buf[..n])?;
                hasher.update(&buf[..n]);
                progress.inc(n as u64);

                chunk_offset += n as u64;
                remaining -= n as u64;
            }

            chunk.checksum = Some(hasher.finalize().to_hex().to_string());
            chunk.completed = true;
            Ok(())
        })?;

    Ok(())
}
```

### Pattern 4: Resume Manifest

**What:** Serialize transfer state to a `.flux-resume.json` sidecar file next to the destination. On resume, load the manifest, skip completed chunks, continue from incomplete ones.

**When to use:** Automatically for chunked transfers. The manifest is cleaned up on successful completion.

**Example:**
```rust
use std::path::{Path, PathBuf};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct TransferManifest {
    pub version: u32,
    pub source: PathBuf,
    pub dest: PathBuf,
    pub total_size: u64,
    pub chunk_count: usize,
    pub chunks: Vec<ChunkPlan>,
    pub compress: bool,
    pub file_checksum: Option<String>,
}

impl TransferManifest {
    /// Manifest file path: dest_path + ".flux-resume.json"
    pub fn manifest_path(dest: &Path) -> PathBuf {
        let mut name = dest.file_name().unwrap().to_os_string();
        name.push(".flux-resume.json");
        dest.with_file_name(name)
    }

    pub fn save(&self, dest: &Path) -> Result<(), FluxError> {
        let path = Self::manifest_path(dest);
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    pub fn load(dest: &Path) -> Result<Option<Self>, FluxError> {
        let path = Self::manifest_path(dest);
        if !path.exists() {
            return Ok(None);
        }
        let json = std::fs::read_to_string(&path)?;
        let manifest: Self = serde_json::from_str(&json)?;
        Ok(Some(manifest))
    }

    pub fn cleanup(dest: &Path) -> Result<(), FluxError> {
        let path = Self::manifest_path(dest);
        if path.exists() {
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }
}
```

### Pattern 5: Streaming Compression Wrapper

**What:** Wrap chunk I/O in zstd `Encoder`/`Decoder` when `--compress` is enabled.

**When to use:** When user passes `--compress` flag. Most effective for text-heavy content.

**Example:**
```rust
use std::io::{Read, Write};

/// Compress `data` in memory using zstd at the given level.
pub fn compress_chunk(data: &[u8], level: i32) -> Result<Vec<u8>, FluxError> {
    let compressed = zstd::encode_all(data, level)?;
    Ok(compressed)
}

/// Decompress zstd-compressed data.
pub fn decompress_chunk(data: &[u8]) -> Result<Vec<u8>, FluxError> {
    let decompressed = zstd::decode_all(data)?;
    Ok(decompressed)
}
```

**Note on compression + chunked I/O:** When compression is enabled, each chunk is compressed independently. This means the compressed chunk data must be written sequentially (compressed chunks have variable sizes, so they can't use positional writes to a flat file). The approach is: read source chunk -> compress in memory -> write compressed chunk to a staging buffer -> decompress on read side. For local-to-local copies, compression adds CPU overhead without reducing I/O, so it is primarily useful for network transfers (Phase 3). For Phase 2, implement the compression/decompression infrastructure so it's ready, but note in documentation that `--compress` is most beneficial for network transfers.

### Pattern 6: Bandwidth Throttling Wrapper

**What:** A `Read`/`Write` wrapper that uses a token bucket to limit throughput.

**When to use:** When `--limit` is specified.

**Example:**
```rust
use std::io::{self, Read};
use std::time::{Duration, Instant};

pub struct ThrottledReader<R: Read> {
    inner: R,
    bytes_per_sec: u64,
    tokens: u64,
    last_refill: Instant,
}

impl<R: Read> ThrottledReader<R> {
    pub fn new(inner: R, bytes_per_sec: u64) -> Self {
        Self {
            inner,
            bytes_per_sec,
            tokens: bytes_per_sec, // start with 1 second of tokens
            last_refill: Instant::now(),
        }
    }

    fn refill(&mut self) {
        let elapsed = self.last_refill.elapsed();
        let new_tokens = (elapsed.as_secs_f64() * self.bytes_per_sec as f64) as u64;
        if new_tokens > 0 {
            self.tokens = std::cmp::min(
                self.tokens + new_tokens,
                self.bytes_per_sec * 2, // max burst = 2 seconds
            );
            self.last_refill = Instant::now();
        }
    }
}

impl<R: Read> Read for ThrottledReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.refill();

        if self.tokens == 0 {
            // Sleep until we have tokens
            let sleep_time = Duration::from_secs_f64(
                buf.len() as f64 / self.bytes_per_sec as f64
            );
            std::thread::sleep(sleep_time);
            self.refill();
        }

        // Limit read size to available tokens
        let max_read = std::cmp::min(buf.len(), self.tokens as usize);
        let n = self.inner.read(&mut buf[..max_read])?;
        self.tokens = self.tokens.saturating_sub(n as u64);
        Ok(n)
    }
}
```

### Pattern 7: Auto Chunk Size Detection

**What:** Heuristic to choose chunk count and size based on file size.

**When to use:** When user doesn't specify `--chunks` explicitly.

**Example:**
```rust
/// Determine optimal chunk count for a file.
///
/// Heuristic based on file size:
/// - < 10 MB: 1 chunk (no parallelism, overhead not worth it)
/// - 10 MB - 100 MB: 2 chunks
/// - 100 MB - 1 GB: 4 chunks
/// - 1 GB - 10 GB: 8 chunks
/// - > 10 GB: 16 chunks
///
/// Capped at available_parallelism() to avoid over-subscribing CPU.
pub fn auto_chunk_count(file_size: u64) -> usize {
    let base_count = match file_size {
        0..=10_485_759 => 1,                    // < 10 MB
        10_485_760..=104_857_599 => 2,           // 10 MB - 100 MB
        104_857_600..=1_073_741_823 => 4,        // 100 MB - 1 GB
        1_073_741_824..=10_737_418_239 => 8,     // 1 GB - 10 GB
        _ => 16,                                 // > 10 GB
    };

    // Don't exceed available CPU parallelism
    let max_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);

    std::cmp::min(base_count, max_threads)
}
```

### Anti-Patterns to Avoid

- **Using `seek()` for parallel reads:** Do NOT use `file.seek()` + `file.read()` from multiple threads. The seek position is shared state. Use positional I/O (`read_at`/`seek_read`) instead.
- **Compressing entire file in memory:** Do NOT load an entire large file to compress it. Use streaming or per-chunk compression.
- **Ignoring partial writes:** `write_at` may write fewer bytes than requested. Always loop until all bytes are written (like `write_all` but for positional writes).
- **Async file I/O for local files:** tokio's async file I/O just wraps blocking I/O in `spawn_blocking`. For CPU-bound parallel chunk work, rayon is more appropriate. Reserve async for network I/O in Phase 3.
- **Sharing a single hasher across threads:** `blake3::Hasher` is not `Sync`. Hash each chunk independently, then combine or verify individually.
- **Forgetting to pre-allocate the destination file:** When writing chunks in parallel, the destination file must be pre-allocated to the correct size (e.g., `file.set_len(total_size)`) so that positional writes at arbitrary offsets work correctly.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Cryptographic hashing | Custom hash function | `blake3` crate | BLAKE3 is faster than MD5/SHA and cryptographically secure. SIMD-optimized. |
| Compression | Custom compression | `zstd` crate | zstd has best ratio-to-speed in the industry. C library bindings are zero-overhead. |
| Thread pool management | Custom thread spawning | `rayon` crate | Work-stealing scheduler handles uneven chunk sizes. Automatic CPU detection. |
| Byte size parsing | Regex/manual parsing of "10MB" | `bytesize` crate | Handles all units, case variations, edge cases correctly. |

**Key insight:** The performance-critical code paths (hashing, compression, thread scheduling) all have mature Rust crates with years of optimization. Hand-rolling any of these would be slower and buggier. The code you DO write is the orchestration: chunk planning, manifest management, wiring the pieces together, and the CLI interface.

## Common Pitfalls

### Pitfall 1: Destination File Not Pre-Allocated
**What goes wrong:** Writing chunks to random offsets in a file that hasn't been sized correctly causes sparse files, incorrect sizes, or errors.
**Why it happens:** On most OS, writing at offset N only works if the file has been extended to at least that size.
**How to avoid:** Call `file.set_len(total_size)` on the destination file before starting parallel chunk writes.
**Warning signs:** Destination file smaller than source, or zero-filled gaps between chunks.

### Pitfall 2: Manifest Not Flushed Before Crash
**What goes wrong:** Chunk completes, but manifest isn't written to disk. On resume, chunk is re-transferred.
**Why it happens:** Manifest is only saved periodically or at end.
**How to avoid:** Save manifest after each chunk completes. Use `fsync`/flush. The overhead is minimal since manifests are small.
**Warning signs:** Interrupted transfers always restart from the beginning.

### Pitfall 3: Windows seek_read Side Effects
**What goes wrong:** On Windows, `seek_read` updates the file cursor as a side effect, unlike Unix `pread`.
**Why it happens:** Win32 API `SetFilePointerEx` + `ReadFile` vs Unix's atomic `pread`.
**How to avoid:** Never rely on the file cursor when using positional I/O. Each thread should only use positional reads/writes, never standard `read()`/`write()` on the same file handle.
**Warning signs:** Corrupted data in multi-threaded transfers on Windows.

### Pitfall 4: Compression Changes Chunk Sizes
**What goes wrong:** Compressed chunks have variable lengths, but the destination file was pre-allocated for uncompressed sizes.
**Why it happens:** Compression ratios vary by content.
**How to avoid:** When compression is enabled, don't use positional writes to the final file. Instead, write compressed chunks sequentially or to temporary chunk files that are concatenated at the end. For Phase 2 (local), simplest approach is to compress each chunk into a buffer and write sequentially. Phase 3 (network) will transmit compressed chunks over the wire.
**Warning signs:** Overlapping writes, corrupted decompressed data.

### Pitfall 5: Bandwidth Limit Applied Per-Thread Instead of Globally
**What goes wrong:** With `--limit 10MB/s` and 4 parallel chunks, actual throughput is 40MB/s.
**Why it happens:** Each thread has its own throttle instance.
**How to avoid:** Share a single `Arc<Mutex<TokenBucket>>` across all threads, or divide the limit evenly: each of N threads gets `limit / N` bytes/sec.
**Warning signs:** Actual bandwidth exceeds the configured limit proportional to chunk count.

### Pitfall 6: Small File Overhead
**What goes wrong:** Chunking a 1KB file into 4 chunks adds overhead (manifest, thread spawning) that makes it slower than sequential copy.
**Why it happens:** No minimum file size threshold.
**How to avoid:** Only chunk files above a minimum threshold (e.g., 10MB). Below that, use the existing `copy_file_with_progress` path.
**Warning signs:** Small file transfers slower than Phase 1.

## Code Examples

### BLAKE3 File Hashing

```rust
// Source: https://docs.rs/blake3/latest/blake3/
use std::fs::File;
use std::io::Read;

pub fn hash_file(path: &std::path::Path) -> Result<String, FluxError> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536]; // 64KB buffer

    loop {
        let n = file.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }

    Ok(hasher.finalize().to_hex().to_string())
}
```

### BLAKE3 Chunk Hashing (Parallel-Friendly)

```rust
/// Hash a specific byte range of a file using positional I/O.
pub fn hash_chunk(file: &File, offset: u64, length: u64) -> Result<String, FluxError> {
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 65536];
    let mut remaining = length;
    let mut pos = offset;

    while remaining > 0 {
        let to_read = std::cmp::min(remaining, buf.len() as u64) as usize;
        let n = read_at(file, pos, &mut buf[..to_read])?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
        pos += n as u64;
        remaining -= n as u64;
    }

    Ok(hasher.finalize().to_hex().to_string())
}
```

### zstd Streaming Compression

```rust
// Source: https://docs.rs/zstd/latest/zstd/
use std::io;

/// Compress src into dst using zstd at the given level.
/// Level 3 is the default; range is typically 1-22.
pub fn compress_stream<R: io::Read, W: io::Write>(
    src: R,
    dst: W,
    level: i32,
) -> Result<u64, FluxError> {
    let mut encoder = zstd::Encoder::new(dst, level)?;
    let bytes = io::copy(&mut src, &mut encoder)?;
    encoder.finish()?;
    Ok(bytes)
}

/// Decompress src into dst.
pub fn decompress_stream<R: io::Read, W: io::Write>(
    src: R,
    mut dst: W,
) -> Result<u64, FluxError> {
    let mut decoder = zstd::Decoder::new(src)?;
    let bytes = io::copy(&mut decoder, &mut dst)?;
    Ok(bytes)
}
```

### CLI Argument Extensions

```rust
// Extend CpArgs in src/cli/args.rs
#[derive(clap::Args, Debug)]
pub struct CpArgs {
    // ... existing fields ...

    /// Number of parallel chunks for transfer (0 = auto-detect)
    #[arg(long, default_value = "0")]
    pub chunks: usize,

    /// Verify transfer integrity with BLAKE3 checksum
    #[arg(long)]
    pub verify: bool,

    /// Enable zstd compression for transfer
    #[arg(long)]
    pub compress: bool,

    /// Bandwidth limit (e.g., "10MB/s", "500KB/s")
    #[arg(long)]
    pub limit: Option<String>,

    /// Resume interrupted transfer
    #[arg(long)]
    pub resume: bool,
}
```

### Bandwidth Parsing

```rust
/// Parse a bandwidth string like "10MB/s" into bytes per second.
pub fn parse_bandwidth(s: &str) -> Result<u64, FluxError> {
    // Strip trailing "/s" if present
    let s = s.trim_end_matches("/s").trim_end_matches("/S");
    let bytes: bytesize::ByteSize = s.parse()
        .map_err(|_| FluxError::Config(format!("Invalid bandwidth: {}", s)))?;
    Ok(bytes.as_u64())
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| SHA-256 checksums | BLAKE3 checksums | 2020+ | 14x faster, parallelizable, tree-structured for chunk verification |
| gzip compression | zstd compression | 2018+ | Better ratio at same speed; or much faster at same ratio |
| Single-threaded copy | Parallel chunked I/O with pread/pwrite | Always available | Saturates disk bandwidth on modern SSDs/NVMe |
| Fixed chunk sizes | Adaptive chunk sizing | Industry practice | Avoids overhead for small files, maximizes parallelism for large ones |

**Deprecated/outdated:**
- MD5/SHA-1 for integrity: Cryptographically broken, slower than BLAKE3
- gzip/deflate for transfer compression: zstd dominates on speed/ratio tradeoff
- `lseek()` + `read()` for parallel access: `pread()` is atomic and thread-safe

## Open Questions

1. **Compression + parallel chunks interaction**
   - What we know: Compressed chunks have variable sizes, making positional writes impossible for the final file.
   - What's unclear: Best approach for local-to-local compressed transfers (compress chunks individually vs. compress-then-chunk vs. skip compression for local).
   - Recommendation: For Phase 2, implement compression as a per-chunk operation. If `--compress` is active, read chunk -> compress -> write compressed sequentially (no parallel writes). Or, only enable parallel writes when compression is off, and fall back to sequential compressed writes. This can be optimized in later phases.

2. **Resume across different chunk counts**
   - What we know: Manifest stores the chunk plan used for the original transfer.
   - What's unclear: Should we allow resuming with a different `--chunks` value than the original?
   - Recommendation: No. If manifest exists, use its chunk plan. If user wants different chunk count, they must delete the manifest (or the partial destination) and restart.

3. **Verify flag behavior**
   - What we know: `--verify` should compute and check BLAKE3 checksums.
   - What's unclear: Should it verify during transfer (per-chunk) or after (whole-file re-read)?
   - Recommendation: Both. Compute per-chunk hashes during transfer. With `--verify`, do a post-transfer whole-file hash comparison. Per-chunk hashes are always stored in manifest for resume verification regardless of `--verify` flag.

## Sources

### Primary (HIGH confidence)
- [blake3 crate docs](https://docs.rs/blake3/latest/blake3/) - API, features, Hasher/Hash types
- [zstd crate docs](https://docs.rs/zstd/latest/zstd/) - Encoder/Decoder API, streaming compression
- [std::os::unix::fs::FileExt](https://doc.rust-lang.org/std/os/unix/fs/trait.FileExt.html) - `read_at`/`write_at` thread safety
- [std::os::windows::fs::FileExt](https://doc.rust-lang.org/std/os/windows/fs/trait.FileExt.html) - `seek_read`/`seek_write`
- [blake3 crates.io](https://crates.io/crates/blake3) - v1.8.3
- [zstd crates.io](https://docs.rs/crate/zstd/latest) - v0.13.3

### Secondary (MEDIUM confidence)
- [Rust forum: parallel chunked file reading](https://users.rust-lang.org/t/what-is-the-best-way-to-read-chunked-file-in-parallel/99702) - Community patterns for pread-based parallelism
- [Rust forum: tokio::io::copy vs std](https://users.rust-lang.org/t/tokio-copy-slower-than-std-io-copy/111242) - Performance difference confirming sync > async for local file I/O
- [blog: don't mix rayon and tokio](https://blog.dureuill.net/articles/dont-mix-rayon-tokio/) - Architecture guidance
- [stream_limiter crate](https://docs.rs/stream_limiter/latest/stream_limiter/) - Sync bandwidth limiting pattern
- [rclone chunker](https://rclone.org/chunker/) - Chunk size defaults and heuristics

### Tertiary (LOW confidence)
- Chunk size heuristic thresholds (10MB/100MB/1GB/10GB): Based on rclone defaults (8MB default chunk), rsync behavior, and general industry practice. No single authoritative source for exact thresholds; these should be tunable and benchmarked.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All crates are well-established (blake3: 98M+ downloads, zstd: 190M+ downloads, rayon: extremely mature). Versions verified against crates.io.
- Architecture: HIGH - Positional I/O pattern (pread/pwrite) is a well-known systems programming technique. Cross-platform Rust APIs verified in std docs.
- Pitfalls: HIGH - Windows seek_read behavior verified in std docs. Compression + chunking interaction is a known architectural challenge.
- Chunk size heuristics: MEDIUM - Thresholds are reasonable engineering defaults based on industry practice but should be benchmarked for this specific tool.

**Research date:** 2026-02-16
**Valid until:** 2026-04-16 (60 days - all crates are stable, no breaking changes expected)
