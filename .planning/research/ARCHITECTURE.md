# Architecture Patterns: High-Performance CLI File Transfer Tools

**Domain:** Multi-protocol file transfer CLI (SMB, SFTP, WebDAV, local paths)
**Researched:** 2026-02-16
**Overall Confidence:** HIGH

## Executive Summary

High-performance file transfer CLIs like rclone, hf_transfer, and similar tools follow a **layered architecture** with clear separation between protocol abstraction, transfer orchestration, and user interface. The key architectural insight is that protocol-specific logic should be isolated behind a unified trait/interface, allowing the transfer engine to operate identically regardless of the underlying protocol.

For Flux, the recommended architecture consists of five primary layers: **Protocol Backend Layer**, **Transfer Engine**, **Queue Manager**, **State/Resume Manager**, and **UI Layer (TUI/CLI)**. This design enables parallel chunked transfers, multi-connection streaming, resume support, and sync mode while maintaining clean separation of concerns.

---

## Recommended Architecture

```
+------------------------------------------------------------------+
|                        UI LAYER                                   |
|  +------------------+  +------------------+  +------------------+ |
|  |    CLI Mode      |  |    TUI Mode      |  |    Sync Mode     | |
|  | (clap arguments) |  | (ratatui + async |  | (watch + daemon) | |
|  |                  |  |  event loop)     |  |                  | |
|  +--------+---------+  +--------+---------+  +--------+---------+ |
+-----------|--------------------|--------------------|--------------+
            |                    |                    |
            v                    v                    v
+------------------------------------------------------------------+
|                     COMMAND DISPATCHER                            |
|  Parses user intent, creates TransferJob(s), routes to engine     |
+------------------------------------------------------------------+
            |
            v
+------------------------------------------------------------------+
|                     QUEUE MANAGER                                 |
|  +------------------+  +------------------+  +------------------+ |
|  | Job Queue        |  | Priority System  |  | Concurrency      | |
|  | (VecDeque +      |  | (user-defined    |  | Limiter          | |
|  |  persistence)    |  |  ordering)       |  | (semaphores)     | |
|  +------------------+  +------------------+  +------------------+ |
+------------------------------------------------------------------+
            |
            v
+------------------------------------------------------------------+
|                     TRANSFER ENGINE                               |
|  +------------------+  +------------------+  +------------------+ |
|  | Chunk Scheduler  |  | Worker Pool      |  | Progress         | |
|  | (splits files,   |  | (tokio tasks,    |  | Aggregator       | |
|  |  byte ranges)    |  |  parallel I/O)   |  | (mpsc channels)  | |
|  +------------------+  +------------------+  +------------------+ |
+------------------------------------------------------------------+
            |
            v
+------------------------------------------------------------------+
|                   STATE / RESUME MANAGER                          |
|  +------------------+  +------------------+  +------------------+ |
|  | Checkpoint Store |  | Manifest Files   |  | Integrity        | |
|  | (tracks chunks   |  | (.flux-state/)   |  | Verification     | |
|  |  completed)      |  |                  |  | (checksums)      | |
|  +------------------+  +------------------+  +------------------+ |
+------------------------------------------------------------------+
            |
            v
+------------------------------------------------------------------+
|                   PROTOCOL BACKEND LAYER                          |
|  +------------+  +------------+  +------------+  +------------+  |
|  |   Local    |  |    SMB     |  |   SFTP     |  |  WebDAV    |  |
|  | (std::fs,  |  | (pavao/   |  | (openssh-  |  | (reqwest_  |  |
|  |  tokio::fs)|  |  smb-rs)   |  |  sftp)     |  |  dav)      |  |
|  +------+-----+  +------+-----+  +------+-----+  +------+-----+  |
|         |               |               |               |        |
|  +------+---------------+---------------+---------------+------+ |
|  |                  FluxBackend Trait                          | |
|  |  connect(), list(), stat(), open_read(), open_write(),      | |
|  |  seek_read(), seek_write(), remove(), mkdir(), rename()     | |
|  +-------------------------------------------------------------+ |
+------------------------------------------------------------------+
```

---

## Component Boundaries

