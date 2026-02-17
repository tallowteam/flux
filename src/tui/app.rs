use std::time::Duration;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Tabs};

use super::action::Action;
use super::components::Component;
use super::components::dashboard::DashboardComponent;
use super::components::file_browser::FileBrowserComponent;
use super::components::history_view::HistoryViewComponent;
use super::components::queue_view::QueueViewComponent;
use super::components::status_bar::StatusBar;
use super::event::{Event, EventHandler};
use super::terminal;

/// The available tabs in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveTab {
    Dashboard,
    FileBrowser,
    Queue,
    History,
}

impl ActiveTab {
    /// All tabs in order.
    const ALL: [ActiveTab; 4] = [
        ActiveTab::Dashboard,
        ActiveTab::FileBrowser,
        ActiveTab::Queue,
        ActiveTab::History,
    ];

    /// Tab display name.
    fn name(self) -> &'static str {
        match self {
            ActiveTab::Dashboard => "Dashboard",
            ActiveTab::FileBrowser => "Files",
            ActiveTab::Queue => "Queue",
            ActiveTab::History => "History",
        }
    }

    /// Tab index (0-based).
    fn index(self) -> usize {
        match self {
            ActiveTab::Dashboard => 0,
            ActiveTab::FileBrowser => 1,
            ActiveTab::Queue => 2,
            ActiveTab::History => 3,
        }
    }

    /// Get tab from index.
    fn from_index(index: usize) -> Option<ActiveTab> {
        ActiveTab::ALL.get(index).copied()
    }

    /// Next tab (wrapping).
    fn next(self) -> ActiveTab {
        let next_index = (self.index() + 1) % ActiveTab::ALL.len();
        ActiveTab::ALL[next_index]
    }

    /// Previous tab (wrapping).
    fn prev(self) -> ActiveTab {
        let prev_index = if self.index() == 0 {
            ActiveTab::ALL.len() - 1
        } else {
            self.index() - 1
        };
        ActiveTab::ALL[prev_index]
    }
}

/// Root application state for the TUI.
pub struct App {
    /// Currently active tab.
    active_tab: ActiveTab,
    /// Whether the app should quit on the next loop iteration.
    should_quit: bool,
    /// Bottom status bar showing key hints.
    status_bar: StatusBar,
    /// Dashboard tab component.
    dashboard: DashboardComponent,
    /// File browser tab component.
    file_browser: FileBrowserComponent,
    /// Queue management tab component.
    queue_view: QueueViewComponent,
    /// Transfer history tab component.
    history_view: HistoryViewComponent,
}

impl App {
    /// Create a new App with default state.
    pub fn new() -> Self {
        let mut dashboard = DashboardComponent::new();
        dashboard.set_mock_data();

        Self {
            active_tab: ActiveTab::Dashboard,
            should_quit: false,
            status_bar: StatusBar::new(),
            dashboard,
            file_browser: FileBrowserComponent::new(),
            queue_view: QueueViewComponent::new(),
            history_view: HistoryViewComponent::new(),
        }
    }

