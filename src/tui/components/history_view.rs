//! History view component displaying past transfer records.
//!
//! Shows a scrollable table of transfer history entries with
//! timestamp, status, source, destination, size, and duration.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use super::Component;
use crate::config::paths::flux_data_dir;
use crate::queue::history::{HistoryEntry, HistoryStore};
use crate::tui::action::Action;
use crate::tui::theme;

/// History view component for the TUI.
///
/// Displays recent transfer history entries in a scrollable table,
/// showing most recent transfers first.
pub struct HistoryViewComponent {
    entries: Vec<HistoryEntry>,
    table_state: TableState,
    data_dir: Option<PathBuf>,
}

impl HistoryViewComponent {
    /// Create a new history view, loading initial data from disk.
    pub fn new() -> Self {
        let data_dir = flux_data_dir().ok();
        let mut component = Self {
            entries: Vec::new(),
            table_state: TableState::default(),
            data_dir,
        };
        component.reload();
        component
    }

    /// Create a history view with an explicit data directory (for testing).
    #[cfg(test)]
    pub fn with_data_dir(data_dir: std::path::PathBuf) -> Self {
        let mut component = Self {
            entries: Vec::new(),
            table_state: TableState::default(),
            data_dir: Some(data_dir),
        };
        component.reload();
        component
    }

    /// Reload history entries from disk (best-effort).
    ///
    /// Entries are reversed so most recent appears first.
    fn reload(&mut self) {
        if let Some(ref dir) = self.data_dir {
            if let Ok(store) = HistoryStore::load(dir, 1000) {
                let mut entries = store.list().to_vec();
                entries.reverse(); // Most recent first
                self.entries = entries;
            }
        }

        // Keep selection valid
        if !self.entries.is_empty() {
            if self.table_state.selected().is_none() {
                self.table_state.select(Some(0));
            } else if let Some(sel) = self.table_state.selected() {
                if sel >= self.entries.len() {
                    self.table_state.select(Some(self.entries.len() - 1));
                }
            }
        } else {
            self.table_state.select(None);
        }
    }

    /// Format a duration in seconds as a human-readable string.
    fn format_duration(secs: f64) -> String {
        if secs < 1.0 {
            format!("{:.0}ms", secs * 1000.0)
        } else if secs < 60.0 {
            format!("{:.1}s", secs)
        } else {
            let mins = (secs / 60.0).floor() as u64;
            let remaining = secs - (mins as f64 * 60.0);
            format!("{}m {:.0}s", mins, remaining)
        }
    }

    /// Style a status string with appropriate color.
    fn status_style(status: &str) -> Style {
        match status {
            "completed" => Style::default().fg(Color::Green),
            "failed" => Style::default().fg(Color::Red),
            "cancelled" => Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::DIM),
            _ => Style::default(),
        }
    }
}

impl Component for HistoryViewComponent {
    fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.entries.is_empty() {
                    let current = self.table_state.selected().unwrap_or(0);
                    let prev = if current == 0 {
                        self.entries.len() - 1
                    } else {
                        current - 1
                    };
                    self.table_state.select(Some(prev));
                }
                Action::ScrollUp
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entries.is_empty() {
                    let current = self.table_state.selected().unwrap_or(0);
                    let next = (current + 1) % self.entries.len();
                    self.table_state.select(Some(next));
                }
                Action::ScrollDown
            }
            _ => Action::Noop,
        }
    }

    fn update(&mut self) {
        self.reload();
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        if self.entries.is_empty() {
            let empty = Paragraph::new("No transfer history")
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Transfer History (0 entries) "),
                );
            frame.render_widget(empty, area);
            return;
        }

        let header_cells = ["Timestamp", "Status", "Source", "Dest", "Size", "Duration"]
            .iter()
            .map(|h| Cell::from(*h).style(theme::HEADER));
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = self
            .entries
            .iter()
            .map(|e| {
                let ts = e.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
                let style = Self::status_style(&e.status);
                let size_str = format!("{}", bytesize::ByteSize(e.bytes));
                let dur_str = Self::format_duration(e.duration_secs);

                Row::new(vec![
                    Cell::from(ts),
                    Cell::from(Span::styled(e.status.clone(), style)),
                    Cell::from(truncate_str(&e.source, 25)),
                    Cell::from(truncate_str(&e.dest, 25)),
                    Cell::from(size_str),
                    Cell::from(dur_str),
                ])
            })
            .collect();

        let title = format!(" Transfer History ({} entries) ", self.entries.len());
        let table = Table::new(
            rows,
            [
                Constraint::Length(20),
                Constraint::Length(10),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Length(10),
                Constraint::Length(10),
            ],
        )
        .header(header)
        .block(Block::default().borders(Borders::ALL).title(title))
        .row_highlight_style(theme::SELECTED);

        let mut table_state = self.table_state.clone();
        frame.render_stateful_widget(table, area, &mut table_state);
    }
}

