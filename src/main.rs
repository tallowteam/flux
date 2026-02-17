use clap::Parser;
use tracing_subscriber::EnvFilter;

mod backend;
mod cli;
mod config;
mod discovery;
mod error;
mod net;
mod progress;
mod protocol;
mod queue;
mod security;
mod transfer;

use cli::args::{Cli, Commands, CpArgs, QueueAction};
use config::types::Verbosity;
use error::FluxError;
use queue::state::QueueStatus;
use bytesize::ByteSize;

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
        Commands::Queue(args) => {
            let data_dir = config::paths::flux_data_dir()?;
            let mut store = queue::state::QueueStore::load(&data_dir)?;

            match args.action.unwrap_or(QueueAction::List) {
                QueueAction::Add(add_args) => {
                    let id = store.add(
                        add_args.source,
                        add_args.dest,
                        add_args.recursive,
                        add_args.verify,
                        add_args.compress,
                    );
                    store.save()?;
                    eprintln!("Queued transfer #{}", id);
                }
                QueueAction::List => {
                    let entries = store.list();
                    if entries.is_empty() {
                        eprintln!("Queue is empty");
                    } else {
                        println!(
                            "{:<4} {:<10} {:<30} {:<30}",
                            "ID", "STATUS", "SOURCE", "DEST"
                        );
                        println!("{}", "-".repeat(76));
                        for entry in entries {
                            let source = truncate_str(&entry.source, 28);
                            let dest = truncate_str(&entry.dest, 28);
                            println!(
                                "{:<4} {:<10} {:<30} {:<30}",
                                entry.id, entry.status, source, dest
                            );
                        }
                    }
                }
                QueueAction::Pause(id_args) => {
                    store.pause(id_args.id)?;
                    store.save()?;
                    eprintln!("Paused transfer #{}", id_args.id);
                }
                QueueAction::Resume(id_args) => {
                    store.resume(id_args.id)?;
                    store.save()?;
                    eprintln!("Resumed transfer #{}", id_args.id);
                }
                QueueAction::Cancel(id_args) => {
                    store.cancel(id_args.id)?;
                    store.save()?;
                    eprintln!("Cancelled transfer #{}", id_args.id);
                }
                QueueAction::Run => {
                    let pending: Vec<u64> =
                        store.pending_entries().iter().map(|e| e.id).collect();
                    if pending.is_empty() {
                        eprintln!("No pending transfers in queue");
                        return Ok(());
                    }
                    eprintln!("Processing {} transfer(s)...", pending.len());

                    for id in pending {
                        // Mark as running
                        if let Some(entry) = store.get_mut(id) {
                            entry.status = QueueStatus::Running;
                            entry.started_at = Some(chrono::Utc::now());
                        }
                        store.save()?;

                        // Clone entry details for CpArgs construction
                        let entry = store.get(id).unwrap().clone();

                        eprintln!("\n[#{}] {} -> {}", id, entry.source, entry.dest);

                        // Build CpArgs from queue entry
                        let cp_args = CpArgs {
                            source: entry.source.clone(),
                            dest: entry.dest.clone(),
                            recursive: entry.recursive,
                            verify: entry.verify,
                            compress: entry.compress,
                            chunks: 0,
                            exclude: vec![],
                            include: vec![],
                            limit: None,
                            resume: false,
                            on_conflict: None,
                            on_error: None,
                            dry_run: false,
                        };

                        match transfer::execute_copy(cp_args, cli.quiet) {
                            Ok(()) => {
                                if let Some(e) = store.get_mut(id) {
                                    e.status = QueueStatus::Completed;
                                    e.completed_at = Some(chrono::Utc::now());
                                }
                                store.save()?;
                                eprintln!("[#{}] Completed", id);
                            }
                            Err(err) => {
                                if let Some(e) = store.get_mut(id) {
                                    e.status = QueueStatus::Failed;
                                    e.completed_at = Some(chrono::Utc::now());
                                    e.error = Some(format!("{}", err));
                                }
                                store.save()?;
                                eprintln!("[#{}] Failed: {}", id, err);
                            }
                        }
                    }
                    eprintln!("\nQueue processing complete");
                }
                QueueAction::Clear => {
                    store.clear_completed();
                    store.save()?;
                    eprintln!("Cleared completed/failed/cancelled entries");
                }
            }
            Ok(())
        }
        Commands::History(args) => {
            let data_dir = config::paths::flux_data_dir()?;
            let flux_config = config::types::load_config().unwrap_or_default();
            let mut store =
                queue::history::HistoryStore::load(&data_dir, flux_config.history_limit)?;

            if args.clear {
                store.clear();
                store.save()?;
                eprintln!("History cleared");
                return Ok(());
            }

            let entries = store.list();
            if entries.is_empty() {
                eprintln!("No transfer history");
                return Ok(());
            }

            // Show most recent N entries
            let start = if entries.len() > args.count {
                entries.len() - args.count
            } else {
                0
            };
            println!(
                "{:<20} {:<10} {:<30} {:<30} {:<10}",
                "TIMESTAMP", "STATUS", "SOURCE", "DEST", "SIZE"
            );
            println!("{}", "-".repeat(102));
            for entry in &entries[start..] {
                let ts = entry.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
                let size = format_bytes(entry.bytes);
                let source = truncate_str(&entry.source, 28);
                let dest = truncate_str(&entry.dest, 28);
                println!(
                    "{:<20} {:<10} {:<30} {:<30} {:<10}",
                    ts, entry.status, source, dest, size
                );
            }
            Ok(())
        }
        Commands::Completions(args) => {
            use clap::CommandFactory;
            use clap_complete::generate;
            let mut cmd = Cli::command();
            generate(args.shell, &mut cmd, "flux", &mut std::io::stdout());
            Ok(())
        }
    }
}

/// Truncate a string to `max` chars, appending "..." if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max.saturating_sub(3)])
    }
}

/// Format bytes as human-readable string using bytesize crate.
fn format_bytes(bytes: u64) -> String {
    ByteSize(bytes).to_string()
}

/// Display a FluxError with optional suggestion hint to stderr.
fn display_error(err: &FluxError) {
    eprintln!("error: {}", err);
    if let Some(suggestion) = err.suggestion() {
        eprintln!("  hint: {}", suggestion);
    }
}
