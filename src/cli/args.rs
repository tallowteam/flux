use clap::{Parser, Subcommand};

use crate::config::types::{ConflictStrategy, FailureStrategy};

#[derive(Parser, Debug)]
#[command(name = "flux", version, about = "Blazing-fast file transfer")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,

    /// Increase verbosity (-v for verbose, -vv for trace)
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    /// Quiet mode: suppress all output except errors
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Launch interactive TUI mode
    #[arg(long, global = true)]
    pub tui: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Copy files or directories
    Cp(CpArgs),

    /// Save a path alias (e.g., flux add nas \\\\server\\share)
    Add(AddArgs),

    /// Manage path aliases
    Alias(AliasArgs),

    /// Manage transfer queue
    Queue(QueueArgs),

    /// View transfer history
    History(HistoryArgs),

    /// Generate shell completions
    Completions(CompletionsArgs),

    /// Discover Flux devices on the local network
    Discover(DiscoverArgs),

    /// Send a file to another Flux device
    Send(SendArgs),

    /// Receive files from other Flux devices
    Receive(ReceiveArgs),

    /// Manage trusted devices
    Trust(TrustArgs),

    /// Launch interactive TUI mode
    Ui,

    /// Sync directories (one-way mirror)
    Sync(SyncArgs),
}

#[derive(clap::Args, Debug)]
pub struct CpArgs {
    /// Source path or URI (e.g., file.txt, sftp://host/path, \\\\server\\share)
    pub source: String,

    /// Destination path or URI (e.g., file.txt, sftp://host/path, \\\\server\\share)
    pub dest: String,

    /// Copy directories recursively
    #[arg(short, long)]
    pub recursive: bool,

    /// Exclude files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,

    /// Include only files matching glob pattern (can be repeated)
    #[arg(long, action = clap::ArgAction::Append)]
    pub include: Vec<String>,

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

    /// Conflict handling when destination file exists: overwrite, skip, rename, ask
    #[arg(long, value_enum)]
    pub on_conflict: Option<ConflictStrategy>,

    /// Failure handling when a copy operation fails: retry, skip, pause
    #[arg(long, value_enum)]
    pub on_error: Option<FailureStrategy>,

    /// Preview operations without performing them
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for the `flux add` command.
#[derive(clap::Args, Debug)]
pub struct AddArgs {
    /// Name for the alias (e.g., nas, backup, server)
    pub name: String,

    /// Path or URI to associate (e.g., \\\\server\\share, sftp://host/path)
    pub path: String,
}

/// Arguments for the `flux alias` command.
#[derive(clap::Args, Debug)]
pub struct AliasArgs {
    #[command(subcommand)]
    pub action: Option<AliasAction>,
}

/// Subcommands for alias management.
#[derive(Subcommand, Debug)]
pub enum AliasAction {
    /// Remove a saved alias
    Rm(AliasRmArgs),
}

/// Arguments for `flux alias rm`.
#[derive(clap::Args, Debug)]
pub struct AliasRmArgs {
    /// Name of alias to remove
    pub name: String,
}

/// Arguments for the `flux queue` command.
#[derive(clap::Args, Debug)]
pub struct QueueArgs {
    #[command(subcommand)]
    pub action: Option<QueueAction>,
}

/// Subcommands for queue management.
#[derive(Subcommand, Debug)]
pub enum QueueAction {
    /// Add a transfer to the queue
    Add(QueueAddArgs),
    /// List queued transfers
    List,
    /// Pause a queued transfer
    Pause(QueueIdArgs),
    /// Resume a paused transfer
    Resume(QueueIdArgs),
    /// Cancel a queued transfer
    Cancel(QueueIdArgs),
    /// Process all pending transfers in the queue
    Run,
    /// Clear completed/failed/cancelled entries
    Clear,
}

/// Arguments for `flux queue add`.
#[derive(clap::Args, Debug)]
pub struct QueueAddArgs {
    /// Source path or URI
    pub source: String,
    /// Destination path or URI
    pub dest: String,
    /// Copy directories recursively
    #[arg(short, long)]
    pub recursive: bool,
    /// Verify transfer integrity
    #[arg(long)]
    pub verify: bool,
    /// Enable compression
    #[arg(long)]
    pub compress: bool,
}

/// Arguments for queue commands that take a job ID.
#[derive(clap::Args, Debug)]
pub struct QueueIdArgs {
    /// Transfer ID
    pub id: u64,
}

/// Arguments for the `flux history` command.
#[derive(clap::Args, Debug)]
pub struct HistoryArgs {
    /// Maximum number of entries to show
    #[arg(short = 'n', long, default_value = "20")]
    pub count: usize,
    /// Clear all history
    #[arg(long)]
    pub clear: bool,
}

/// Arguments for the `flux completions` command.
#[derive(clap::Args, Debug)]
pub struct CompletionsArgs {
    /// Shell to generate completions for
    #[arg(value_enum)]
    pub shell: clap_complete::Shell,
}

/// Arguments for the `flux discover` command.
#[derive(clap::Args, Debug)]
pub struct DiscoverArgs {
    /// Discovery timeout in seconds
    #[arg(short, long, default_value = "5")]
    pub timeout: u64,
}

/// Arguments for the `flux send` command.
#[derive(clap::Args, Debug)]
pub struct SendArgs {
    /// File to send
    pub file: String,

    /// Target device (@devicename, host:port, or IP)
    pub target: String,

    /// Enable end-to-end encryption
    #[arg(long)]
    pub encrypt: bool,

    /// Device name to identify as
    #[arg(long)]
    pub name: Option<String>,
}

/// Arguments for the `flux receive` command.
#[derive(clap::Args, Debug)]
pub struct ReceiveArgs {
    /// Directory to save received files (default: current directory)
    #[arg(short, long, default_value = ".")]
    pub output: String,

    /// Port to listen on
    #[arg(short, long, default_value = "9741")]
    pub port: u16,

    /// Enable end-to-end encryption (require encrypted connections)
    #[arg(long)]
    pub encrypt: bool,

    /// Device name to advertise
    #[arg(long)]
    pub name: Option<String>,
}

/// Arguments for the `flux trust` command.
#[derive(clap::Args, Debug)]
pub struct TrustArgs {
    #[command(subcommand)]
    pub action: Option<TrustAction>,
}

/// Subcommands for trust management.
#[derive(Subcommand, Debug)]
pub enum TrustAction {
    /// List trusted devices
    List,
    /// Remove a trusted device
    Rm(TrustRmArgs),
}

/// Arguments for `flux trust rm`.
#[derive(clap::Args, Debug)]
pub struct TrustRmArgs {
    /// Device name to remove from trust store
    pub name: String,
}

/// Arguments for the `flux sync` command.
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

    /// Force sync even when source is empty (safety override for --delete)
    #[arg(long)]
    pub force: bool,
}
