# Phase 1: Foundation - Research

**Researched:** 2026-02-16
**Domain:** Rust CLI application scaffolding, local file transfer, progress reporting, cross-platform path handling
**Confidence:** HIGH

## Summary

Phase 1 establishes the complete foundation for Flux: a working CLI binary that can copy files and directories between local paths with real-time progress, glob-based filtering, configurable verbosity, and helpful error messages. This is a greenfield Rust project with no existing code.

The Rust ecosystem provides mature, battle-tested libraries for every component needed. **clap 4.5** with derive macros handles CLI argument parsing and subcommands. **tokio** provides the async runtime (required for future phases but established now). **indicatif** delivers thread-safe progress bars with speed/ETA display. **globset** (from the ripgrep ecosystem) handles glob pattern matching for include/exclude filters. **walkdir** provides recursive directory traversal with efficient filtering. **thiserror + anyhow** provide the error handling foundation, and **tracing** handles structured logging with verbosity levels.

The key architectural decisions for Phase 1 are: (1) define the `FluxBackend` trait abstraction that all future protocol backends will implement, (2) implement the `LocalBackend` as the first implementation, (3) establish the progress reporting channel pattern using `tokio::sync::mpsc`, and (4) use a custom `Read` wrapper pattern for tracking bytes transferred during copy operations. The local file copy should use `std::fs` with `BufReader`/`BufWriter` in `spawn_blocking` for best performance, NOT `tokio::fs` for individual operations (which adds spawn_blocking overhead per call).