| Component | Responsibility | Communicates With | Boundary Type |
|-----------|---------------|-------------------|---------------|
| **CLI Parser** | Parse arguments, validate paths, construct commands | Command Dispatcher | Function calls |
| **TUI Renderer** | Display progress, handle keyboard input, render widgets | Transfer Engine (via channels), Queue Manager | MPSC channels |
| **Command Dispatcher** | Route commands (copy, sync, queue) to appropriate handlers | Queue Manager, Transfer Engine | Function calls |
| **Queue Manager** | Maintain job queue, persist state, enforce priorities | Transfer Engine, State Manager | Channels + shared state |
| **Transfer Engine** | Orchestrate chunked parallel transfers, manage worker pool | Protocol Backends, State Manager, Progress Aggregator | Async trait calls + channels |
| **Chunk Scheduler** | Split files into chunks, assign byte ranges to workers | Worker Pool, State Manager | Internal to engine |
| **Worker Pool** | Execute individual chunk transfers concurrently | Protocol Backends | Tokio tasks |
| **Progress Aggregator** | Collect progress from workers, compute rates, ETA | TUI Renderer, CLI output | MPSC channels |
| **State Manager** | Persist transfer state, enable resume, track checksums | Filesystem (local), Queue Manager | File I/O |
| **Protocol Backends** | Protocol-specific I/O operations | Remote/local filesystems | Async trait implementation |

---

## Data Flow

### Copy Operation Flow

```
User Input: "flux copy smb://server/share/file.zip ./local/"
                            |
                            v
                   +------------------+
                   | CLI Parser       |
                   | - Parse URIs     |
                   | - Resolve paths  |
                   +--------+---------+
                            |
                            v
                   +------------------+
                   | Command Dispatch |
                   | - Create Job     |
                   | - Validate perms |
                   +--------+---------+
                            |
                            v
                   +------------------+
                   | Queue Manager    |
                   | - Enqueue job    |
                   | - Check existing |
                   +--------+---------+
                            |
                            v
                   +------------------+
                   | Transfer Engine  |<------------ Progress Events
                   | - Query file size|               (to TUI/CLI)
                   | - Calculate chunks|
                   +--------+---------+
                            |
              +-------------+-------------+
              |             |             |
              v             v             v
        +---------+   +---------+   +---------+
        | Worker 1|   | Worker 2|   | Worker 3|  (parallel)
        | Chunk 0 |   | Chunk 1 |   | Chunk 2 |
        +----+----+   +----+----+   +----+----+
             |             |             |
             v             v             v
        +---------+   +---------+   +---------+
        | Backend |   | Backend |   | Backend |
        | seek_   |   | seek_   |   | seek_   |
        | read()  |   | read()  |   | read()  |
        +---------+   +---------+   +---------+
             |             |             |
             +------+------+------+------+
                    |
                    v
             +-------------+
             | Local Write |
             | (merged at  |
             |  offsets)   |
             +-------------+
                    |
                    v
             +-------------+
             | Checksum    |
             | Verify      |
             +-------------+
                    |
                    v
             +-------------+
             | State: Done |
             | Clean temp  |
             +-------------+
```

### Sync Operation Flow

```
User Input: "flux sync --bidirectional smb://share ./local"
                            |
                            v
                   +------------------+
                   | Sync Orchestrator|
                   +--------+---------+
                            |
              +-------------+-------------+
              |                           |
              v                           v
     +------------------+        +------------------+
     | List Remote      |        | List Local       |
     | (Backend.list()) |        | (std::fs)        |
     +--------+---------+        +--------+---------+
              |                           |
              +-------------+-------------+
                            |
                            v
                   +------------------+
                   | Diff Calculator  |
                   | - Compare trees  |
                   | - Load last sync |
                   |   manifest       |
                   +--------+---------+
                            |
              +-------------+-------------+-------------+
              |             |             |             |
              v             v             v             v
        +---------+   +---------+   +---------+   +---------+
        | New on  |   | New on  |   | Modified|   | Conflict|
        | Remote  |   | Local   |   | (either)|   | (both)  |
        +---------+   +---------+   +---------+   +---------+
              |             |             |             |
              v             v             v             v
        +---------+   +---------+   +---------+   +---------+
        | Copy    |   | Copy    |   | Compare |   | Resolve |
        | to local|   | to      |   | mtime/  |   | strategy|
        |         |   | remote  |   | hash    |   | (config)|
        +---------+   +---------+   +---------+   +---------+
                            |
                            v
                   +------------------+
                   | Update Manifest  |
                   | (persist sync    |
                   |  state)          |
                   +------------------+
```

---

## Patterns to Follow

### Pattern 1: Backend Trait Abstraction (CRITICAL)

**What:** Define a unified `FluxBackend` trait that all protocol implementations must satisfy. This is the architectural foundation.

**When:** Always. All file operations must go through this trait.

**Why:** Enables protocol-agnostic transfer logic. Allows adding new protocols without touching core transfer code.

