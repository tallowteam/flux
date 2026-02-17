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