**Primary recommendation:** Build a well-structured Cargo binary project with clear module boundaries (cli, backend, transfer, progress, error, config), implement `FluxBackend` trait + `LocalBackend` first, use `indicatif` for progress (not a full channel-based aggregator yet -- that comes with parallel chunks in Phase 2), and focus on correctness and cross-platform path handling from day one.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| CORE-01 | User can copy single file from source to destination path | Local backend `open_read`/`open_write` with buffered I/O; custom `Read` wrapper for progress tracking; `std::fs::copy` equivalent with progress |
| CORE-02 | User can copy directory recursively preserving structure | `walkdir` crate for recursive traversal; `filter_entry` for efficient exclusion; create directory structure before copying files |
| CORE-04 | User can see real-time progress (percentage, speed, ETA, bytes) | `indicatif` ProgressBar with template `{bar} {percent}% {bytes}/{total_bytes} {bytes_per_sec} ETA {eta}`; `MultiProgress` for future multi-file support |
| CORE-06 | User can exclude files/folders using glob patterns | `globset` crate from ripgrep ecosystem; `GlobSet` for matching multiple patterns efficiently; integrate with `walkdir::filter_entry` |
| CORE-07 | User can include only matching files using glob patterns | Same `globset` infrastructure; include patterns checked after exclude patterns; files must match at least one include pattern if any are specified |
| CORE-09 | Tool works on Windows, Linux, and macOS | Use `std::path::Path`/`PathBuf` exclusively; `dirs` crate for config paths; test trailing slash normalization; handle Windows UNC paths |
| PROT-01 | User can transfer to/from local filesystem paths | `LocalBackend` implementing `FluxBackend` trait; uses `std::fs` with `BufReader`/`BufWriter` in blocking context |
| CONF-04 | User can set verbosity level (quiet/normal/verbose) | `tracing` with `tracing-subscriber` env-filter; `--quiet`/`-q` and `--verbose`/`-v` flags via clap; suppress progress bar in quiet mode |
| CLI-01 | User can run simple commands (`flux cp src dest`) | `clap` 4.5 derive API with `#[derive(Parser)]`; `cp` subcommand with positional source/dest args; `-r`/`--recursive` flag for directories |
| CLI-04 | Tool provides helpful error messages with suggested fixes | `thiserror` for typed errors with Display messages; `anyhow` for context chaining; custom error renderer that suggests fixes (e.g., "Permission denied. Try running with elevated privileges.") |
</phase_requirements>

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| **clap** | 4.5.x | CLI argument parsing, subcommands, help generation | Derive API is zero-boilerplate, type-safe; 10K+ code snippets; built-in shell completion support; colored help output |
| **tokio** | 1.49.x | Async runtime (foundation for future phases) | Industry standard async runtime; LTS releases; required by all async libraries in later phases; `spawn_blocking` for file I/O |
| **indicatif** | 0.18.x | Progress bars with speed, ETA, percentage | 90M+ downloads; thread-safe; `MultiProgress` for parallel bars; `ProgressStyle` templates for custom formatting |
| **walkdir** | 2.5.x | Recursive directory traversal | BurntSushi (ripgrep author); `filter_entry` for efficient pruning; cross-platform; handles symlinks |
| **globset** | 0.4.x | Glob pattern matching for include/exclude | BurntSushi (ripgrep ecosystem); compiles multiple globs into optimized matcher; supports `**`, `{a,b}`, `[!x]` |
| **thiserror** | 2.x | Library/domain error types | Derive macro for `Error` trait; `#[from]` for automatic conversion; `#[error]` for Display; compile-time checked |
| **anyhow** | 1.x | Application error context/propagation | `.context("doing X")` for error chains; `bail!()` macro; works with any `Error` type |
| **tracing** | 0.1.x | Structured logging with levels | Async-aware; spans for operation tracing; by Tokio team; replaces `log` crate |
| **tracing-subscriber** | 0.3.x | Log output formatting and filtering | `env-filter` for `RUST_LOG` support; console output with colors; integrates with tracing |
| **serde** | 1.0.x | Serialization framework | De facto standard; needed for config file parsing and future state persistence |
| **toml** | 0.8.x | Configuration file format | Rust community convention (Cargo.toml uses it); human-readable; less error-prone than YAML |
| **dirs** | 5.x | Platform-specific directory paths | XDG on Linux, Known Folders on Windows, Standard Dirs on macOS; for config file location |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| **tempfile** | 3.x | Temp files/directories for testing | All integration tests need temp dirs for copy operations |
| **assert_cmd** | 2.x | CLI integration testing | Testing `flux cp` command end-to-end from binary |
| **predicates** | 3.x | Assertion helpers for tests | Complement to assert_cmd for output/file assertions |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| walkdir | `ignore` crate (WalkBuilder) | `ignore` adds .gitignore respect and more features but heavier; walkdir + globset is simpler for Phase 1 and gives us fine-grained control over pattern logic |
| globset | `glob` crate | `glob` is simpler but only matches one pattern at a time; `globset` matches multiple patterns simultaneously which is needed for `--exclude` lists |
| indicatif | Custom progress with `crossterm` | indicatif is purpose-built for progress bars; building custom wastes time and misses edge cases (terminal resize, redirect detection) |
| std::fs in spawn_blocking | tokio::fs directly | tokio::fs adds spawn_blocking per operation internally; manual batching in `spawn_blocking` gives better control and performance |
| confy | Manual toml + dirs | confy auto-creates config dir and loads/saves; but we want more control over config merging and error messages; use toml + dirs directly |

