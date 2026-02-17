# Phase 4: User Experience - Research

**Researched:** 2026-02-16
**Domain:** Path alias management, transfer queue, configuration system, shell completions, dry-run mode
**Confidence:** HIGH

## Summary

Phase 4 transforms Flux from a raw transfer engine into a productive daily-use tool. It covers three major subsystems (path aliases, transfer queue, configuration) plus two cross-cutting features (shell completions, dry-run mode). The existing codebase provides a solid foundation: `clap` derive-based CLI with a single `Cp` command, a `FluxConfig` skeleton in `src/config/types.rs`, TOML + serde + `dirs` crate already in `Cargo.toml`, and a `Protocol` enum with `detect_protocol()` parser that needs to be extended to resolve aliases before protocol detection.

The key architectural decision is the queue model. A daemon-based approach (like pueue) provides true background execution but adds massive complexity (IPC, process management, service lifecycle). For a CLI file transfer tool focused on "zero friction," a simpler **in-process queue with persistent state file** is the right fit: the user queues transfers in a JSON state file, and a single `flux queue run` command processes them sequentially/in-parallel in the foreground. Pause/resume/cancel operate on the state file. This avoids daemon complexity while satisfying all QUEUE requirements.

**Primary recommendation:** Use `dirs::config_dir()` (already available) for config/alias/history storage in `~/.config/flux/` (Linux), `%APPDATA%/flux/` (Windows), `~/Library/Application Support/flux/` (macOS). Store config as `config.toml`, aliases as `aliases.toml`, queue state as `queue.json`, and history as `history.json`. Use `clap_complete` (v4.5) for shell completion generation via a `completions` subcommand. Implement dry-run as a global `--dry-run` flag that runs the full operation pipeline but replaces actual I/O with logging.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| PATH-01 | User can save named path aliases (`flux add nas \\server\share`) | New `Add` and `Remove` subcommands in clap CLI. Aliases stored in `aliases.toml` via serde. AliasStore struct with CRUD operations. |
| PATH-02 | User can use aliases in commands (`flux cp file.txt nas:`) | Alias resolution layer in `execute_copy()` before `detect_protocol()`. Pattern: if string matches `<name>:` or `<name>:<subpath>`, expand from AliasStore, then pass to protocol detection. |
| PATH-03 | User can set default destination for quick sends | Special alias name `default` in aliases.toml, or dedicated `default_destination` field in config.toml. Used when dest is omitted or set to `.` shorthand. |
| PATH-04 | User can view and reuse path history (`flux history`) | New `History` subcommand. Transfer completions appended to `history.json` with timestamp, source, dest, bytes, duration. HistoryStore with append + query operations. |
| PATH-05 | User can list and remove saved aliases | `flux alias` (list), `flux alias rm <name>` (remove). AliasStore::list() and AliasStore::remove() methods. |
| QUEUE-01 | User can queue multiple transfers | New `Queue` subcommand with `add` sub-action. QueueStore persists transfer specs to `queue.json`. Each entry gets a unique ID (incrementing u64). |
| QUEUE-02 | User can view queue status | `flux queue` or `flux queue list` shows pending/running/completed/failed entries with progress info. |
| QUEUE-03 | User can pause/resume individual transfers | `flux queue pause <id>` / `flux queue resume <id>`. State transitions in queue.json: Running->Paused, Paused->Pending. During execution, check pause flag before each file in directory copy or between chunks. |
| QUEUE-04 | User can cancel transfers | `flux queue cancel <id>`. Marks entry as Cancelled in queue.json. Running transfers check cancellation flag and stop gracefully. |
| QUEUE-05 | User can view transfer history/logs | `flux history` shows completed transfers with timestamps, sizes, speeds. Persisted in `history.json`. Shares display with PATH-04. |
| CONF-01 | User can configure conflict handling (overwrite/skip/rename/ask) | `ConflictStrategy` enum in config. Applied in `execute_copy()` when destination file exists. `--on-conflict` CLI flag overrides config. |
| CONF-02 | User can configure failure handling (retry/pause/skip) | `FailureStrategy` enum in config. Applied per-file in directory copy error handling. `--on-error` CLI flag overrides config. |
| CONF-03 | User can configure retry count and backoff | `retry_count` (u32, default 3) and `retry_backoff_ms` (u64, default 1000) in config. Exponential backoff: delay * 2^attempt. |
| CONF-05 | User can preview operations with dry-run mode | Global `--dry-run` flag on Cli struct. When active, walk source files and report what would happen (copy/skip/overwrite/rename) without performing I/O. |
| CLI-05 | Tool supports shell completion (bash, zsh, fish, powershell) | `flux completions <shell>` subcommand using `clap_complete` crate. Generates completion script to stdout. User pipes to shell-specific location. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `clap` | 4.5 | CLI parsing with derive macros | Already in use. Add new subcommands (add, alias, queue, history, completions). Global --dry-run and --on-conflict flags. |
| `clap_complete` | 4.5 | Shell completion script generation | Official companion to clap. Supports Bash, Zsh, Fish, PowerShell, Elvish. 4.6M downloads/month. |
| `serde` | 1.0 | Serialization framework | Already in use. Derive Serialize/Deserialize for config, alias, queue, history structs. |
| `toml` | 0.8 | TOML config file format | Already in use. Human-readable config files with clear sections. |
| `serde_json` | 1.0 | JSON serialization | Already in use. For queue state and history (append-friendly, machine-readable). |
| `dirs` | 5 | Platform-specific config/data directories | Already in use. `dirs::config_dir()` for config, `dirs::data_dir()` for queue/history. |
| `chrono` | 0.4 | Timestamps for history entries | Widely used (39M downloads/month). Serde support via `chrono::serde::ts_seconds`. Needed for transfer history timestamps. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `uuid` | 1.0 | Unique transfer IDs in queue | Only if incrementing u64 IDs prove insufficient. Likely not needed for single-user CLI. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `chrono` for timestamps | `std::time::SystemTime` | SystemTime lacks formatting/parsing. chrono adds ~200KB but provides RFC3339 serde out of box. |
| TOML for all storage | JSON for all storage | TOML is more human-readable for config/aliases. JSON is better for append-only logs (history) and structured state (queue). Use both. |
| File-based queue | pueue-style daemon | Daemon enables true background execution but adds IPC, service management, platform-specific daemonization. Massive complexity for a CLI tool. File-based queue with foreground execution is simpler and sufficient. |
| `confy` for config | Manual toml load/save | confy adds magic (auto-creates files, auto-derives paths) but hides control. Manual approach gives explicit paths, better error messages, and config validation. Already have toml + dirs. |
| `clap_complete` | `clap_complete_command` | `clap_complete_command` reduces boilerplate but adds another dependency. Standard `clap_complete` is maintained by clap team and well-documented. |

