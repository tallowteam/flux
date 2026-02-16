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
}
