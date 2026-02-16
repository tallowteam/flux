use clap::Parser;
use tracing_subscriber::EnvFilter;

mod backend;
mod cli;
mod config;
mod error;

use cli::args::{Cli, Commands};
use config::types::Verbosity;
use error::FluxError;

fn main() {
    let cli = Cli::parse();

    // Convert CLI flags to verbosity level
    let verbosity = Verbosity::from((cli.quiet, cli.verbose));

    // Set up tracing with verbosity-based filter
    // RUST_LOG env var overrides CLI flags
    let filter = verbosity.as_tracing_filter();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new(filter)),
        )
        .with_writer(std::io::stderr) // Keep stdout clean for output
        .init();

    tracing::debug!("Verbosity level: {:?}", verbosity);

    if let Err(err) = run(cli) {
        display_error(&err);
        std::process::exit(1);
    }
}

/// Execute the dispatched command.
fn run(cli: Cli) -> Result<(), FluxError> {
    match cli.command {
        Commands::Cp(args) => {
            tracing::info!(
                source = %args.source.display(),
                dest = %args.dest.display(),
                recursive = args.recursive,
                "Copy command received"
            );
            eprintln!(
                "Copy: {} -> {} (recursive: {}, excludes: {}, includes: {})",
                args.source.display(),
                args.dest.display(),
                args.recursive,
                args.exclude.len(),
                args.include.len(),
            );
            // Actual copy implementation will be wired in Plan 03
            Ok(())
        }
    }
}

/// Display a FluxError with optional suggestion hint to stderr.
fn display_error(err: &FluxError) {
    eprintln!("error: {}", err);
    if let Some(suggestion) = err.suggestion() {
        eprintln!("  hint: {}", suggestion);
    }
}
