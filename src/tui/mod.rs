mod action;
mod app;
mod event;
mod terminal;
pub mod theme;
pub mod components;

use crate::error::FluxError;

/// Launch the interactive TUI mode.
///
/// Creates a tokio runtime and runs the async TUI event loop.
/// Terminal is initialized with alternate screen and raw mode,
/// and restored on exit (including on panic).
pub fn launch_tui() -> Result<(), FluxError> {
    let rt = tokio::runtime::Runtime::new().map_err(|e| FluxError::Io { source: e })?;
    rt.block_on(app::run_app()).map_err(|e| FluxError::Io { source: e })
}