/// Truncate a string to the given max length, appending "..." if truncated.
fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else if max <= 3 {
        s[..max].to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    #[test]
    fn history_view_new_creates_component() {
        let dir = tempfile::tempdir().unwrap();
        let view = HistoryViewComponent::with_data_dir(dir.path().to_path_buf());
        assert!(view.entries.is_empty());
    }

    #[test]
    fn history_view_reload_reverses_entries() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = HistoryStore::load(dir.path(), 1000).unwrap();
        store
            .append(HistoryEntry {
                source: "first".into(),
                dest: "dst1".into(),
                bytes: 100,
                files: 1,
                duration_secs: 0.5,
                timestamp: Utc::now(),
                status: "completed".into(),
                error: None,
            })
            .unwrap();
        store
            .append(HistoryEntry {
                source: "second".into(),
                dest: "dst2".into(),
                bytes: 200,
                files: 1,
                duration_secs: 1.0,
                timestamp: Utc::now(),
                status: "completed".into(),
                error: None,
            })
            .unwrap();

        let view = HistoryViewComponent::with_data_dir(dir.path().to_path_buf());
        assert_eq!(view.entries.len(), 2);
        // Most recent (second) should be first after reversal
        assert_eq!(view.entries[0].source, "second");
        assert_eq!(view.entries[1].source, "first");
    }

    #[test]
    fn history_view_j_k_navigation() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = HistoryStore::load(dir.path(), 1000).unwrap();
        for i in 0..3 {
            store
                .append(HistoryEntry {
                    source: format!("src_{}", i),
                    dest: format!("dst_{}", i),
                    bytes: 100,
                    files: 1,
                    duration_secs: 0.1,
                    timestamp: Utc::now(),
                    status: "completed".into(),
                    error: None,
                })
                .unwrap();
        }

        let mut view = HistoryViewComponent::with_data_dir(dir.path().to_path_buf());
        assert_eq!(view.table_state.selected(), Some(0));

        view.handle_key_event(test_key(KeyCode::Char('j')));
        assert_eq!(view.table_state.selected(), Some(1));

        view.handle_key_event(test_key(KeyCode::Char('k')));
        assert_eq!(view.table_state.selected(), Some(0));
    }

    #[test]
    fn format_duration_milliseconds() {
        assert_eq!(HistoryViewComponent::format_duration(0.5), "500ms");
        assert_eq!(HistoryViewComponent::format_duration(0.05), "50ms");
    }

    #[test]
    fn format_duration_seconds() {
        assert_eq!(HistoryViewComponent::format_duration(5.0), "5.0s");
        assert_eq!(HistoryViewComponent::format_duration(30.5), "30.5s");
    }

    #[test]
    fn format_duration_minutes() {
        assert_eq!(HistoryViewComponent::format_duration(90.0), "1m 30s");
        assert_eq!(HistoryViewComponent::format_duration(125.0), "2m 5s");
    }

    fn test_key(code: KeyCode) -> KeyEvent {
        use ratatui::crossterm::event::{KeyEventKind, KeyEventState, KeyModifiers};
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }
}