**Installation:**
```toml
[package]
name = "flux"
version = "0.1.0"
edition = "2021"

[dependencies]
# CLI
clap = { version = "4.5", features = ["derive"] }

# Async runtime (foundation for future phases)
tokio = { version = "1", features = ["full"] }

# Progress
indicatif = "0.18"

# File operations
walkdir = "2.5"
globset = "0.4"

# Config & serialization
serde = { version = "1.0", features = ["derive"] }
toml = "0.8"
dirs = "5"

# Error handling
thiserror = "2"
anyhow = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

## Architecture Patterns

### Recommended Project Structure

```
src/
├── main.rs              # Entry point: parse args, set up tracing, dispatch
├── cli/
│   ├── mod.rs           # CLI module root
│   └── args.rs          # Clap derive structs (Cli, Commands, CpArgs)
├── backend/
│   ├── mod.rs           # FluxBackend trait definition + BackendFeatures
│   └── local.rs         # LocalBackend implementation (std::fs)
├── transfer/
│   ├── mod.rs           # Transfer orchestration (single file, directory)
│   ├── copy.rs          # File copy with progress (Read wrapper pattern)
│   └── filter.rs        # Glob-based include/exclude filtering
├── progress/
│   ├── mod.rs           # Progress reporting abstractions
│   └── bar.rs           # indicatif ProgressBar/MultiProgress setup
├── config/
│   ├── mod.rs           # Configuration loading/merging
│   └── types.rs         # FluxConfig, Verbosity enum
└── error.rs             # FluxError enum (thiserror), error rendering
```

### Pattern 1: FluxBackend Trait (Foundation for all phases)

**What:** Define the protocol abstraction trait that the local backend implements now, and SFTP/SMB/WebDAV will implement later.
**When to use:** Always -- all file operations go through this trait.
**Why:** Establishes the architectural contract early. Changing the trait later cascades through the entire codebase. Phase 1 is the cheapest time to get it right.

```rust
// src/backend/mod.rs
use std::path::Path;

/// Metadata about a file or directory
#[derive(Debug, Clone)]
pub struct FileStat {
    pub size: u64,
    pub is_dir: bool,
    pub is_file: bool,
    pub modified: Option<std::time::SystemTime>,
    pub permissions: Option<u32>,
}

/// Entry in a directory listing
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub path: std::path::PathBuf,
    pub stat: FileStat,
}

/// What capabilities a backend supports
#[derive(Debug, Clone)]
pub struct BackendFeatures {
    pub supports_seek: bool,
    pub supports_parallel: bool,
    pub supports_permissions: bool,
}

/// Core abstraction for all file backends.
/// Phase 1 implements LocalBackend only.
/// Future phases add SftpBackend, SmbBackend, WebDavBackend.
pub trait FluxBackend: Send + Sync {
    /// Get file/directory metadata
    fn stat(&self, path: &Path) -> Result<FileStat, crate::error::FluxError>;

    /// List directory contents (non-recursive)
    fn list_dir(&self, path: &Path) -> Result<Vec<FileEntry>, crate::error::FluxError>;

    /// Open a file for reading, returns a boxed Read
    fn open_read(&self, path: &Path) -> Result<Box<dyn std::io::Read + Send>, crate::error::FluxError>;

    /// Create/open a file for writing, returns a boxed Write
    fn open_write(&self, path: &Path) -> Result<Box<dyn std::io::Write + Send>, crate::error::FluxError>;

    /// Create directory (and parents if needed)
    fn create_dir_all(&self, path: &Path) -> Result<(), crate::error::FluxError>;

    /// Check backend capabilities
    fn features(&self) -> BackendFeatures;
}
```

**Note on sync vs async:** Phase 1 uses synchronous `FluxBackend` trait methods because local file I/O is inherently blocking. When network backends arrive (Phase 3), the trait will evolve to async with `#[async_trait]`. For now, sync is simpler, correct, and avoids unnecessary `spawn_blocking` wrappers. The transfer orchestrator wraps backend calls in `spawn_blocking` where needed.

### Pattern 2: Progress-Tracking Read Wrapper

**What:** A custom `Read` implementation that wraps another `Read` and reports bytes transferred to an `indicatif::ProgressBar`.
**When to use:** For every file copy operation.
**Why:** `std::fs::copy` and `std::io::copy` don't provide progress callbacks. Wrapping the reader is the standard Rust pattern.

