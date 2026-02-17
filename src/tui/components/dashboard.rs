//! Dashboard component showing real-time transfer monitoring.
//!
//! Displays an active transfers table with scrollable selection,
//! speed sparkline graph, and progress indicators.

use std::collections::VecDeque;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Sparkline, Table, TableState};

use super::Component;
use crate::config::paths::flux_data_dir;
use crate::queue::state::{QueueEntry, QueueStatus, QueueStore};
use crate::tui::action::Action;
use crate::tui::theme;

/// Ring buffer collecting speed samples for sparkline rendering.
pub struct SpeedHistory {
    samples: VecDeque<u64>,
    capacity: usize,
}

impl SpeedHistory {
    /// Create a new ring buffer with the given capacity.
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Push a bytes-per-second sample. Drops oldest if at capacity.
    pub fn push(&mut self, bytes_per_sec: u64) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(bytes_per_sec);
    }

    /// Return samples as a Vec for the Sparkline widget.
    pub fn as_slice(&self) -> Vec<u64> {
        self.samples.iter().copied().collect()
    }

    /// Return peak (max) sample value.
    pub fn peak(&self) -> u64 {
        self.samples.iter().copied().max().unwrap_or(0)
    }
}

/// Display-friendly snapshot of a single transfer.
pub struct TransferInfo {
    pub id: u64,
    pub source: String,
    pub dest: String,
    pub status: String,
    pub progress_pct: u16,
    pub speed_bps: u64,
    pub bytes_transferred: u64,
    pub total_bytes: u64,
}

/// Dashboard view component for the TUI.
///
/// Shows a table of active/recent transfers with a speed sparkline
/// graph below. Supports j/k scrolling through the transfer list.
pub struct DashboardComponent {
    transfers: Vec<TransferInfo>,
    table_state: TableState,
    speed_history: SpeedHistory,
    total_speed: u64,
}

impl DashboardComponent {
    /// Create a new dashboard with empty state.
    pub fn new() -> Self {
        Self {
            transfers: Vec::new(),
            table_state: TableState::default(),
            speed_history: SpeedHistory::new(60),
            total_speed: 0,
        }
    }

    /// Refresh transfer list from QueueStore entries.
    pub fn update_transfers(&mut self, entries: &[QueueEntry]) {
        self.transfers = entries
            .iter()
            .map(|e| {
                let status_str = format!("{}", e.status);
                // Progress and speed require real-time channels (deferred to integration).
                // For now: completed=100%, running=0% (live), others=0%.
                let progress_pct = match e.status {
                    QueueStatus::Completed => 100,
                    _ => 0,
                };
                TransferInfo {
                    id: e.id,
                    source: e.source.clone(),
                    dest: e.dest.clone(),
                    status: status_str,
                    progress_pct,
                    speed_bps: 0,
                    bytes_transferred: e.bytes_transferred,
                    total_bytes: 0,
                }
            })
            .collect();

        // Compute total speed from all running transfers
        self.total_speed = self
            .transfers
            .iter()
            .filter(|t| t.status == "running")
            .map(|t| t.speed_bps)
            .sum();

        self.speed_history.push(self.total_speed);

        // Ensure selection stays valid
        if !self.transfers.is_empty() {
            if self.table_state.selected().is_none() {
                self.table_state.select(Some(0));
            } else if let Some(sel) = self.table_state.selected() {
                if sel >= self.transfers.len() {
                    self.table_state.select(Some(self.transfers.len() - 1));
                }
            }
        } else {
            self.table_state.select(None);
        }
    }

    /// Populate mock data for development/testing.
    pub fn set_mock_data(&mut self) {
        self.transfers = vec![
            TransferInfo {
                id: 1,
                source: "/home/user/documents/report.pdf".into(),
                dest: "sftp://server/backup/report.pdf".into(),
                status: "running".into(),
                progress_pct: 67,
                speed_bps: 12_500_000,
                bytes_transferred: 67_000_000,
                total_bytes: 100_000_000,
            },
            TransferInfo {
                id: 2,
                source: "/tmp/archive.tar.gz".into(),
                dest: "\\\\nas\\share\\archive.tar.gz".into(),
                status: "pending".into(),
                progress_pct: 0,
                speed_bps: 0,
                bytes_transferred: 0,
                total_bytes: 500_000_000,
            },
            TransferInfo {
                id: 3,
                source: "/var/log/app.log".into(),
                dest: "/mnt/backup/app.log".into(),
                status: "completed".into(),
                progress_pct: 100,
                speed_bps: 0,
                bytes_transferred: 2_048_000,
                total_bytes: 2_048_000,
            },
        ];
        self.table_state.select(Some(0));

        // Push some mock speed samples for the sparkline
        let mock_speeds = [
            5_000_000u64, 8_000_000, 10_000_000, 12_000_000, 11_000_000, 13_000_000,
            12_500_000, 14_000_000, 9_000_000, 11_500_000, 12_000_000, 12_500_000,
        ];
        for &speed in &mock_speeds {
            self.speed_history.push(speed);
        }
        self.total_speed = 12_500_000;
    }

    /// Format bytes per second as human-readable speed string.
    fn format_speed(bps: u64) -> String {
        if bps == 0 {
            return "-".into();
        }
        let bs = bytesize::ByteSize(bps);
        format!("{}/s", bs)
    }

