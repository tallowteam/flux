//! Queue management component for viewing and managing transfer jobs.
//!
//! Displays queue entries in a scrollable table with pause/resume/cancel
//! key bindings and status feedback.

use std::path::PathBuf;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState};

use super::Component;
use crate::config::paths::flux_data_dir;
use crate::queue::state::{QueueEntry, QueueStatus, QueueStore};
use crate::tui::action::Action;
use crate::tui::theme;

/// Queue management view component for the TUI.
///
/// Displays queued transfers in a table and supports pause/resume/cancel
/// operations via keyboard shortcuts.
pub struct QueueViewComponent {
    entries: Vec<QueueEntry>,
    table_state: TableState,
    data_dir: Option<PathBuf>,
    status_message: Option<String>,
    message_ttl: u8,
}

impl QueueViewComponent {
    /// Create a new queue view, loading initial data from disk.
    pub fn new() -> Self {
        let data_dir = flux_data_dir().ok();
        let mut component = Self {
            entries: Vec::new(),
            table_state: TableState::default(),
            data_dir,
            status_message: None,
            message_ttl: 0,
        };
        component.reload();
        component
    }

    /// Create a queue view with an explicit data directory (for testing).
    #[cfg(test)]
    pub fn with_data_dir(data_dir: std::path::PathBuf) -> Self {
        let mut component = Self {
            entries: Vec::new(),
            table_state: TableState::default(),
            data_dir: Some(data_dir),
            status_message: None,
            message_ttl: 0,
        };
        component.reload();
        component
    }

