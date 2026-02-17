//! Bottom status bar showing key binding hints.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use super::Component;
use super::super::action::Action;
use ratatui::crossterm::event::KeyEvent;

/// Status bar widget displayed at the bottom of the TUI.
///
/// Shows available key bindings as a horizontal bar.
pub struct StatusBar {
    /// Key binding hints as (key, description) pairs.
    pub hints: Vec<(String, String)>,
}

impl StatusBar {
    /// Create a new StatusBar with default key hints.
    pub fn new() -> Self {
        Self {
            hints: vec![
                ("q".into(), "Quit".into()),
                ("1".into(), "Dashboard".into()),
                ("2".into(), "Files".into()),
                ("3".into(), "Queue".into()),
                ("4".into(), "History".into()),
                ("Tab".into(), "Next".into()),
            ],
        }
    }
}

impl Component for StatusBar {
    fn handle_key_event(&mut self, _key: KeyEvent) -> Action {
        // Status bar doesn't handle key events
        Action::Noop
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        let key_style = Style::default()
            .fg(Color::Black)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD);
        let desc_style = Style::default().fg(Color::Gray);
        let sep_style = Style::default().fg(Color::DarkGray);

        let mut spans = Vec::new();
        for (i, (key, desc)) in self.hints.iter().enumerate() {
            if i > 0 {
                spans.push(Span::styled(" | ", sep_style));
            }
            spans.push(Span::styled(format!(" {} ", key), key_style));
            spans.push(Span::styled(format!(" {}", desc), desc_style));
        }

        let line = Line::from(spans);
        let paragraph = Paragraph::new(line);
        frame.render_widget(paragraph, area);
    }
}