**Example:**
```rust
#[async_trait]
pub trait FluxBackend: Send + Sync {
    /// Connect to the backend (may be no-op for local)
    async fn connect(&mut self) -> Result<(), FluxError>;

    /// Disconnect and cleanup
    async fn disconnect(&mut self) -> Result<(), FluxError>;

    /// List directory contents
    async fn list(&self, path: &Path) -> Result<Vec<FileEntry>, FluxError>;

    /// Get file metadata (size, mtime, permissions)
    async fn stat(&self, path: &Path) -> Result<FileStat, FluxError>;

    /// Open file for reading at arbitrary offset (enables chunking)
    async fn open_read(&self, path: &Path, offset: u64) -> Result<Box<dyn AsyncRead + Send + Unpin>, FluxError>;

    /// Open file for writing at arbitrary offset
    async fn open_write(&self, path: &Path, offset: u64) -> Result<Box<dyn AsyncWrite + Send + Unpin>, FluxError>;

    /// Check if backend supports byte-range reads
    fn supports_seek(&self) -> bool;

    /// Check if backend supports parallel connections
    fn supports_parallel(&self) -> bool;

    /// Backend-specific feature flags
    fn features(&self) -> BackendFeatures;
}
```

### Pattern 2: Semaphore-Based Concurrency Control

**What:** Use async semaphores to limit concurrent operations at multiple levels (connections, chunks, retries).

**When:** For all parallel operations to prevent resource exhaustion.

**Why:** Prevents overwhelming remote servers, controls memory usage, enables graceful degradation.

**Example:**
```rust
pub struct TransferEngine {
    /// Limits total concurrent file transfers
    transfer_semaphore: Arc<Semaphore>,
    /// Limits concurrent chunks per file
    chunk_semaphore: Arc<Semaphore>,
    /// Limits concurrent retry attempts
    retry_semaphore: Arc<Semaphore>,
}

impl TransferEngine {
    pub fn new(config: &TransferConfig) -> Self {
        Self {
            transfer_semaphore: Arc::new(Semaphore::new(config.max_transfers)),
            chunk_semaphore: Arc::new(Semaphore::new(config.max_chunks_per_file)),
            retry_semaphore: Arc::new(Semaphore::new(config.max_parallel_retries)),
        }
    }
}
```

### Pattern 3: Channel-Based Progress Reporting

**What:** Use MPSC channels to stream progress updates from workers to aggregator to UI.

**When:** For all transfer operations needing progress feedback.

**Why:** Decouples transfer logic from display logic. Enables both TUI and CLI modes with same engine.

**Example:**
```rust
#[derive(Clone)]
pub enum ProgressEvent {
    ChunkStarted { job_id: Uuid, chunk_id: u32, offset: u64, size: u64 },
    ChunkProgress { job_id: Uuid, chunk_id: u32, bytes_transferred: u64 },
    ChunkCompleted { job_id: Uuid, chunk_id: u32, checksum: Option<String> },
    ChunkFailed { job_id: Uuid, chunk_id: u32, error: String, will_retry: bool },
    FileCompleted { job_id: Uuid, total_bytes: u64, duration: Duration },
}

// Workers send events
tx.send(ProgressEvent::ChunkProgress {
    job_id,
    chunk_id,
    bytes_transferred: current
}).await?;

// Aggregator collects and computes rates
while let Some(event) = rx.recv().await {
    match event {
        ProgressEvent::ChunkProgress { job_id, chunk_id, bytes_transferred } => {
            self.update_progress(job_id, chunk_id, bytes_transferred);
            self.ui_tx.send(self.compute_display_state())?;
        }
        // ...
    }
}
```

### Pattern 4: State Checkpointing for Resume

**What:** Persist transfer state to disk periodically, enabling resume after interruption.

**When:** For any transfer that may be interrupted (large files, unreliable networks).

**Why:** Large file transfers can take hours; losing progress is unacceptable.

**Example:**
```rust
#[derive(Serialize, Deserialize)]
pub struct TransferCheckpoint {
    pub job_id: Uuid,
    pub source: String,
    pub destination: String,
    pub total_size: u64,
    pub chunk_size: u64,
    pub chunks: Vec<ChunkState>,
    pub started_at: DateTime<Utc>,
    pub last_updated: DateTime<Utc>,
}

#[derive(Serialize, Deserialize)]
pub struct ChunkState {
    pub chunk_id: u32,
    pub offset: u64,
    pub size: u64,
    pub status: ChunkStatus,
    pub checksum: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub enum ChunkStatus {
    Pending,
    InProgress,
    Completed,
    Failed { attempts: u32, last_error: String },
}
```

### Pattern 5: Immediate-Mode TUI Rendering