```rust
// src/transfer/copy.rs
use std::io::{self, Read, Write, BufReader, BufWriter};
use indicatif::ProgressBar;

/// Wraps a Read and updates a ProgressBar as bytes are read.
pub struct ProgressReader<R: Read> {
    inner: R,
    progress: ProgressBar,
}

impl<R: Read> ProgressReader<R> {
    pub fn new(inner: R, progress: ProgressBar) -> Self {
        Self { inner, progress }
    }
}

impl<R: Read> Read for ProgressReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let bytes_read = self.inner.read(buf)?;
        self.progress.inc(bytes_read as u64);
        Ok(bytes_read)
    }
}

/// Copy a single file with progress reporting.
/// Uses BufReader/BufWriter for efficient I/O.
pub fn copy_file_with_progress(
    source: &std::path::Path,
    dest: &std::path::Path,
    progress: &ProgressBar,
) -> Result<u64, crate::error::FluxError> {
    let src_file = std::fs::File::open(source)?;
    let src_size = src_file.metadata()?.len();

    progress.set_length(src_size);

    let reader = BufReader::with_capacity(256 * 1024, src_file); // 256KB buffer
    let mut reader = ProgressReader::new(reader, progress.clone());

    let dest_file = std::fs::File::create(dest)?;
    let mut writer = BufWriter::with_capacity(256 * 1024, dest_file);

    let bytes_copied = io::copy(&mut reader, &mut writer)?;
    writer.flush()?;

    progress.finish();
    Ok(bytes_copied)
}
```

### Pattern 3: Glob-Based File Filtering

**What:** Use `globset::GlobSet` to compile include/exclude patterns into an efficient matcher, then integrate with directory traversal.
**When to use:** When `--exclude` or `--include` flags are provided.
**Why:** `GlobSet` matches multiple patterns simultaneously in a single pass. Integrating with `walkdir::filter_entry` prunes excluded directories early (avoids descending).

```rust
// src/transfer/filter.rs
use globset::{Glob, GlobSet, GlobSetBuilder};

pub struct TransferFilter {
    excludes: Option<GlobSet>,
    includes: Option<GlobSet>,
}

impl TransferFilter {
    pub fn new(
        exclude_patterns: &[String],
        include_patterns: &[String],
    ) -> Result<Self, crate::error::FluxError> {
        let excludes = if exclude_patterns.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in exclude_patterns {
                builder.add(Glob::new(pattern)?);
            }
            Some(builder.build()?)
        };

        let includes = if include_patterns.is_empty() {
            None
        } else {
            let mut builder = GlobSetBuilder::new();
            for pattern in include_patterns {
                builder.add(Glob::new(pattern)?);
            }
            Some(builder.build()?)
        };

        Ok(Self { excludes, includes })
    }

    /// Returns true if the path should be transferred.
    /// Logic: exclude first, then include.
    /// - If excludes match, skip (unless includes also match).
    /// - If includes exist and none match, skip.
    pub fn should_transfer(&self, path: &std::path::Path) -> bool {
        // Check excludes first
        if let Some(ref excludes) = self.excludes {
            if excludes.is_match(path) {
                return false;
            }
        }

        // If includes specified, path must match at least one
        if let Some(ref includes) = self.includes {
            return includes.is_match(path);
        }

        true
    }
}
```

### Pattern 4: Structured Error Types with Helpful Messages

**What:** Use `thiserror` for typed error variants with human-readable messages and suggested fixes.
**When to use:** For all error paths in the application.
**Why:** CLI-04 requires "helpful error messages with suggested fixes." Structured errors enable a renderer that adds context-specific suggestions.