**Installation:**
```toml
[dependencies]
# Add to existing Cargo.toml
clap_complete = "4.5"
chrono = { version = "0.4", features = ["serde"] }
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── cli/
│   ├── args.rs          # Extended: Add, Alias, Queue, History, Completions subcommands
│   └── mod.rs           # Command dispatch
├── config/
│   ├── mod.rs           # Config loading/saving, FluxConfig with all settings
│   ├── types.rs         # Verbosity (existing), ConflictStrategy, FailureStrategy enums
│   ├── aliases.rs       # AliasStore: load/save/add/remove/list/resolve
│   └── paths.rs         # flux_config_dir(), flux_data_dir() helpers
├── queue/
│   ├── mod.rs           # QueueStore: add/list/pause/resume/cancel/run
│   ├── state.rs         # QueueEntry, QueueStatus enum, persistence
│   └── history.rs       # HistoryEntry, HistoryStore: append/list
├── transfer/
│   ├── mod.rs           # Updated: alias resolution, conflict handling, dry-run
│   ├── conflict.rs      # ConflictStrategy resolution logic
│   └── ...              # Existing modules unchanged
├── backend/             # Unchanged
├── protocol/            # Unchanged (alias resolution happens BEFORE protocol detection)
├── progress/            # Unchanged
└── error.rs             # Extended: AliasError, QueueError variants
```