    /// Reload queue entries from disk (best-effort).
    fn reload(&mut self) {
        if let Some(ref dir) = self.data_dir {
            if let Ok(store) = QueueStore::load(dir) {
                self.entries = store.list().to_vec();
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

    /// Get the ID of the currently selected entry.
    fn selected_id(&self) -> Option<u64> {
        self.table_state
            .selected()
            .and_then(|i| self.entries.get(i))
            .map(|e| e.id)
    }

    /// Perform an action on the selected queue entry.
    fn perform_action<F>(&mut self, action_fn: F, verb: &str)
    where
        F: FnOnce(&mut QueueStore, u64) -> Result<(), crate::error::FluxError>,
    {
        let id = match self.selected_id() {
            Some(id) => id,
            None => {
                self.status_message = Some("No entry selected".into());
                self.message_ttl = 12;
                return;
            }
        };

        if let Some(ref dir) = self.data_dir {
            match QueueStore::load(dir) {
                Ok(mut store) => match action_fn(&mut store, id) {
                    Ok(()) => {
                        if let Err(e) = store.save() {
                            self.status_message = Some(format!("Save error: {}", e));
                            self.message_ttl = 20;
                        } else {
                            self.status_message = Some(format!("{} #{}", verb, id));
                            self.message_ttl = 12;
                        }
                    }
                    Err(e) => {
                        self.status_message = Some(format!("Error: {}", e));
                        self.message_ttl = 20;
                    }
                },
                Err(e) => {
                    self.status_message = Some(format!("Load error: {}", e));
                    self.message_ttl = 20;
                }
            }
        }

        self.reload();
    }

    /// Clear all completed/failed/cancelled entries.
    fn clear_completed(&mut self) {
        if let Some(ref dir) = self.data_dir {
            if let Ok(mut store) = QueueStore::load(dir) {
                store.clear_completed();
                if let Err(e) = store.save() {
                    self.status_message = Some(format!("Save error: {}", e));
                    self.message_ttl = 20;
                } else {
                    self.status_message = Some("Cleared completed entries".into());
                    self.message_ttl = 12;
                }
            }
        }
        self.reload();
    }

    /// Style a queue status with appropriate color.
    fn status_style(status: &QueueStatus) -> Style {
        match status {
            QueueStatus::Pending => Style::default(),
            QueueStatus::Running => Style::default().fg(Color::Green),
            QueueStatus::Paused => Style::default().fg(Color::Yellow),
            QueueStatus::Completed => Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::DIM),
            QueueStatus::Failed => Style::default().fg(Color::Red),
            QueueStatus::Cancelled => Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::DIM),
        }
    }
}

impl Component for QueueViewComponent {
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
            KeyCode::Char('p') => {
                self.perform_action(|store, id| store.pause(id), "Paused");
                Action::Noop
            }
            KeyCode::Char('r') => {
                self.perform_action(|store, id| store.resume(id), "Resumed");
                Action::Noop
            }
            KeyCode::Char('c') => {
                self.perform_action(|store, id| store.cancel(id), "Cancelled");
                Action::Noop
            }
            KeyCode::Char('x') => {
                self.clear_completed();
                Action::Noop
            }
            _ => Action::Noop,
        }
    }

    fn update(&mut self) {
        self.reload();

        // Decrement message TTL
        if self.message_ttl > 0 {
            self.message_ttl -= 1;
            if self.message_ttl == 0 {
                self.status_message = None;
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Layout: table fills most space, optional status line at bottom
        let has_message = self.status_message.is_some();
        let chunks = if has_message {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3), Constraint::Length(1)])
                .split(area)
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(3)])
                .split(area)
        };

        if self.entries.is_empty() {
            let empty = Paragraph::new("Queue is empty")
                .style(
                    Style::default()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::DIM),
                )
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(" Queue (0 entries) "),
                );
            frame.render_widget(empty, chunks[0]);
        } else {
            let header_cells = ["ID", "Status", "Source", "Dest", "Added"]
                .iter()
                .map(|h| Cell::from(*h).style(theme::HEADER));
            let header = Row::new(header_cells).height(1);

            let rows: Vec<Row> = self
                .entries
                .iter()
                .map(|e| {
                    let status_str = format!("{}", e.status);
                    let style = Self::status_style(&e.status);
                    let added = e.added_at.format("%Y-%m-%d %H:%M").to_string();

                    Row::new(vec![
                        Cell::from(format!("{}", e.id)),
                        Cell::from(Span::styled(status_str, style)),
                        Cell::from(truncate_str(&e.source, 35)),
                        Cell::from(truncate_str(&e.dest, 35)),
                        Cell::from(added),
                    ])
                })
                .collect();

            let title = format!(" Queue ({} entries) ", self.entries.len());
            let table = Table::new(
                rows,
                [
                    Constraint::Length(6),
                    Constraint::Length(12),
                    Constraint::Percentage(35),
                    Constraint::Percentage(35),
                    Constraint::Length(16),
                ],
            )
            .header(header)
            .block(Block::default().borders(Borders::ALL).title(title))
            .row_highlight_style(theme::SELECTED);

            let mut table_state = self.table_state.clone();
            frame.render_stateful_widget(table, chunks[0], &mut table_state);
        }

        // Status message line
        if has_message {
            if let Some(ref msg) = self.status_message {
                let style = if msg.starts_with("Error") || msg.starts_with("Load error") || msg.starts_with("Save error") {
                    Style::default().fg(Color::Red)
                } else {
                    Style::default().fg(Color::Green)
                };
                let para = Paragraph::new(msg.as_str()).style(style);
                frame.render_widget(para, chunks[1]);
            }
        }
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

    #[test]
    fn queue_view_new_creates_component() {
        let dir = tempfile::tempdir().unwrap();
        let view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());
        assert!(view.entries.is_empty());
        assert!(view.status_message.is_none());
    }

    #[test]
    fn queue_view_reload_with_entries() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = QueueStore::load(dir.path()).unwrap();
        store.add("src1".into(), "dst1".into(), false, false, false);
        store.add("src2".into(), "dst2".into(), false, false, false);
        store.save().unwrap();

        let view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());
        assert_eq!(view.entries.len(), 2);
        assert_eq!(view.table_state.selected(), Some(0));
    }

    #[test]
    fn queue_view_j_k_navigation() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = QueueStore::load(dir.path()).unwrap();
        store.add("a".into(), "b".into(), false, false, false);
        store.add("c".into(), "d".into(), false, false, false);
        store.save().unwrap();

        let mut view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());
        assert_eq!(view.table_state.selected(), Some(0));

        view.handle_key_event(test_key(KeyCode::Char('j')));
        assert_eq!(view.table_state.selected(), Some(1));

        view.handle_key_event(test_key(KeyCode::Char('k')));
        assert_eq!(view.table_state.selected(), Some(0));
    }

    #[test]
    fn queue_view_pause_resume_cancel() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = QueueStore::load(dir.path()).unwrap();
        store.add("a".into(), "b".into(), false, false, false);
        store.save().unwrap();

        let mut view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());

        // Pause
        view.handle_key_event(test_key(KeyCode::Char('p')));
        assert!(view.status_message.as_ref().unwrap().contains("Paused"));

        // Reload to see state change
        view.reload();
        assert_eq!(
            format!("{}", view.entries[0].status),
            "paused"
        );

        // Resume
        view.handle_key_event(test_key(KeyCode::Char('r')));
        assert!(view.status_message.as_ref().unwrap().contains("Resumed"));

        view.reload();
        assert_eq!(
            format!("{}", view.entries[0].status),
            "pending"
        );

        // Cancel
        view.handle_key_event(test_key(KeyCode::Char('c')));
        assert!(view.status_message.as_ref().unwrap().contains("Cancelled"));

        view.reload();
        assert_eq!(
            format!("{}", view.entries[0].status),
            "cancelled"
        );
    }

    #[test]
    fn queue_view_clear_completed() {
        let dir = tempfile::tempdir().unwrap();

        let mut store = QueueStore::load(dir.path()).unwrap();
        store.add("a".into(), "b".into(), false, false, false); // 1: pending
        store.add("c".into(), "d".into(), false, false, false); // 2: will complete
        store.get_mut(2).unwrap().status = QueueStatus::Completed;
        store.save().unwrap();

        let mut view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());
        assert_eq!(view.entries.len(), 2);

        view.handle_key_event(test_key(KeyCode::Char('x')));
        assert!(view.status_message.as_ref().unwrap().contains("Cleared"));
        assert_eq!(view.entries.len(), 1);
    }

    #[test]
    fn queue_view_message_ttl_decrements() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = QueueViewComponent::with_data_dir(dir.path().to_path_buf());
        view.status_message = Some("Test message".into());
        view.message_ttl = 2;

        view.update();
        assert_eq!(view.message_ttl, 1);
        assert!(view.status_message.is_some());

        view.update();
        assert_eq!(view.message_ttl, 0);
        assert!(view.status_message.is_none());
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
