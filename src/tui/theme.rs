//! Color constants and style helpers for consistent TUI theming.

use ratatui::style::{Color, Modifier, Style};

/// Style for the currently active tab label.
pub const TAB_ACTIVE: Style = Style::new()
    .fg(Color::Yellow)
    .add_modifier(Modifier::BOLD);

/// Style for inactive tab labels.
pub const TAB_INACTIVE: Style = Style::new().fg(Color::Gray);

/// Style for section headers and titles.
pub const HEADER: Style = Style::new()
    .fg(Color::Cyan)
    .add_modifier(Modifier::BOLD);

/// Style for selected/highlighted items.
pub const SELECTED: Style = Style::new()
    .bg(Color::DarkGray)
    .add_modifier(Modifier::BOLD);

/// Style for speed/throughput values.
pub const SPEED: Style = Style::new().fg(Color::Cyan);

/// Style for success indicators.
pub const SUCCESS: Style = Style::new().fg(Color::Green);

/// Style for error indicators.
pub const ERROR: Style = Style::new().fg(Color::Red);

/// Style for warning indicators.
pub const WARNING: Style = Style::new().fg(Color::Yellow);

/// Style for borders and dividers.
pub const BORDER: Style = Style::new().fg(Color::White);