```rust
// src/error.rs
use std::path::PathBuf;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FluxError {
    #[error("Source not found: {path}")]
    SourceNotFound { path: PathBuf },

    #[error("Destination not writable: {path}")]
    DestinationNotWritable { path: PathBuf },

    #[error("Permission denied: {path}")]
    PermissionDenied { path: PathBuf },

    #[error("Source is a directory, use -r flag for recursive copy")]
    IsDirectory { path: PathBuf },

    #[error("Invalid glob pattern '{pattern}': {reason}")]
    InvalidPattern { pattern: String, reason: String },

    #[error("I/O error: {source}")]
    Io {
        #[from]
        source: std::io::Error,
    },

    #[error("Configuration error: {0}")]
    Config(String),
}

impl FluxError {
    /// Returns a user-friendly suggestion for how to fix the error.
    pub fn suggestion(&self) -> Option<&str> {
        match self {
            FluxError::SourceNotFound { .. } => {
                Some("Check the path exists and spelling is correct.")
            }
            FluxError::PermissionDenied { .. } => {
                Some("Try running with elevated privileges, or check file permissions.")
            }
            FluxError::IsDirectory { .. } => {
                Some("Use 'flux cp -r <source> <dest>' for directory copies.")
            }
            FluxError::DestinationNotWritable { .. } => {
                Some("Check that the destination directory exists and you have write permission.")
            }
            _ => None,
        }
    }
}
```

### Pattern 5: CLI Argument Parsing with Clap Derive

**What:** Use clap's derive macros for type-safe argument parsing with subcommands.
**When to use:** Entry point of the application.
**Why:** Zero-boilerplate, compile-time checked, auto-generates help text and shell completions.

```rust
// src/cli/args.rs
use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "flux", version, about = "Blazing-fast file transfer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Verbosity: -q for quiet, -v for verbose
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Quiet mode: suppress all output except errors
    #[arg(short, long, global = true)]
    pub quiet: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Copy files or directories
    Cp(CpArgs),
}

#[derive(clap::Args, Debug)]
pub struct CpArgs {
    /// Source path
    pub source: PathBuf,

    /// Destination path
    pub dest: PathBuf,

    /// Copy directories recursively
    #[arg(short, long)]
    pub recursive: bool,

    /// Exclude files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Include only files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub include: Vec<String>,
}
```

### Anti-Patterns to Avoid

