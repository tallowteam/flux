use clap::Parser;
use tracing_subscriber::EnvFilter;

mod backend;
mod cli;
mod config;
mod error;
mod progress;
mod protocol;
mod transfer;

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
            tracing::debug!(
                source = %args.source,
                dest = %args.dest,
                recursive = args.recursive,
                chunks = args.chunks,
                verify = args.verify,
                compress = args.compress,
                limit = ?args.limit,
                resume = args.resume,
                "Copy command received"
            );
            transfer::execute_copy(args, cli.quiet)?;
            Ok(())
        }
        Commands::Add(args) => {
            let config_dir = config::paths::flux_config_dir()?;
            config::aliases::validate_alias_name(&args.name)?;
            let mut store = config::aliases::AliasStore::load(&config_dir)?;
            store.add(args.name.clone(), args.path.clone());
            store.save()?;
            eprintln!("Alias saved: {} -> {}", args.name, args.path);
            Ok(())
        }
        Commands::Alias(args) => {
            let config_dir = config::paths::flux_config_dir()?;
            let mut store = config::aliases::AliasStore::load(&config_dir)?;

            match args.action {
                None => {
                    // List all aliases
                    let aliases = store.list();
                    if aliases.is_empty() {
                        println!("(no aliases saved)");
                    } else {
                        for (name, path) in aliases {
                            println!("{} -> {}", name, path);
                        }
                    }
                }
                Some(cli::args::AliasAction::Rm(rm_args)) => {
                    if store.remove(&rm_args.name) {
                        store.save()?;
                        eprintln!("Alias removed: {}", rm_args.name);
                    } else {
                        eprintln!("Alias not found: {}", rm_args.name);
                    }
                }
            }
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