### Pattern 1: Alias Resolution Before Protocol Detection
**What:** Intercept source/dest strings, check for `name:` or `name:subpath` alias patterns, expand to full path, then pass to `detect_protocol()`.
**When to use:** Every time source or dest is provided by the user in any command.
**Example:**
```rust
// Source: custom implementation
use crate::config::aliases::AliasStore;

/// Resolve alias references in a path string.
/// "nas:" -> "\\\\server\\share"
/// "nas:docs/file.txt" -> "\\\\server\\share\\docs\\file.txt"
/// "regular/path" -> "regular/path" (unchanged)
pub fn resolve_alias(input: &str, aliases: &AliasStore) -> String {
    // Check for alias pattern: word followed by colon
    if let Some(colon_pos) = input.find(':') {
        let name = &input[..colon_pos];
        // Don't match URL schemes (sftp://, https://) or Windows drive letters (C:)
        if !name.is_empty()
            && !name.contains('/')
            && !name.contains('\\')
            && name.len() > 1  // Skip single-char (drive letters)
            && !input[colon_pos..].starts_with("//")  // Skip URL schemes
        {
            if let Some(base_path) = aliases.get(name) {
                let subpath = &input[colon_pos + 1..];
                if subpath.is_empty() {
                    return base_path.clone();
                }
                // Join base path with subpath
                let separator = if base_path.contains('\\') { "\\" } else { "/" };
                return format!("{}{}{}", base_path, separator, subpath);
            }
        }
    }
    input.to_string()
}
```

### Pattern 2: TOML Config with Serde Defaults
**What:** Define config struct with `#[serde(default)]` so missing fields use defaults. Load from file, fall back to defaults if file missing.
**When to use:** FluxConfig loading at startup.
**Example:**
```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FluxConfig {
    pub conflict: ConflictStrategy,
    pub failure: FailureStrategy,
    pub retry_count: u32,
    pub retry_backoff_ms: u64,
    pub default_destination: Option<String>,
    pub parallel_chunks: Option<usize>,
    pub verify: bool,
    pub compress: bool,
}

impl Default for FluxConfig {
    fn default() -> Self {
        Self {
            conflict: ConflictStrategy::Ask,
            failure: FailureStrategy::Retry,
            retry_count: 3,
            retry_backoff_ms: 1000,
            default_destination: None,
            parallel_chunks: None,
            verify: false,
            compress: false,
        }
    }
}

/// Load config from disk, creating default if not found.
pub fn load_config() -> Result<FluxConfig, FluxError> {
    let config_path = flux_config_dir()?.join("config.toml");
    if config_path.exists() {
        let contents = std::fs::read_to_string(&config_path)?;
        let config: FluxConfig = toml::from_str(&contents)
            .map_err(|e| FluxError::Config(format!("Invalid config: {}", e)))?;
        Ok(config)
    } else {
        Ok(FluxConfig::default())
    }
}
```

### Pattern 3: File-Based Queue with JSON State
**What:** Queue entries stored in a single JSON file. Each entry has an ID, status, and transfer spec. Queue operations are atomic file writes.
**When to use:** All queue operations (add, list, pause, resume, cancel, run).
**Example:**
```rust
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueEntry {
    pub id: u64,
    pub status: QueueStatus,
    pub source: String,
    pub dest: String,
    pub recursive: bool,
    pub verify: bool,
    pub compress: bool,
    pub added_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub bytes_transferred: u64,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum QueueStatus {
    Pending,
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
}

pub struct QueueStore {
    path: PathBuf,
    entries: Vec<QueueEntry>,
    next_id: u64,
}

impl QueueStore {
    pub fn load(data_dir: &Path) -> Result<Self, FluxError> {
        let path = data_dir.join("queue.json");
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            let entries: Vec<QueueEntry> = serde_json::from_str(&contents)?;
            let next_id = entries.iter().map(|e| e.id).max().unwrap_or(0) + 1;
            Ok(Self { path, entries, next_id })
        } else {
            Ok(Self { path, entries: Vec::new(), next_id: 1 })
        }
    }

    pub fn save(&self) -> Result<(), FluxError> {
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    pub fn add(&mut self, source: String, dest: String, opts: TransferOpts) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.entries.push(QueueEntry {
            id,
            status: QueueStatus::Pending,
            source,
            dest,
            recursive: opts.recursive,
            verify: opts.verify,
            compress: opts.compress,
            added_at: Utc::now(),
            started_at: None,
            completed_at: None,
            bytes_transferred: 0,
            error: None,
        });
        id
    }
}
```

