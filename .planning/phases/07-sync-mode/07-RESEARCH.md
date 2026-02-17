# Phase 7: Sync Mode - Research

**Researched:** 2026-02-16
**Domain:** One-way directory synchronization, filesystem watching, cron scheduling
**Confidence:** HIGH

## Summary

Phase 7 implements one-way directory mirroring (`flux sync source/ dest/`), dry-run preview, cron-based scheduling, and filesystem watch mode for continuous sync. The existing codebase provides strong foundations: `SyncArgs` CLI skeleton (from Phase 6), `copy_directory` with filtering/conflict resolution, `TransferFilter` for glob patterns, `FluxBackend` abstraction with `FileStat` including modification times, and a proven `walkdir`-based directory traversal pattern.

The sync algorithm is a classic one-way mirror: walk the source tree, compare each file against the destination by modification time and size, copy new/changed files, and optionally delete files in the destination that no longer exist in the source. This is well-understood territory -- no delta/rolling-checksum needed (that's explicitly v2 scope per SYNC-V2-01). For filesystem watching, the `notify` crate (v8.2.0) with `notify-debouncer-full` (v0.7.0) is the ecosystem standard, used by rust-analyzer, deno, zed, and cargo-watch. For cron scheduling, the `cron` crate (v0.15.0) provides expression parsing, and `tokio` (already a dependency) handles the timing loop.

**Primary recommendation:** Build a `SyncEngine` that computes a diff plan (list of `SyncAction` items: Copy, Update, Delete, Skip), executes it using existing copy infrastructure, and supports dry-run by simply printing the plan. Watch mode wraps this in a `notify` watcher loop. Scheduling wraps it in a `cron`+`tokio::time::sleep_until` loop.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| SYNC-01 | User can sync directories (one-way mirror) | SyncEngine computes diff between source/dest using mtime+size comparison, then copies new/changed files and optionally deletes orphaned dest files. Reuses existing `copy_file_with_progress`, `TransferFilter`, `walkdir` traversal. |
| SYNC-02 | User can preview sync changes before applying (dry-run) | SyncEngine produces a `Vec<SyncAction>` plan. In dry-run mode, print the plan without executing. Same pattern as existing `dry_run_directory` in `transfer/mod.rs`. |
| SYNC-03 | User can schedule recurring syncs | `cron` crate (v0.15.0) parses cron expressions. Tokio sleep loop waits until next fire time. CLI flag `--schedule "*/5 * * * *"` triggers scheduling mode. |
| SYNC-04 | User can enable watch mode for continuous sync | `notify` (v8.2.0) + `notify-debouncer-full` (v0.7.0) watches source directory. On debounced change events, re-run sync. CLI flag `--watch` triggers watch mode. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| notify | 8.2.0 | Cross-platform filesystem watching | 62.7M downloads, used by rust-analyzer/deno/zed, supports inotify/FSEvents/ReadDirectoryChanges |
| notify-debouncer-full | 0.7.0 | Event debouncing for filesystem watcher | Companion to notify, consolidates rapid events, tracks renames via file IDs |
| cron | 0.15.0 | Cron expression parsing | Standard Rust cron parser, 0.15.0 released 2025-01-14, uses chrono for time |
| walkdir | 2.5 | Directory tree traversal | Already a project dependency, battle-tested |
| tokio | 1 | Async runtime for scheduling/watch loops | Already a project dependency with `full` features |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| chrono | 0.4 | DateTime operations for cron scheduling | Already a project dependency, used by `cron` crate for schedule iteration |
| indicatif | 0.18 | Progress bars for sync operations | Already a project dependency, reuse `create_directory_progress` |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| notify + debouncer-full | notify + debouncer-mini | debouncer-mini is simpler but doesn't handle rename tracking or file ID caching; full is more robust |
| cron (parsing only) + tokio sleep | tokio-cron-scheduler | tokio-cron-scheduler adds PostgreSQL/NATS persistence features we don't need; cron + manual tokio loop is lighter |
| Manual timestamp comparison | librsync (rolling checksums) | Rolling checksums are v2 scope (SYNC-V2-01); timestamp+size comparison is sufficient for v1 one-way mirror |

**Installation:**
```bash
cargo add notify@8.2 notify-debouncer-full@0.7 cron@0.15
```

## Architecture Patterns

### Recommended Module Structure
```
src/
├── sync/
│   ├── mod.rs           # Public API: execute_sync(), SyncArgs extension
│   ├── engine.rs        # SyncEngine: diff computation, plan execution
│   ├── plan.rs          # SyncAction enum, SyncPlan type, display formatting
│   ├── watch.rs         # Watch mode: notify watcher integration
│   └── schedule.rs      # Cron scheduling: parse + tokio sleep loop
├── cli/
│   └── args.rs          # Extend SyncArgs with --watch, --schedule, --delete, --exclude, --include
└── main.rs              # Wire Commands::Sync to sync::execute_sync()
```

### Pattern 1: Diff-Then-Execute Sync
**What:** Separate sync into two phases: (1) compute a plan (list of actions), (2) execute the plan. This enables dry-run by skipping step 2.
**When to use:** Always. This is the core pattern for the entire sync feature.
**Example:**
```rust
// Source: architecture pattern from rsync/rusync/robocopy
pub enum SyncAction {
    /// File exists in source but not dest -- copy it
    CopyNew { src: PathBuf, dest: PathBuf, size: u64 },
    /// File exists in both but source is newer/different -- update it
    UpdateChanged { src: PathBuf, dest: PathBuf, src_size: u64, dest_size: u64 },
    /// File exists in dest but not source -- delete it (only if --delete flag)
    DeleteOrphan { path: PathBuf, size: u64 },
    /// File is identical -- skip it
    Skip { path: PathBuf, reason: &'static str },
}

pub struct SyncPlan {
    pub actions: Vec<SyncAction>,
    pub total_copy_bytes: u64,
    pub files_to_copy: u64,
    pub files_to_update: u64,
    pub files_to_delete: u64,
    pub files_to_skip: u64,
}

pub fn compute_sync_plan(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
) -> Result<SyncPlan, FluxError> {
    // Walk source, compare against dest using mtime + size
    // Walk dest to find orphans (files not in source)
}

pub fn execute_sync_plan(
    plan: &SyncPlan,
    quiet: bool,
) -> Result<SyncResult, FluxError> {
    // Execute each action using existing copy infrastructure
}
```

### Pattern 2: Modification-Time + Size Comparison
**What:** Determine if a file needs syncing by comparing modification time and file size. A file needs updating if: (a) dest doesn't exist, (b) source mtime > dest mtime, or (c) source size != dest size.
**When to use:** For SYNC-01 file comparison logic.
**Example:**
```rust
fn needs_sync(src_meta: &std::fs::Metadata, dest_path: &Path) -> SyncDecision {
    let dest_meta = match std::fs::metadata(dest_path) {
        Ok(m) => m,
        Err(_) => return SyncDecision::CopyNew, // dest doesn't exist
    };

    // Different size -> definitely changed
    if src_meta.len() != dest_meta.len() {
        return SyncDecision::Update;
    }

    // Compare modification times
    match (src_meta.modified(), dest_meta.modified()) {
        (Ok(src_mtime), Ok(dest_mtime)) if src_mtime > dest_mtime => {
            SyncDecision::Update
        }
        _ => SyncDecision::Skip, // Same size, not newer
    }
}
```

### Pattern 3: Watch Mode Event Loop
**What:** Use `notify-debouncer-full` to watch the source directory, then re-run sync on each batch of debounced events. Use a cooldown period to avoid rapid successive syncs.
**When to use:** For SYNC-04 `--watch` mode.
**Example:**
```rust
// Source: notify-debouncer-full docs (docs.rs)
use notify_debouncer_full::{new_debouncer, DebounceEventResult};
use std::time::Duration;

fn watch_and_sync(source: &Path, dest: &Path, /* ... */) -> Result<(), FluxError> {
    let (tx, rx) = std::sync::mpsc::channel();

    let mut debouncer = new_debouncer(
        Duration::from_secs(2), // debounce timeout
        None,                   // tick rate (None = default)
        move |result: DebounceEventResult| {
            let _ = tx.send(result);
        },
    ).map_err(|e| FluxError::Io { source: std::io::Error::new(
        std::io::ErrorKind::Other, e.to_string()
    )})?;

    debouncer.watch(source, notify::RecursiveMode::Recursive)
        .map_err(|e| FluxError::Io { source: std::io::Error::new(
            std::io::ErrorKind::Other, e.to_string()
        )})?;

    eprintln!("Watching {} for changes...", source.display());

    // Initial sync
    let plan = compute_sync_plan(source, dest, &filter, delete_orphans)?;
    execute_sync_plan(&plan, quiet)?;

    // Event loop
    loop {
        match rx.recv() {
            Ok(Ok(_events)) => {
                // Re-compute and execute sync plan
                let plan = compute_sync_plan(source, dest, &filter, delete_orphans)?;
                if plan.has_changes() {
                    execute_sync_plan(&plan, quiet)?;
                }
            }
            Ok(Err(errors)) => {
                for e in errors {
                    tracing::warn!("Watch error: {}", e);
                }
            }
            Err(_) => break, // Channel closed
        }
    }
    Ok(())
}
```

### Pattern 4: Cron Schedule Loop
**What:** Parse a cron expression, calculate next fire time, sleep until then, run sync, repeat.
**When to use:** For SYNC-03 `--schedule` mode.
**Example:**
```rust
use cron::Schedule;
use std::str::FromStr;
use chrono::Utc;

fn scheduled_sync(
    cron_expr: &str,
    source: &Path,
    dest: &Path,
    /* ... */
) -> Result<(), FluxError> {
    let schedule = Schedule::from_str(cron_expr)
        .map_err(|e| FluxError::Config(format!("Invalid cron expression: {}", e)))?;

    eprintln!("Scheduled sync: {} -> {}", source.display(), dest.display());
    eprintln!("Cron: {}", cron_expr);

    let rt = tokio::runtime::Runtime::new()
        .map_err(|e| FluxError::Io { source: e })?;

    rt.block_on(async {
        loop {
            let next = schedule.upcoming(Utc).next()
                .ok_or_else(|| FluxError::Config("No upcoming schedule times".into()))?;

            let duration = (next - Utc::now()).to_std()
                .unwrap_or(std::time::Duration::from_secs(1));

            eprintln!("Next sync at: {}", next.format("%Y-%m-%d %H:%M:%S UTC"));
            tokio::time::sleep(duration).await;

            // Run sync
            let plan = compute_sync_plan(source, dest, &filter, delete_orphans)?;
            execute_sync_plan(&plan, quiet)?;
        }
    })
}
```

### Anti-Patterns to Avoid
- **Copying all files every sync:** Always diff first. The plan phase must skip identical files or the feature is useless for large directories.
- **Using polling watcher by default:** Use `RecommendedWatcher` (native OS events) not `PollWatcher`. Reserve polling for network filesystems only.
- **Blocking the main thread in watch/schedule mode:** Both modes involve long-running loops. Handle Ctrl+C gracefully with signal handling.
- **Re-implementing copy logic:** Reuse `copy_file_with_progress` and existing infrastructure. Don't duplicate the copy/conflict/progress code.
- **Modifying destination mtime:** After copying, DO NOT set the destination file's mtime to match the source. Let the OS set mtime naturally. This is simpler and avoids cross-platform issues. The sync comparison already handles "source newer" correctly.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Filesystem watching | Custom polling loop scanning mtimes | `notify` + `notify-debouncer-full` | OS-native events (inotify/FSEvents/ReadDirectoryChanges) are instant and battery-friendly; polling is slow and CPU-heavy |
| Cron expression parsing | Custom cron parser | `cron` crate | Cron parsing has many edge cases (ranges, steps, named months/days, year field); `cron` is well-tested |
| Event debouncing | Manual timer + event coalescing | `notify-debouncer-full` | Rename tracking, event consolidation, file ID matching are tricky; debouncer handles all of it |
| Directory traversal | Manual `read_dir` recursion | `walkdir` (already used) | Handles symlinks, permissions, depth limits correctly |

**Key insight:** The sync algorithm itself (diff computation) IS custom code because it encodes our specific comparison logic, but all supporting infrastructure (watching, scheduling, traversal, copying) has battle-tested libraries.

## Common Pitfalls

### Pitfall 1: Cross-Platform Modification Time Resolution
**What goes wrong:** File modification times have different resolutions on different filesystems. FAT32 has 2-second resolution. NTFS has 100ns. ext4 has 1ns. Comparing mtime across filesystems (e.g., syncing from ext4 to FAT32) can cause false positives.
**Why it happens:** The source mtime is 12:00:00.500 and the dest mtime is 12:00:00.000 (FAT32 rounded down). The diff says "source is newer" even though the file was just copied.
**How to avoid:** After copying, compare sizes as the primary "is it done" check. For mtime comparison, use a threshold (e.g., treat files as "same time" if mtime difference < 2 seconds). Or: always compare size first, and only use mtime as a tiebreaker when sizes match.
**Warning signs:** Tests pass on dev machine but sync keeps re-copying the same files in CI or on USB drives.

### Pitfall 2: Symlink Handling in Sync
**What goes wrong:** Following symlinks during sync can cause infinite loops (symlink pointing to parent), or can sync unexpected data.
**Why it happens:** `walkdir` follows symlinks by default.
**How to avoid:** Use `walkdir::WalkDir::new(path).follow_links(false)` to NOT follow symlinks. Copy symlinks as symlinks, or skip them. This is the rsync default behavior.
**Warning signs:** Sync hangs or produces enormous output for small directories.

### Pitfall 3: Race Conditions in Watch Mode
**What goes wrong:** A file is detected as changed, sync starts, but the file is still being written to. The sync copies a partial/corrupt file.
**Why it happens:** Notify fires events as soon as the OS detects the change, which may be before the write is complete.
**How to avoid:** The debouncer helps by waiting 2+ seconds after the last event. Additionally, for large files, consider checking if the file size is stable (compare size at event time vs. at sync time). The debounce timeout of 2 seconds handles most cases.
**Warning signs:** Synced files are truncated or corrupt, especially large files.

### Pitfall 4: Delete Mode Safety
**What goes wrong:** User runs `flux sync source/ dest/` with `--delete`, source directory is accidentally empty (e.g., wrong path), and all files in dest are deleted.
**Why it happens:** An empty source + `--delete` means "delete everything in dest."
**How to avoid:** Add a safety check: if source is empty (0 files after filtering), warn and require `--force` to proceed with deletions. Print a clear warning message showing how many files would be deleted.
**Warning signs:** Users report data loss after sync.

### Pitfall 5: inotify Watch Limit on Linux
**What goes wrong:** Watching a large directory tree fails with "too many open files" or similar errors on Linux.
**Why it happens:** Linux has a per-user limit on inotify watches (default: 8192). Large directories can exceed this.
**How to avoid:** Document the issue. Catch the error and provide a helpful message suggesting `sysctl fs.inotify.max_user_watches=524288`. Consider falling back to PollWatcher for very large trees.
**Warning signs:** Watch mode fails immediately on large source directories on Linux.

### Pitfall 6: Graceful Shutdown in Long-Running Modes
**What goes wrong:** `--watch` and `--schedule` modes run forever. Ctrl+C kills the process mid-sync, potentially leaving partially-copied files.
**Why it happens:** No signal handling.
**How to avoid:** Use `ctrlc` crate or `tokio::signal::ctrl_c()` to catch SIGINT/SIGTERM. Set a cancellation flag checked between file copies. Clean up partial files on cancellation.
**Warning signs:** Corrupt destination files after Ctrl+C during watch mode.

## Code Examples

Verified patterns from the existing codebase:

### Reusing Existing Copy Infrastructure
```rust
// Source: src/transfer/mod.rs - existing copy_file_with_progress
use crate::transfer::copy::copy_file_with_progress;
use crate::progress::bar::create_file_progress;

fn sync_single_file(src: &Path, dest: &Path, quiet: bool) -> Result<u64, FluxError> {
    let size = std::fs::metadata(src)?.len();
    // Ensure parent exists
    if let Some(parent) = dest.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let progress = create_file_progress(size, quiet);
    copy_file_with_progress(src, dest, &progress)
}
```

### Extending SyncArgs with New Flags
```rust
// Source: src/cli/args.rs - extend existing SyncArgs struct
#[derive(clap::Args, Debug)]
pub struct SyncArgs {
    /// Source directory
    pub source: String,

    /// Destination directory
    pub dest: String,

    /// Preview sync changes without executing
    #[arg(long)]
    pub dry_run: bool,

    /// Delete files in dest that don't exist in source
    #[arg(long)]
    pub delete: bool,

    /// Watch source for changes and sync continuously
    #[arg(long)]
    pub watch: bool,

    /// Schedule recurring syncs with cron expression (e.g., "*/5 * * * *")
    #[arg(long)]
    pub schedule: Option<String>,

    /// Exclude files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Include only files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub include: Vec<String>,

    /// Verify integrity with BLAKE3 checksum after sync
    #[arg(long)]
    pub verify: bool,
}
```

### SyncEngine Diff Computation
```rust
// Source: pattern derived from rusync + existing copy_directory logic
use walkdir::WalkDir;

fn compute_sync_plan(
    source: &Path,
    dest: &Path,
    filter: &TransferFilter,
    delete_orphans: bool,
) -> Result<SyncPlan, FluxError> {
    let mut actions = Vec::new();
    let mut total_copy_bytes = 0u64;

    // Phase 1: Walk source tree, compare against dest
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !filter.is_excluded_dir(e))
    {
        let entry = entry?;
        if !entry.file_type().is_file() { continue; }
        if !filter.should_transfer(entry.path()) { continue; }

        let relative = entry.path().strip_prefix(source)?;
        let dest_path = dest.join(relative);
        let src_meta = entry.metadata()?;

        match needs_sync(&src_meta, &dest_path) {
            SyncDecision::CopyNew => {
                total_copy_bytes += src_meta.len();
                actions.push(SyncAction::CopyNew {
                    src: entry.path().to_path_buf(),
                    dest: dest_path,
                    size: src_meta.len(),
                });
            }
            SyncDecision::Update => {
                total_copy_bytes += src_meta.len();
                actions.push(SyncAction::UpdateChanged {
                    src: entry.path().to_path_buf(),
                    dest: dest_path.clone(),
                    src_size: src_meta.len(),
                    dest_size: std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0),
                });
            }
            SyncDecision::Skip => {
                actions.push(SyncAction::Skip {
                    path: entry.path().to_path_buf(),
                    reason: "unchanged",
                });
            }
        }
    }

    // Phase 2: Walk dest tree, find orphans (if --delete)
    if delete_orphans && dest.exists() {
        for entry in WalkDir::new(dest)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() { continue; }
            let relative = entry.path().strip_prefix(dest)?;
            let src_path = source.join(relative);
            if !src_path.exists() {
                actions.push(SyncAction::DeleteOrphan {
                    path: entry.path().to_path_buf(),
                    size: entry.metadata().map(|m| m.len()).unwrap_or(0),
                });
            }
        }
    }

    Ok(SyncPlan::from_actions(actions))
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| notify v4-5 with built-in debouncing | notify v8 + separate debouncer crate | notify v6 (2022) | Debouncing is now a separate crate; must add both dependencies |
| `cron` crate using `nom` parser | `cron` v0.15 using `winnow` parser | cron v0.13 (2024) | Faster parsing, but API is the same (`Schedule::from_str`) |
| Custom polling for file watching | OS-native watchers via `notify` | Long-standing | PollWatcher still available as fallback for network/pseudo filesystems |

**Deprecated/outdated:**
- notify v5 `Watcher::with_channel` API: replaced in v6+ with callback-based `recommended_watcher()`
- notify built-in debouncing: removed in v6; use `notify-debouncer-mini` or `notify-debouncer-full`
- `cron` crate old versions using `nom`: current v0.15 uses `winnow`; API unchanged

## Open Questions

1. **Should `--delete` be opt-in or opt-out?**
   - What we know: rsync requires explicit `--delete` flag for safety. This is the standard UX pattern.
   - Recommendation: Make `--delete` opt-in (off by default). This prevents accidental data loss and matches user expectations from rsync.

2. **Should watch mode and schedule mode be mutually exclusive?**
   - What we know: They serve different purposes. Watch mode is for real-time dev workflows. Schedule mode is for periodic batch sync.
   - Recommendation: Make them mutually exclusive. If both `--watch` and `--schedule` are passed, return a clear error. This simplifies the implementation and matches user intent.

3. **Should sync support non-local backends (SFTP, SMB, WebDAV)?**
   - What we know: The `FluxBackend` trait exists with `stat()`, `list_dir()`, etc. for all backends. However, `notify` only works on local filesystems.
   - Recommendation: v1 sync works local-to-local only. The `compute_sync_plan` should use `std::fs` directly for simplicity. Network backend sync can be added later using `FluxBackend::stat()` and `FluxBackend::list_dir()` for comparison.

4. **What debounce timeout for watch mode?**
   - What we know: Too short = multiple syncs for one save. Too long = noticeable delay. Common values: 500ms-2s.
   - Recommendation: Default 2 seconds. This handles most editors (which do save+rename sequences). Could be a CLI flag later if users want to tune it.

5. **Handling empty source in `--delete` mode**
   - What we know: An empty source with `--delete` would delete everything in dest. This is catastrophic.
   - Recommendation: If `--delete` is set and source has 0 transferable files after filtering, refuse to proceed and print a warning. Require `--force` or similar to override.

## Sources

### Primary (HIGH confidence)
- [notify 8.2.0 docs](https://docs.rs/notify/latest/notify/) - API overview, platform backends, RecommendedWatcher
- [notify-debouncer-full 0.7.0 docs](https://docs.rs/notify-debouncer-full/latest/notify_debouncer_full/) - Debouncer API, new_debouncer usage
- [cron 0.15.0 docs](https://docs.rs/cron/latest/cron/) - Schedule parsing, upcoming() iteration
- Existing codebase: `src/transfer/mod.rs` (copy_directory, dry_run_directory, TransferFilter, walkdir patterns)
- Existing codebase: `src/backend/mod.rs` (FileStat with modified time, FluxBackend trait)
- Existing codebase: `src/cli/args.rs` (SyncArgs skeleton with source, dest, dry_run)

### Secondary (MEDIUM confidence)
- [notify-rs/notify GitHub](https://github.com/notify-rs/notify) - 62.7M total downloads, used by rust-analyzer, deno, zed, cargo-watch
- [cron GitHub](https://github.com/zslayton/cron) - Standard cron expression parser for Rust
- [rusync](https://github.com/your-tools/rusync) - Reference implementation of one-way sync: copy when dest missing, older, or different size
- [File synchronisation algorithms](https://ianhowson.com/blog/file-synchronisation-algorithms/) - Algorithm overview: simple state comparison vs. history-based
- [LuminS](https://github.com/wchang22/LuminS) - Multithreaded directory sync with hash verification

### Tertiary (LOW confidence)
- None. All findings verified with at least two sources.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - notify, cron are clearly the ecosystem standards with massive adoption
- Architecture: HIGH - Diff-then-execute pattern is well-established (rsync, rusync, robocopy); existing codebase provides strong foundation
- Pitfalls: HIGH - All pitfalls are well-documented in community (inotify limits, cross-platform mtime, symlinks)

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (stable domain, libraries mature)