**What:** Render entire UI each frame based on current state (not retained mode).

**When:** For the TUI interface using ratatui.

**Why:** Ratatui uses immediate rendering; trying to use retained mode patterns will cause bugs.

**Example:**
```rust
fn ui(frame: &mut Frame, app: &AppState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Header
            Constraint::Min(10),    // Transfer list
            Constraint::Length(3),  // Progress bar
            Constraint::Length(1),  // Status line
        ])
        .split(frame.area());

    // Render each widget based on current state
    frame.render_widget(header_widget(&app.queue_stats), chunks[0]);
    frame.render_widget(transfer_list_widget(&app.active_transfers), chunks[1]);
    frame.render_widget(overall_progress_gauge(&app.overall_progress), chunks[2]);
    frame.render_widget(status_line(&app.status_message), chunks[3]);
}
```

---

## Anti-Patterns to Avoid

### Anti-Pattern 1: Protocol-Specific Code in Transfer Engine

**What:** Embedding `if protocol == SMB { ... } else if protocol == SFTP { ... }` logic in the transfer orchestration code.

**Why bad:** Creates tight coupling, makes adding protocols expensive, spreads protocol bugs across codebase.

**Instead:** All protocol-specific logic lives behind the `FluxBackend` trait. Transfer engine only calls trait methods.

### Anti-Pattern 2: Blocking I/O in Async Context

**What:** Using `std::fs::File` or blocking network calls directly in async functions.

**Why bad:** Blocks the Tokio runtime thread, destroying parallelism and responsiveness.

**Instead:** Use `tokio::fs::File`, `spawn_blocking()` for CPU-intensive work, and async protocol clients.

### Anti-Pattern 3: Unbounded Queues Without Backpressure

**What:** Using unbounded channels/queues for progress events or chunk scheduling.

**Why bad:** Memory grows unbounded under load; can OOM with many small files.

**Instead:** Use bounded channels with capacity limits. Apply backpressure when queues fill.

### Anti-Pattern 4: Single-Threaded Checksum Verification

**What:** Computing file checksums sequentially after transfer completes.

**Why bad:** Wastes time re-reading file; doubles I/O for large files.

**Instead:** Compute rolling checksum during transfer, or verify chunks as they complete.

### Anti-Pattern 5: Global Mutable State for Progress

**What:** Using `static mut` or `lazy_static` RefCell for progress tracking.

**Why bad:** Race conditions, hard to test, prevents multiple concurrent operations.

**Instead:** Pass owned state through channels; use `Arc<Mutex>` only where necessary with clear ownership.

---

## Build Order (Dependencies Between Components)

The following build order respects component dependencies:

### Phase 1: Foundation (No External Dependencies)
1. **Error types** - `FluxError` enum with all error variants
2. **Configuration** - `FluxConfig` struct, TOML/env parsing
3. **Data types** - `FileEntry`, `FileStat`, `TransferJob`, `ChunkState`

*Rationale: These are leaf nodes; everything else depends on them.*

### Phase 2: Backend Layer
4. **FluxBackend trait** - Define the abstraction interface
5. **Local backend** - Implement for `std::fs`/`tokio::fs` (easiest, enables testing)
6. **Additional backends** - SFTP, SMB, WebDAV (can be parallel)

*Rationale: Backend trait must exist before implementations. Local backend enables integration testing of higher layers without network.*

### Phase 3: Core Engine
7. **State Manager** - Checkpoint persistence, resume logic
8. **Progress Aggregator** - Channel-based progress collection
9. **Chunk Scheduler** - File splitting, byte range calculation
10. **Worker Pool** - Tokio task management, semaphore control
11. **Transfer Engine** - Orchestrates all above components

*Rationale: State Manager and Progress Aggregator are independent. Chunk Scheduler and Worker Pool need data types. Transfer Engine integrates everything.*

### Phase 4: Queue & Orchestration
12. **Queue Manager** - Job queue, persistence, priority
13. **Sync Engine** - Directory comparison, manifest tracking, conflict resolution

*Rationale: Queue Manager uses Transfer Engine. Sync Engine is a specialized orchestrator.*

### Phase 5: User Interface
14. **CLI Mode** - Argument parsing with clap, output formatting
15. **TUI Mode** - Ratatui widgets, event loop, keyboard handling

*Rationale: UI is last because it only consumes other components; nothing depends on it.*

### Dependency Graph