### Pattern 4: Shell Completion Subcommand
**What:** A `completions` subcommand that generates shell completion scripts to stdout.
**When to use:** `flux completions bash > ~/.bash_completion.d/flux`
**Example:**
```rust
use clap::CommandFactory;
use clap_complete::{generate, Shell};

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Copy files or directories
    Cp(CpArgs),
    /// Save a path alias
    Add(AddArgs),
    /// Manage path aliases
    Alias(AliasArgs),
    /// Manage transfer queue
    Queue(QueueArgs),
    /// View transfer history
    History(HistoryArgs),
    /// Generate shell completions
    Completions(CompletionsArgs),
}

#[derive(clap::Args, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: Shell,
}

// In main dispatch:
Commands::Completions(args) => {
    let mut cmd = Cli::command();
    generate(args.shell, &mut cmd, "flux", &mut std::io::stdout());
    Ok(())
}
```

### Pattern 5: Conflict Resolution Strategy
**What:** Check if destination exists before copy, apply configured strategy.
**When to use:** Before every file copy operation.
**Example:**
```rust
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictStrategy {
    Overwrite,
    Skip,
    Rename,
    Ask,
}

/// Resolve a file conflict. Returns the final destination path, or None to skip.
pub fn resolve_conflict(
    dest: &Path,
    strategy: ConflictStrategy,
    quiet: bool,
) -> Result<Option<PathBuf>, FluxError> {
    if !dest.exists() {
        return Ok(Some(dest.to_path_buf()));
    }

    match strategy {
        ConflictStrategy::Overwrite => Ok(Some(dest.to_path_buf())),
        ConflictStrategy::Skip => {
            if !quiet {
                eprintln!("Skipped (exists): {}", dest.display());
            }
            Ok(None)
        }
        ConflictStrategy::Rename => {
            // Generate unique name: file.txt -> file_1.txt, file_2.txt, etc.
            let renamed = find_unique_name(dest);
            if !quiet {
                eprintln!("Renamed: {} -> {}", dest.display(), renamed.display());
            }
            Ok(Some(renamed))
        }
        ConflictStrategy::Ask => {
            // Interactive prompt to stderr
            eprint!("{} exists. (o)verwrite / (s)kip / (r)ename? ", dest.display());
            let mut input = String::new();
            std::io::stdin().read_line(&mut input)?;
            match input.trim().to_lowercase().as_str() {
                "o" | "overwrite" => Ok(Some(dest.to_path_buf())),
                "s" | "skip" => Ok(None),
                "r" | "rename" => Ok(Some(find_unique_name(dest))),
                _ => Ok(None), // Default to skip on invalid input
            }
        }
    }
}

fn find_unique_name(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let ext = path.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    let parent = path.parent().unwrap_or(Path::new("."));
    for i in 1..=9999 {
        let candidate = parent.join(format!("{}_{}{}", stem, i, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    // Fallback: append timestamp
    parent.join(format!("{}_{}{}", stem, chrono::Utc::now().timestamp(), ext))
}
```

