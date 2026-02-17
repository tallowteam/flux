use ratatui::DefaultTerminal;

/// Initialize the terminal for TUI rendering.
///
/// Uses `ratatui::init()` which:
/// - Enables alternate screen
/// - Enables raw mode
/// - Installs panic hooks to restore terminal on panic
///
/// Returns a `DefaultTerminal` ready for drawing.
pub fn init() -> DefaultTerminal {
    ratatui::init()
}

/// Restore the terminal to its original state.
///
/// Uses `ratatui::restore()` which:
/// - Disables raw mode
/// - Leaves alternate screen
/// - Shows cursor
pub fn restore() {
    ratatui::restore();
}