    /// Handle a key event at the app level.
    ///
    /// Global keys (quit, tab switching) are handled here first.
    /// If not consumed, the event is delegated to the active tab component.
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('q') => Action::Quit,
            KeyCode::Char('1') => {
                self.active_tab = ActiveTab::Dashboard;
                Action::Noop
            }
            KeyCode::Char('2') => {
                self.active_tab = ActiveTab::FileBrowser;
                Action::Noop
            }
            KeyCode::Char('3') => {
                self.active_tab = ActiveTab::Queue;
                Action::Noop
            }
            KeyCode::Char('4') => {
                self.active_tab = ActiveTab::History;
                Action::Noop
            }
            KeyCode::Tab => {
                self.active_tab = self.active_tab.next();
                Action::Noop
            }
            KeyCode::BackTab => {
                self.active_tab = self.active_tab.prev();
                Action::Noop
            }
            KeyCode::Char('?') => {
                // Help toggle placeholder -- future plans will add a help overlay
                Action::Noop
            }
            _ => {
                // Delegate to active tab component
                match self.active_tab {
                    ActiveTab::Dashboard => self.dashboard.handle_key_event(key),
                    ActiveTab::FileBrowser => self.file_browser.handle_key_event(key),
                    ActiveTab::Queue => self.queue_view.handle_key_event(key),
                    ActiveTab::History => self.history_view.handle_key_event(key),
                }
            }
        }
    }

    /// Called on each tick event for periodic state updates.
    pub fn on_tick(&mut self) {
        self.dashboard.update();
        self.queue_view.update();
        self.history_view.update();
    }

    /// Render the entire application UI.
    pub fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Layout: tab bar (3 rows), content area (fills), status bar (1 row)
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        // -- Tab bar --
        let tab_titles: Vec<Line> = ActiveTab::ALL
            .iter()
            .map(|t| Line::from(t.name()))
            .collect();

        let tabs = Tabs::new(tab_titles)
            .select(self.active_tab.index())
            .style(Style::default().fg(Color::Gray))
            .highlight_style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .divider("|")
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(" Flux TUI "),
            );
        frame.render_widget(tabs, chunks[0]);

        // -- Active tab content --
        match self.active_tab {
            ActiveTab::Dashboard => {
                self.dashboard.render(frame, chunks[1]);
            }
            ActiveTab::FileBrowser => {
                self.file_browser.render(frame, chunks[1]);
            }
            ActiveTab::Queue => {
                self.queue_view.render(frame, chunks[1]);
            }
            ActiveTab::History => {
                self.history_view.render(frame, chunks[1]);
            }
        }

        // -- Status bar with tab-appropriate hints --
        let mut status_bar = StatusBar::new();
        status_bar.hints = match self.active_tab {
            ActiveTab::Dashboard => vec![
                ("j/k".into(), "Navigate".into()),
                ("1-4".into(), "Tabs".into()),
                ("q".into(), "Quit".into()),
            ],
            ActiveTab::FileBrowser => vec![
                ("j/k".into(), "Navigate".into()),
                ("Enter".into(), "Open".into()),
                ("Bksp".into(), "Parent".into()),
                ("q".into(), "Quit".into()),
            ],
            ActiveTab::Queue => vec![
                ("j/k".into(), "Navigate".into()),
                ("p".into(), "Pause".into()),
                ("r".into(), "Resume".into()),
                ("c".into(), "Cancel".into()),
                ("x".into(), "Clear".into()),
                ("q".into(), "Quit".into()),
            ],
            ActiveTab::History => vec![
                ("j/k".into(), "Navigate".into()),
                ("1-4".into(), "Tabs".into()),
                ("q".into(), "Quit".into()),
            ],
        };
        status_bar.render(frame, chunks[2]);
    }
}