- **Using `tokio::fs` for each small operation:** Each `tokio::fs` call internally calls `spawn_blocking`. For file copy, use `std::fs` with `BufReader`/`BufWriter` inside a single `spawn_blocking` call, or use it synchronously since Phase 1 is single-file sequential.
- **String manipulation for paths:** Never split on `/` or `\`. Always use `std::path::Path` methods: `.join()`, `.parent()`, `.file_name()`, `.components()`.
- **Hardcoding path separators:** Use `std::path::MAIN_SEPARATOR` or better yet, let `Path` handle it.
- **Loading entire directory tree into memory:** Use `walkdir` as a streaming iterator; process entries as they come. For Phase 1 this matters for large directories.
- **Unbounded progress bar updates:** Don't call `progress.inc()` for every single byte. The `BufReader` naturally batches reads (256KB default), so progress updates happen at buffer-fill granularity -- fast enough for smooth display, infrequent enough for low overhead.
- **Ignoring trailing slash semantics:** Decide and document: does `flux cp dir/ dest/` copy contents of dir into dest, or copy dir itself into dest? Recommend rsync-like behavior: trailing slash = contents, no trailing slash = directory itself.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Glob pattern matching | Custom regex-based matcher | `globset` crate | Glob syntax is deceptively complex (`**`, `{a,b}`, `[!x]`); globset handles all edge cases |
| Progress bar display | Custom terminal output with `\r` | `indicatif` crate | Terminal resize, redirect detection, rate estimation, multi-bar support, thread safety |
| Recursive dir walk | `std::fs::read_dir` with manual recursion | `walkdir` crate | Symlink cycles, permission errors, efficient pruning, cross-platform edge cases |
| Platform config dirs | Hardcoded `~/.config/flux` | `dirs` crate | XDG on Linux, Library/Application Support on macOS, AppData on Windows |
| CLI parsing | Manual `std::env::args` | `clap` derive | Help text, version, completions, validation, conflict groups, colored output |
| Speed/ETA calculation | Manual `Instant::elapsed() / bytes` | `indicatif` built-in | Rolling average, human-readable formatting, handles zero-byte edge case |

**Key insight:** Phase 1 might look simple ("just copy files"), but every component has subtle cross-platform edge cases. Using battle-tested crates prevents week-long debugging sessions on Windows path handling or terminal rendering glitches.

## Common Pitfalls

### Pitfall 1: Trailing Slash Path Semantics Confusion

**What goes wrong:** `flux cp dir/ dest/` vs `flux cp dir dest/` behave differently (copy contents vs. copy directory). Users get unexpected directory nesting.
**Why it happens:** rsync-style trailing slash semantics are non-obvious. Developers implement one behavior without documenting or testing the other.
**How to avoid:** Choose rsync convention (trailing slash = copy contents), document it clearly, normalize paths at CLI boundary (strip trailing separators and record whether one was present), and test both cases explicitly.
**Warning signs:** Tests only cover one slash variant. Users report "extra nesting" in destination.

### Pitfall 2: Windows Path Edge Cases

**What goes wrong:** UNC paths (`\\server\share`), paths with spaces, very long paths (>260 chars), reserved names (CON, PRN, NUL), and drive letter differences break operations.
**Why it happens:** Development/testing only on Linux or only on short paths.
**How to avoid:** Use `std::path::Path` for all operations. On Windows, use `\\?\` prefix for long paths (Rust's `std::fs::canonicalize` does this). Test with spaces, Unicode, and long paths in CI. Never construct paths via string concatenation.
**Warning signs:** Tests pass on Linux CI but fail on Windows.

### Pitfall 3: Permission Errors During Directory Copy

**What goes wrong:** Permission error mid-directory-copy leaves destination in partial state. No cleanup, no clear report of what succeeded vs. failed.
**Why it happens:** Error handling copies pattern from single-file (bail on first error) without considering partial-success semantics.
**How to avoid:** Track which files succeeded and which failed. On error, report the specific file that failed and continue with remaining files (unless `--fail-fast` flag). Summary at end shows "copied X of Y files, Z errors."
**Warning signs:** Large directory copy fails on one unreadable file and reports nothing about the hundreds that succeeded.

### Pitfall 4: Empty Source or Destination Paths

**What goes wrong:** User passes empty string or "." or ".." as path, leading to confusing behavior or overwriting unexpected files.
**Why it happens:** No path validation at CLI boundary.
**How to avoid:** Validate paths before starting operations. Canonicalize paths to resolve `.` and `..`. Check source != destination. Warn if destination is the current directory.
**Warning signs:** `flux cp . .` succeeds silently (copies nothing or corrupts).

### Pitfall 5: Progress Bar Rendering in Piped/Redirected Output

**What goes wrong:** Progress bar control characters pollute redirected output (`flux cp src dest > log.txt` fills log with ANSI codes).
**Why it happens:** Terminal detection not checked before rendering progress.
**How to avoid:** `indicatif` handles this automatically -- it detects whether stdout is a TTY. But ensure progress bar goes to stderr, not stdout, so `> file` still works. Use `ProgressBar::with_draw_target(ProgressDrawTarget::stderr())`.
**Warning signs:** Machine-parseable output mixed with progress bar characters.

## Code Examples

Verified patterns for Phase 1 implementation:

### Complete CLI Entry Point

```rust
// src/main.rs
use clap::Parser;
use tracing_subscriber::EnvFilter;

mod cli;
mod backend;
mod transfer;
mod progress;
mod config;
mod error;

fn main() -> anyhow::Result<()> {
    let cli = cli::args::Cli::parse();

    // Set up tracing based on verbosity
    let filter = match (cli.quiet, cli.verbose) {
        (true, _) => "error",
        (_, 0) => "info",
        (_, 1) => "debug",
        (_, _) => "trace",
    };

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(filter))
        )
        .with_writer(std::io::stderr) // Keep stdout clean for output
        .init();

    match cli.command {
        cli::args::Commands::Cp(args) => {
            transfer::execute_copy(args, cli.quiet)?;
        }
    }

    Ok(())
}
```

### Progress Bar Setup with indicatif

```rust
// src/progress/bar.rs
use indicatif::{ProgressBar, ProgressStyle, HumanBytes, ProgressDrawTarget};