### Pattern 6: Dry-Run Mode
**What:** Global flag that walks the operation pipeline, reports what would happen, but performs no I/O.
**When to use:** `flux cp -r folder/ dest/ --dry-run`
**Example:**
```rust
// In Cli struct (global flag):
#[arg(long, global = true)]
pub dry_run: bool,

// In execute_copy, after all validation and alias resolution:
if dry_run {
    if source_meta.is_file() {
        let action = if dest.exists() {
            match conflict_strategy {
                ConflictStrategy::Overwrite => "overwrite",
                ConflictStrategy::Skip => "skip",
                ConflictStrategy::Rename => "rename",
                ConflictStrategy::Ask => "overwrite (ask)",
            }
        } else {
            "copy"
        };
        eprintln!("[dry-run] {} {} -> {} ({} bytes)",
            action, source.display(), dest.display(), source_meta.len());
    } else if source_meta.is_dir() {
        // Walk directory, report each file
        for entry in WalkDir::new(source) {
            // ... report copy/skip/overwrite for each
        }
    }
    return Ok(());
}
```

### Anti-Patterns to Avoid
- **Daemon for queue management:** A background daemon adds enormous complexity (IPC, process lifecycle, platform differences for daemonization, service management). For a CLI tool, file-based state with foreground execution is simpler, testable, and sufficient. Users who need background execution can use OS tools (nohup, screen, tmux, Task Scheduler).
- **Storing aliases in the same file as config:** Separate files (config.toml vs aliases.toml) allow independent editing and cleaner separation of concerns. Config rarely changes; aliases change frequently.
- **Resolving aliases inside detect_protocol():** Alias resolution is a user-facing convenience layer that should happen BEFORE protocol detection. detect_protocol() should remain a pure string-to-Protocol parser with no side effects or file I/O.
- **Making dry-run a separate command:** Dry-run should be a flag on the existing commands (cp, queue run), not a separate `flux dry-run cp ...` command. Users expect `--dry-run` to modify behavior of existing operations.
- **Interactive prompts without fallback:** The `ask` conflict strategy uses stdin. When stdin is not a TTY (piped input, scripts), fall back to `skip` to avoid blocking. Check with `atty::is(atty::Stream::Stdin)` or equivalent.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Shell completion scripts | Custom completion generators per shell | `clap_complete` crate | Each shell has different completion syntax (Bash uses `complete -F`, Zsh uses `compdef`, Fish uses `complete -c`, PowerShell uses `Register-ArgumentCompleter`). clap_complete handles all variants from the Command definition. |
| Platform-specific config paths | Hardcoded `~/.config/flux` | `dirs::config_dir()` | Windows uses `%APPDATA%`, macOS uses `~/Library/Application Support`, Linux uses `~/.config`. dirs handles all platforms including XDG overrides. |
| TOML parsing/serialization | Manual string building | `toml` crate + serde derive | TOML has subtle syntax rules (quoting, escaping, inline tables). serde derive generates correct round-trip serialization. |
| Unique filename generation | Random suffix | Sequential numbering (`file_1.txt`, `file_2.txt`) | Sequential names are predictable and human-friendly. Random suffixes make it hard to find renamed files. |

**Key insight:** The User Experience phase is glue code connecting well-solved library problems (config paths, TOML parsing, shell completions) to the existing transfer engine. The custom logic is in the integration: alias resolution piping into protocol detection, conflict resolution injected into the copy loop, queue state management across CLI invocations.

## Common Pitfalls