/// Run the main TUI event loop.
///
/// Initializes the terminal, creates the event handler and app,
/// then loops: receiving events, updating state, and rendering.
/// Terminal is restored on exit.
pub async fn run_app() -> Result<(), std::io::Error> {
    let mut terminal = terminal::init();

    let mut events = EventHandler::new(
        Duration::from_millis(250), // tick rate: 4Hz
        Duration::from_millis(50),  // render rate: 20fps
    );

    let mut app = App::new();

    loop {
        let event = events.next().await;
        match event {
            Event::Render => {
                terminal.draw(|frame| app.render(frame))?;
            }
            Event::Tick => {
                app.on_tick();
            }
            Event::Key(key) => {
                let action = app.handle_key_event(key);
                if action == Action::Quit {
                    break;
                }
            }
            Event::Resize(_, _) => {
                // ratatui handles resize automatically on next draw
            }
            Event::Mouse(_) | Event::Quit => {}
        }
    }

    terminal::restore();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key_event(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::empty(),
            kind: KeyEventKind::Press,
            state: KeyEventState::empty(),
        }
    }

    #[test]
    fn test_app_new_defaults_to_dashboard() {
        let app = App::new();
        assert_eq!(app.active_tab, ActiveTab::Dashboard);
        assert!(!app.should_quit);
    }

    #[test]
    fn test_quit_action() {
        let mut app = App::new();
        let action = app.handle_key_event(key_event(KeyCode::Char('q')));
        assert_eq!(action, Action::Quit);
    }

    #[test]
    fn test_switch_tabs_with_number_keys() {
        let mut app = App::new();

        app.handle_key_event(key_event(KeyCode::Char('2')));
        assert_eq!(app.active_tab, ActiveTab::FileBrowser);

        app.handle_key_event(key_event(KeyCode::Char('3')));
        assert_eq!(app.active_tab, ActiveTab::Queue);

        app.handle_key_event(key_event(KeyCode::Char('4')));
        assert_eq!(app.active_tab, ActiveTab::History);

        app.handle_key_event(key_event(KeyCode::Char('1')));
        assert_eq!(app.active_tab, ActiveTab::Dashboard);
    }

    #[test]
    fn test_tab_cycles_forward() {
        let mut app = App::new();
        assert_eq!(app.active_tab, ActiveTab::Dashboard);

        app.handle_key_event(key_event(KeyCode::Tab));
        assert_eq!(app.active_tab, ActiveTab::FileBrowser);

        app.handle_key_event(key_event(KeyCode::Tab));
        assert_eq!(app.active_tab, ActiveTab::Queue);

        app.handle_key_event(key_event(KeyCode::Tab));
        assert_eq!(app.active_tab, ActiveTab::History);

        // Wraps around
        app.handle_key_event(key_event(KeyCode::Tab));
        assert_eq!(app.active_tab, ActiveTab::Dashboard);
    }

    #[test]
    fn test_backtab_cycles_backward() {
        let mut app = App::new();
        assert_eq!(app.active_tab, ActiveTab::Dashboard);

        // Wraps to History
        app.handle_key_event(key_event(KeyCode::BackTab));
        assert_eq!(app.active_tab, ActiveTab::History);

        app.handle_key_event(key_event(KeyCode::BackTab));
        assert_eq!(app.active_tab, ActiveTab::Queue);

        app.handle_key_event(key_event(KeyCode::BackTab));
        assert_eq!(app.active_tab, ActiveTab::FileBrowser);

        app.handle_key_event(key_event(KeyCode::BackTab));
        assert_eq!(app.active_tab, ActiveTab::Dashboard);
    }

    #[test]
    fn test_active_tab_names() {
        assert_eq!(ActiveTab::Dashboard.name(), "Dashboard");
        assert_eq!(ActiveTab::FileBrowser.name(), "Files");
        assert_eq!(ActiveTab::Queue.name(), "Queue");
        assert_eq!(ActiveTab::History.name(), "History");
    }

    #[test]
    fn test_active_tab_indices() {
        assert_eq!(ActiveTab::Dashboard.index(), 0);
        assert_eq!(ActiveTab::FileBrowser.index(), 1);
        assert_eq!(ActiveTab::Queue.index(), 2);
        assert_eq!(ActiveTab::History.index(), 3);
    }

    #[test]
    fn test_active_tab_from_index() {
        assert_eq!(ActiveTab::from_index(0), Some(ActiveTab::Dashboard));
        assert_eq!(ActiveTab::from_index(1), Some(ActiveTab::FileBrowser));
        assert_eq!(ActiveTab::from_index(2), Some(ActiveTab::Queue));
        assert_eq!(ActiveTab::from_index(3), Some(ActiveTab::History));
        assert_eq!(ActiveTab::from_index(4), None);
    }

    #[test]
    fn test_unhandled_key_returns_noop() {
        let mut app = App::new();
        let action = app.handle_key_event(key_event(KeyCode::Char('x')));
        assert_eq!(action, Action::Noop);
    }
}