    /// Style a status string with appropriate color.
    fn status_style(status: &str) -> Style {
        match status {
            "running" => Style::default().fg(Color::Green),
            "paused" => Style::default().fg(Color::Yellow),
            "failed" => Style::default().fg(Color::Red),
            "completed" => Style::default().fg(Color::Green).add_modifier(Modifier::DIM),
            "pending" => Style::default().fg(Color::Gray),
            "cancelled" => Style::default().fg(Color::Gray).add_modifier(Modifier::DIM),
            _ => Style::default(),
        }
    }
}

impl Component for DashboardComponent {
    fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.transfers.is_empty() {
                    let current = self.table_state.selected().unwrap_or(0);
                    let prev = if current == 0 {
                        self.transfers.len() - 1
                    } else {
                        current - 1
                    };
                    self.table_state.select(Some(prev));
                }
                Action::ScrollUp
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.transfers.is_empty() {
                    let current = self.table_state.selected().unwrap_or(0);
                    let next = (current + 1) % self.transfers.len();
                    self.table_state.select(Some(next));
                }
                Action::ScrollDown
            }
            _ => Action::Noop,
        }
    }

    fn update(&mut self) {
        // Load QueueStore from data directory (best-effort)
        if let Ok(data_dir) = flux_data_dir() {
            if let Ok(store) = QueueStore::load(&data_dir) {
                self.update_transfers(store.list());
            }
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // Active transfers table
                Constraint::Length(7), // Speed sparkline
            ])
            .split(area);

        // -- Transfers Table --
        let header_cells = ["ID", "Source", "Dest", "Status", "Progress", "Speed"]
            .iter()
            .map(|h| Cell::from(*h).style(theme::HEADER));
        let header = Row::new(header_cells).height(1);

        let rows: Vec<Row> = self
            .transfers
            .iter()
            .map(|t| {
                let progress_str = format!("{}%", t.progress_pct);
                let speed_str = Self::format_speed(t.speed_bps);
                let status_style = Self::status_style(&t.status);

                Row::new(vec![
                    Cell::from(format!("{}", t.id)),
                    Cell::from(truncate_str(&t.source, 30)),
                    Cell::from(truncate_str(&t.dest, 30)),
                    Cell::from(Span::styled(t.status.clone(), status_style)),
                    Cell::from(progress_str),
                    Cell::from(speed_str),
                ])
            })
            .collect();

        let table = Table::new(
            rows,
            [
                Constraint::Length(5),
                Constraint::Percentage(30),
                Constraint::Percentage(30),
                Constraint::Length(10),
                Constraint::Length(10),
                Constraint::Length(12),
            ],
        )
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Active Transfers "),
        )
        .row_highlight_style(theme::SELECTED);

        // render_stateful_widget requires &mut table_state
        let mut table_state = self.table_state.clone();
        frame.render_stateful_widget(table, chunks[0], &mut table_state);

        // -- Speed Sparkline --
        let speed_data = self.speed_history.as_slice();
        let peak = self.speed_history.peak();
        let title = if peak > 0 {
            let bs = bytesize::ByteSize(peak);
            format!(" Transfer Speed (peak: {}/s) ", bs)
        } else {
            " Transfer Speed (no data) ".to_string()
        };

        if speed_data.is_empty() {
            let empty = Paragraph::new("No speed data yet")
                .style(Style::default().fg(Color::DarkGray))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title),
                );
            frame.render_widget(empty, chunks[1]);
        } else {
            let sparkline = Sparkline::default()
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(title),
                )
                .data(&speed_data)
                .style(theme::SPEED);
            frame.render_widget(sparkline, chunks[1]);
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
    fn speed_history_respects_capacity() {
        let mut hist = SpeedHistory::new(3);
        hist.push(100);
        hist.push(200);
        hist.push(300);
        hist.push(400);
        assert_eq!(hist.as_slice(), vec![200, 300, 400]);
    }

    #[test]
    fn speed_history_peak() {
        let mut hist = SpeedHistory::new(10);
        hist.push(50);
        hist.push(200);
        hist.push(100);
        assert_eq!(hist.peak(), 200);
    }

    #[test]
    fn speed_history_empty_peak_is_zero() {
        let hist = SpeedHistory::new(10);
        assert_eq!(hist.peak(), 0);
    }

    #[test]
    fn dashboard_new_is_empty() {
        let dash = DashboardComponent::new();
        assert!(dash.transfers.is_empty());
        assert_eq!(dash.total_speed, 0);
    }

    #[test]
    fn set_mock_data_populates_transfers() {
        let mut dash = DashboardComponent::new();
        dash.set_mock_data();
        assert_eq!(dash.transfers.len(), 3);
        assert_eq!(dash.table_state.selected(), Some(0));
        assert!(dash.speed_history.peak() > 0);
    }

    #[test]
    fn truncate_str_short_unchanged() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_long_gets_ellipsis() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn format_speed_zero_is_dash() {
        assert_eq!(DashboardComponent::format_speed(0), "-");
    }

    #[test]
    fn format_speed_nonzero() {
        let result = DashboardComponent::format_speed(1_000_000);
        assert!(result.contains("/s"));
    }

    #[test]
    fn key_j_scrolls_down() {
        let mut dash = DashboardComponent::new();
        dash.set_mock_data();
        dash.handle_key_event(test_key(KeyCode::Char('j')));
        assert_eq!(dash.table_state.selected(), Some(1));
    }

    #[test]
    fn key_k_scrolls_up_wraps() {
        let mut dash = DashboardComponent::new();
        dash.set_mock_data();
        // At position 0, k wraps to last
        dash.handle_key_event(test_key(KeyCode::Char('k')));
        assert_eq!(dash.table_state.selected(), Some(2));
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