```
                    Error Types
                         |
                         v
                   Configuration
                         |
                         v
                    Data Types
                    /    |    \
                   v     v     v
              FluxBackend Trait
              /    |     |    \
             v     v     v     v
          Local  SFTP   SMB  WebDAV
             \     \    /    /
              \     \  /    /
               v     vv    v
              State Manager    Progress Aggregator
                   \              /
                    \            /
                     v          v
               Chunk Scheduler  Worker Pool
                        \      /
                         v    v
                    Transfer Engine
                         |
              +----------+----------+
              |                     |
              v                     v
        Queue Manager          Sync Engine
              |                     |
              +----------+----------+
                         |
              +----------+----------+
              |                     |
              v                     v
          CLI Mode              TUI Mode
```

---

## Scalability Considerations

| Concern | Small (< 100 files) | Medium (1K-10K files) | Large (100K+ files) |
|---------|---------------------|----------------------|---------------------|
| **Listing** | In-memory tree | Streaming iterator | Paginated listing, no full tree in memory |
| **Queue** | In-memory VecDeque | File-backed queue | SQLite-backed queue |
| **Progress** | Per-file events | Batched events (100ms) | Sampled events (1% or time-based) |
| **Checksums** | SHA-256 all files | xxHash for speed | Optional, configurable |
| **State files** | Single JSON | Per-directory manifests | Sharded state DB |

---

## Sources

### Architecture References (HIGH Confidence)
- [Rclone Architecture (DeepWiki)](https://deepwiki.com/rclone/rclone) - Layered architecture, backend interface, VFS design
- [hf_transfer Core Components (DeepWiki)](https://deepwiki.com/huggingface/hf_transfer/2.1-core-components) - Rust async chunked transfer architecture
- [WinSCP File System Abstraction (DeepWiki)](https://deepwiki.com/winscp/winscp/2.4-file-system-and-remote-files) - Multi-protocol abstraction layer patterns

### Rust Libraries (HIGH Confidence - Context7/Official Docs)
- [Tokio Documentation](https://docs.rs/tokio/latest/tokio/) - Async runtime, channels, semaphores
- [Ratatui Documentation](https://ratatui.rs/) - TUI patterns, immediate rendering, widgets

### Protocol Libraries (HIGH Confidence)
- [remotefs crate](https://crates.io/crates/remotefs) - Unified file system trait for multiple protocols
- [openssh-sftp-client](https://crates.io/crates/openssh-sftp-client) - Async SFTP client
- [pavao (SMB)](https://crates.io/crates/pavao) - SMB client library
- [reqwest_dav](https://lib.rs/crates/reqwest_dav) - Async WebDAV client

### Design Patterns (MEDIUM Confidence - WebSearch verified)
- [Mutagen File Synchronization](https://mutagen.io/documentation/synchronization/) - Bidirectional sync algorithm
- [A Journey into File Transfer Protocols in Rust](https://blog.veeso.dev/blog/en/a-journey-into-file-transfer-protocols-in-rust/) - RemoteFs design rationale
- [AWS: Parallelizing Large Downloads](https://aws.amazon.com/blogs/developer/parallelizing-large-downloads-for-optimal-speed/) - Chunked transfer patterns

### Transfer Resume (MEDIUM Confidence)
- [VanDyke Resume File Transfers](https://www.vandyke.com/products/securefx/resume_file_transfers.html) - Checkpoint file design
- [Movedat Resuming Downloads](https://www.dataexpedition.com/expedat/Docs/movedat/resuming.html) - Checkpoint architecture

---

## Implications for Roadmap

Based on this architecture analysis, the recommended phase structure is:

1. **Phase 1: Foundation** - Error types, config, data types, FluxBackend trait, local backend
   - *This is the minimum viable architecture; everything builds on it*

2. **Phase 2: Single-File Transfer** - Transfer engine (chunking, workers), progress reporting
   - *Proves the parallel transfer model works before adding complexity*

3. **Phase 3: Protocol Backends** - SFTP, SMB, WebDAV implementations
   - *Can be parallelized; each backend is independent*

4. **Phase 4: Queue & Resume** - Queue manager, state persistence, resume support
   - *Requires working transfer engine to test against*

5. **Phase 5: CLI Polish** - Argument parsing, output formatting, error messages
   - *User-facing quality; depends on all features existing*

6. **Phase 6: TUI Mode** - Ratatui integration, interactive queue management
   - *Separate interface; can be developed in parallel with CLI polish*

7. **Phase 7: Sync Mode** - Bidirectional sync, conflict resolution, watch mode
   - *Most complex feature; needs all infrastructure in place*

**Key dependencies to respect:**
- FluxBackend trait must be finalized before implementing any backends
- Transfer engine must work with local backend before adding network protocols
- Resume support requires both transfer engine and state manager
- TUI cannot be built until progress aggregator exists
