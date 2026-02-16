use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