pub fn create_file_progress(total_bytes: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total_bytes);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
             {bytes}/{total_bytes} ({bytes_per_sec}, ETA {eta})"
        )
        .unwrap()
        .progress_chars("=>-")
    );
    pb
}

pub fn create_directory_progress(total_files: u64, quiet: bool) -> ProgressBar {
    if quiet {
        return ProgressBar::hidden();
    }

    let pb = ProgressBar::new(total_files);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] \
             {pos}/{len} files ({per_sec}, ETA {eta}) {msg}"
        )
        .unwrap()
        .progress_chars("=>-")
    );
    pb
}
```

### Recursive Directory Copy with Filtering

```rust
// src/transfer/mod.rs (sketch)
use walkdir::WalkDir;
use std::path::Path;

pub fn copy_directory(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    quiet: bool,
) -> Result<TransferResult, FluxError> {
    // First pass: count files for progress (fast, metadata only)
    let file_count = WalkDir::new(source)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .filter(|e| filter.should_transfer(e.path()))
        .count() as u64;

    let progress = create_directory_progress(file_count, quiet);
    let mut result = TransferResult::new();

    // Second pass: actual copy
    for entry in WalkDir::new(source)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let dest_path = dest.join(relative);

        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&dest_path)?;
        } else if entry.file_type().is_file() {
            if !filter.should_transfer(entry.path()) {
                continue;
            }

            // Ensure parent directory exists
            if let Some(parent) = dest_path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            match copy_single_file(entry.path(), &dest_path) {
                Ok(bytes) => result.add_success(bytes),
                Err(e) => result.add_error(entry.path().to_owned(), e),
            }
            progress.inc(1);
        }
    }

    progress.finish_with_message("done");
    Ok(result)
}
```

### Error Display with Suggestions

```rust
// Rendering errors to user (in main.rs or error.rs)
fn display_error(err: &FluxError) {
    eprintln!("error: {}", err);
    if let Some(suggestion) = err.suggestion() {
        eprintln!("  hint: {}", suggestion);
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `log` crate for logging | `tracing` crate (structured, async-aware) | 2020-2021 | All major Rust async projects use tracing now |
| thiserror 1.x | thiserror 2.x | 2024 | Procedural macro improvements; same API surface |
| clap 3.x builder API | clap 4.x derive API | 2022 | Derive is now the recommended approach; builder still available |
| `glob` crate (one pattern at a time) | `globset` (multi-pattern matching) | 2017+ | globset is far more efficient for multiple patterns |
| Manual terminal progress | `indicatif` with ProgressStyle templates | 2018+ | De facto standard; handles terminal edge cases |
| `std::fs::copy` (no progress) | Custom Read wrapper + `io::copy` | N/A | Required pattern for progress tracking in Rust |

**Deprecated/outdated:**
- **clap 3.x builder API**: Still works but derive is strongly preferred for new projects
- **`log` crate alone**: Use `tracing` with `tracing-log` bridge for compatibility
- **`glob` crate for multi-pattern**: Use `globset` instead for efficiency

## Open Questions

1. **Trailing slash semantics**
   - What we know: rsync uses trailing slash to mean "copy contents only." cp does not distinguish.
   - What's unclear: Which convention will be most intuitive for Flux users?
   - Recommendation: Adopt rsync convention (it's the power-user expectation), document clearly in help text, and test both cases.

2. **FluxBackend trait: sync vs async from day one**
   - What we know: Local file I/O is blocking. Network backends (Phase 3) need async. Changing trait from sync to async is a large refactor.
   - What's unclear: Is it worth the async complexity in Phase 1 when only local backend exists?
   - Recommendation: Start with synchronous trait in Phase 1. The refactor to async in Phase 3 is well-understood and bounded. Premature async adds `#[async_trait]` overhead and complexity without benefit. The transfer orchestrator already wraps calls in `spawn_blocking` where needed.

3. **Buffer size for file copy**
   - What we know: Default `BufReader` is 8KB. xcp and similar tools use larger buffers (64KB-256KB). NVMe SSDs benefit from larger buffers.
   - What's unclear: Optimal buffer size varies by hardware.
   - Recommendation: Use 256KB buffers (256 * 1024) as a sensible default. Can be made configurable later. This is a good middle ground for SSD and HDD.

4. **Single progress bar vs. multi-progress for directory copy**
   - What we know: `indicatif` supports both. Single bar (overall bytes) is simpler. Multi-bar (per-file + overall) is richer but noisy.
   - What's unclear: User preference for Phase 1.
   - Recommendation: Use a single progress bar showing file count for directory copies. Phase 2 (parallel chunks) will introduce `MultiProgress` for concurrent operations. Keep it simple for Phase 1.

5. **Error handling during directory copy: fail-fast vs. continue**
   - What we know: rsync continues past errors by default. cp stops on first error by default.
   - What's unclear: Which is the right default for Flux?
   - Recommendation: Continue on error by default (matches rsync behavior). Report errors in summary at end. Add `--fail-fast` flag for strict behavior.

## Sources

### Primary (HIGH confidence)
- [clap derive API tutorial](https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html) - Official documentation for clap 4.x derive macros
- [indicatif documentation](https://docs.rs/indicatif) - Progress bar API, ProgressStyle templates, MultiProgress
- [globset documentation](https://docs.rs/globset/latest/globset/) - Glob pattern syntax and GlobSet builder
- [walkdir documentation](https://docs.rs/walkdir/latest/walkdir/) - Recursive directory traversal with filter_entry
- [tokio::fs documentation](https://docs.rs/tokio/latest/tokio/fs/index.html) - Async file I/O limitations (spawn_blocking behavior)
- [std::path documentation](https://doc.rust-lang.org/std/path/index.html) - Cross-platform path handling
- [thiserror documentation](https://docs.rs/thiserror) - Error derive macros
- [tracing documentation](https://docs.rs/tracing) - Structured logging framework

### Secondary (MEDIUM confidence)
- [Rust Error Handling with thiserror and anyhow](https://momori.dev/posts/rust-error-handling-thiserror-anyhow/) - Best practices for combining both
- [Shuttle Logging in Rust 2025](https://www.shuttle.dev/blog/2023/09/20/logging-in-rust) - tracing setup patterns
- [xcp - Extended cp](https://github.com/tarka/xcp) - Reference Rust file copy implementation with progress bars
- [Cargo Package Layout](https://doc.rust-lang.org/cargo/guide/project-layout.html) - Standard project structure
- [dirs crate](https://github.com/xdg-rs/dirs) - Platform-specific directory paths
- [Sling Academy - Cross-Platform Paths in Rust](https://www.slingacademy.com/article/creating-cross-platform-paths-and-file-operations-in-rust/) - std::path patterns
- [Tokio I/O tutorial](https://tokio.rs/tokio/tutorial/io) - Buffered I/O patterns
- [ignore crate WalkBuilder](https://docs.rs/ignore/latest/ignore/struct.WalkBuilder.html) - Alternative to walkdir + globset

### Project Research (HIGH confidence)
- `.planning/research/STACK.md` - Full technology stack analysis
- `.planning/research/ARCHITECTURE.md` - System architecture with FluxBackend trait design
- `.planning/research/PITFALLS.md` - Domain pitfalls including cross-platform and file I/O issues
- `.planning/research/FEATURES.md` - Feature landscape and competitive analysis

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries verified via official docs, crates.io versions, and project research
- Architecture: HIGH - Based on project ARCHITECTURE.md analysis plus verified patterns from xcp, rclone, termscp
- Pitfalls: HIGH - Cross-platform path handling and file I/O pitfalls verified across multiple sources
- Code examples: HIGH - Patterns verified against official documentation for clap, indicatif, walkdir, globset

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (stable domain, 30 days)