### Pitfall 1: Alias Pattern Collision with URL Schemes and Drive Letters
**What goes wrong:** `sftp:` looks like an alias. `C:` looks like an alias. `nas:` is a valid alias.
**Why it happens:** The colon character is used in aliases (`nas:`), URL schemes (`sftp://`), and Windows drive letters (`C:\`).
**How to avoid:** Alias resolution must explicitly exclude: (1) single-character names (drive letters), (2) names followed by `//` (URL schemes), (3) names containing path separators. Check these before alias lookup.
**Warning signs:** `flux cp file.txt sftp://server/path` tries to look up an alias named "sftp" instead of using the SFTP protocol.

### Pitfall 2: Queue State File Corruption on Crash
**What goes wrong:** If the process is killed mid-write to queue.json, the file may be truncated or invalid JSON.
**Why it happens:** std::fs::write is not atomic.
**How to avoid:** Write to a temporary file first (`queue.json.tmp`), then rename atomically. `std::fs::rename` is atomic on most filesystems. This is the same pattern used for resume manifests.
**Warning signs:** `flux queue` shows "Failed to parse queue state" after a crash.

### Pitfall 3: Config File Validation Errors Blocking All Commands
**What goes wrong:** A typo in config.toml causes all flux commands to fail, even `flux --help`.
**Why it happens:** Config loading happens in main before command dispatch.
**How to avoid:** Load config lazily (only when needed by the command). Commands like `completions`, `alias list`, and `--help` should work without valid config. If config fails to parse, warn but use defaults.
**Warning signs:** User edits config.toml, then `flux --help` fails with "Invalid config".

### Pitfall 4: History File Growing Unbounded
**What goes wrong:** After thousands of transfers, history.json becomes multiple megabytes. Loading it slows down every `flux history` invocation.
**Why it happens:** Append-only log without rotation.
**How to avoid:** Cap history entries (e.g., 1000 most recent). On load, if entries exceed cap, truncate oldest. Or use a line-delimited JSON (JSONL) format and only read the last N lines. Alternatively, configurable `history_limit` in config.
**Warning signs:** `flux history` becomes slow over time.

### Pitfall 5: Shell Completion Script Path Confusion
**What goes wrong:** User generates completions but puts them in the wrong directory, or the completions never load.
**Why it happens:** Each shell has a different completion directory, and the user may not know where to put the output.
**How to avoid:** Print clear instructions after generation: "Source this file from your shell profile" with shell-specific examples. Include common paths in help text.
**Warning signs:** User reports "completions don't work" even after running `flux completions bash`.

### Pitfall 6: Dry-Run Not Reflecting Actual Behavior
**What goes wrong:** Dry-run says "would copy 50 files" but actual run copies 48 because of conflicts or permission errors.
**Why it happens:** Dry-run and actual execution take different code paths. Dry-run doesn't check permissions, doesn't resolve conflicts interactively, etc.
**How to avoid:** Share as much code as possible between dry-run and actual execution. Dry-run should run the same validation, filtering, and conflict detection -- just skip the actual I/O. Use a `DryRun` flag parameter through the call chain rather than a separate code path.
**Warning signs:** Users report "dry-run showed X but actual run did Y".

### Pitfall 7: Concurrent Queue Access
**What goes wrong:** Two `flux queue add` invocations at the same time overwrite each other's additions.
**Why it happens:** File-based state without locking.
**How to avoid:** Use file locking (`fs2` crate or platform lock files) when reading/writing queue.json. A simple advisory lock file (`queue.lock`) prevents concurrent modification. For a single-user CLI tool, this is unlikely but should be handled gracefully.
**Warning signs:** Queue entries disappear after rapid successive additions.

## Code Examples

Verified patterns from official sources:

### Shell Completion Generation with clap_complete
```rust
// Source: clap_complete docs + completion-derive.rs example
use clap::{CommandFactory, Parser, Subcommand};
use clap_complete::{generate, Shell};
use std::io;

#[derive(Parser)]
#[command(name = "flux")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        #[arg(value_enum)]
        shell: Shell,
    },
}

fn print_completions(shell: Shell) {
    let mut cmd = Cli::command();
    generate(shell, &mut cmd, "flux", &mut io::stdout());
}
```

### Config Directory Discovery with dirs crate
```rust
// Source: docs.rs/dirs/5.0.1/dirs/
use std::path::PathBuf;

/// Get the Flux config directory, creating it if needed.
/// Linux:   ~/.config/flux/
/// Windows: C:\Users\<user>\AppData\Roaming\flux\
/// macOS:   ~/Library/Application Support/flux/
pub fn flux_config_dir() -> Result<PathBuf, FluxError> {
    let base = dirs::config_dir()
        .ok_or_else(|| FluxError::Config("Could not determine config directory".into()))?;
    let flux_dir = base.join("flux");
    if !flux_dir.exists() {
        std::fs::create_dir_all(&flux_dir)?;
    }
    Ok(flux_dir)
}

/// Get the Flux data directory for queue and history.
/// Linux:   ~/.local/share/flux/
/// Windows: C:\Users\<user>\AppData\Roaming\flux\
/// macOS:   ~/Library/Application Support/flux/
pub fn flux_data_dir() -> Result<PathBuf, FluxError> {
    let base = dirs::data_dir()
        .ok_or_else(|| FluxError::Config("Could not determine data directory".into()))?;
    let flux_dir = base.join("flux");
    if !flux_dir.exists() {
        std::fs::create_dir_all(&flux_dir)?;
    }
    Ok(flux_dir)
}
```

### TOML Alias Store
```rust
// Source: toml + serde derive pattern
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AliasFile {
    #[serde(default)]
    pub aliases: BTreeMap<String, String>,
}

pub struct AliasStore {
    path: PathBuf,
    data: AliasFile,
}

impl AliasStore {
    pub fn load(config_dir: &Path) -> Result<Self, FluxError> {
        let path = config_dir.join("aliases.toml");
        let data = if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            toml::from_str(&contents)
                .map_err(|e| FluxError::Config(format!("Invalid aliases.toml: {}", e)))?
        } else {
            AliasFile::default()
        };
        Ok(Self { path, data })
    }

    pub fn save(&self) -> Result<(), FluxError> {
        let contents = toml::to_string_pretty(&self.data)
            .map_err(|e| FluxError::Config(format!("Failed to serialize aliases: {}", e)))?;
        // Atomic write: write to tmp, then rename
        let tmp_path = self.path.with_extension("toml.tmp");
        std::fs::write(&tmp_path, contents)?;
        std::fs::rename(&tmp_path, &self.path)?;
        Ok(())
    }

    pub fn add(&mut self, name: String, path: String) {
        self.data.aliases.insert(name, path);
    }

    pub fn remove(&mut self, name: &str) -> bool {
        self.data.aliases.remove(name).is_some()
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.data.aliases.get(name)
    }

    pub fn list(&self) -> &BTreeMap<String, String> {
        &self.data.aliases
    }
}
```

### Transfer History with Chrono Timestamps
```rust
// Source: chrono serde docs + serde_json
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub source: String,
    pub dest: String,
    pub bytes: u64,
    pub files: u64,
    pub duration_secs: f64,
    pub timestamp: DateTime<Utc>,
    pub status: String,  // "completed", "failed", "cancelled"
    pub error: Option<String>,
}

pub struct HistoryStore {
    path: PathBuf,
    entries: Vec<HistoryEntry>,
    limit: usize,
}

impl HistoryStore {
    pub fn load(data_dir: &Path, limit: usize) -> Result<Self, FluxError> {
        let path = data_dir.join("history.json");
        let entries = if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            serde_json::from_str(&contents).unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(Self { path, entries, limit })
    }

    pub fn append(&mut self, entry: HistoryEntry) -> Result<(), FluxError> {
        self.entries.push(entry);
        // Truncate to limit
        if self.entries.len() > self.limit {
            let excess = self.entries.len() - self.limit;
            self.entries.drain(..excess);
        }
        self.save()
    }

    fn save(&self) -> Result<(), FluxError> {
        let json = serde_json::to_string_pretty(&self.entries)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `clap_generate` crate | `clap_complete` (same team, renamed) | clap 4.x (2022) | Import path changed. `clap_complete` is the canonical name now. |
| Build-time completion scripts only | Runtime + `CompleteEnv` dynamic completions | clap_complete 4.4+ (2024) | `COMPLETE=bash flux` enables dynamic completions without pre-generated scripts. Still unstable/experimental. |
| `dirs` v4 (deprecated) | `dirs` v5 (current) | 2023 | macOS config_dir changed to Application Support. Already using v5 in Cargo.toml. |
| `chrono` pre-0.4.20 (RUSTSEC advisory) | `chrono` 0.4.38+ (fixed) | 2023+ | Security advisory resolved. Safe to use current chrono. |

**Deprecated/outdated:**
- `clap_generate` crate: Renamed to `clap_complete`. Do not use the old name.
- `dirs-next` crate: Fork of `dirs` that is no longer needed; `dirs` v5 is maintained.
- `app_dirs` crate: Abandoned. Use `dirs` or `directories` instead.

## Open Questions

1. **Queue execution model: foreground vs background**
   - What we know: A foreground model (user runs `flux queue run` and it processes entries until done) is simple. A daemon model enables true background processing but adds massive complexity.
   - What's unclear: Will users expect `flux queue add ... && flux queue add ...` to start processing immediately in the background?
   - Recommendation: Start with foreground execution (`flux queue run`). Document that users can use OS tools (nohup, screen, Task Scheduler) for background execution. If demand exists, add daemon mode in a future phase.

2. **Config init / first-run experience**
   - What we know: First time a user runs flux, no config directory or files exist.
   - What's unclear: Should `flux` auto-create `config.toml` with defaults on first run, or only create it when user explicitly configures something?
   - Recommendation: Auto-create the config directory but NOT config.toml. Use in-memory defaults. Only write config.toml when user explicitly sets a value (e.g., `flux config set conflict skip`). This avoids surprising users with files they didn't create.

3. **Alias name validation rules**
   - What we know: Aliases must not collide with URL schemes (sftp, https, smb) or drive letters (C, D).
   - What's unclear: Should we enforce strict naming (alphanumeric + hyphens only)? Allow Unicode?
   - Recommendation: Alphanumeric + hyphens + underscores. Reject single characters, reject known URL schemes, reject names starting with a digit. Validate on add, not on use.

4. **Default destination behavior**
   - What we know: PATH-03 requires "set default destination for quick sends."
   - What's unclear: What command syntax triggers the default? `flux send file.txt`? `flux cp file.txt`?
   - Recommendation: Support `flux cp file.txt` (no dest) when a default is configured. Error with helpful message if no default is set. Also support explicit `flux cp file.txt default:` syntax.

## Sources

### Primary (HIGH confidence)
- [clap_complete crate docs](https://docs.rs/clap_complete/latest/clap_complete/) - Version 4.5.66, Shell enum, generate() function, supported shells (Bash, Zsh, Fish, PowerShell, Elvish)
- [clap_complete shells module](https://docs.rs/clap_complete/latest/clap_complete/shells/index.html) - Shell variants and aot module
- [clap completion-derive.rs example](https://github.com/clap-rs/clap/blob/master/clap_complete/examples/completion-derive.rs) - Official derive-based completion example
- [dirs crate docs](https://docs.rs/dirs/5.0.1/dirs/) - v5, config_dir/data_dir cross-platform paths
- [toml crate docs](https://docs.rs/toml/latest/toml/) - Serde-based TOML serialization
- [chrono crate docs](https://docs.rs/chrono/latest/chrono/) - DateTime, Utc, serde support
- Existing codebase: `src/cli/args.rs`, `src/config/types.rs`, `src/protocol/parser.rs`, `src/transfer/mod.rs`, `Cargo.toml`

### Secondary (MEDIUM confidence)
- [pueue architecture](https://github.com/Nukesor/pueue/blob/main/docs/Architecture.md) - Daemon-based queue management, Unix socket IPC, state persistence pattern
- [confy crate](https://github.com/rust-cli/confy) - Zero-boilerplate config management (considered but rejected: too much magic)
- [Persistent task queue blog post](https://jmmv.dev/2023/06/iii-iv-task-queue.html) - JSON-based task persistence with write-ahead journal pattern

### Tertiary (LOW confidence)
- [Files app conflict resolution issue](https://github.com/files-community/Files/issues/7681) - Skip/Overwrite/Rename patterns in file managers (design reference, not Rust-specific)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All libraries already in Cargo.toml except `clap_complete` and `chrono`. Both are extremely well-established (clap_complete is official clap companion, chrono has 39M downloads/month). No unknowns.
- Architecture: HIGH - Pattern is clear: config/alias files in platform dirs, JSON state for queue/history, alias resolution layer before protocol detection, conflict resolution injected into copy loop. No daemon complexity. Well-tested patterns from existing codebase (resume manifests use same serde+JSON approach).
- Pitfalls: HIGH - Common issues well-documented: alias/URL scheme collision, concurrent file access, config validation blocking, history growth. All have clear mitigation strategies.

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (30 days - these libraries are stable and well-established)
