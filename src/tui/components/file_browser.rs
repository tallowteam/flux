//! File browser component for navigating local (and future remote) file systems.
//!
//! Displays directory contents in a scrollable list with keyboard navigation.
//! Directories are sorted before files, both alphabetically.

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use ratatui::Frame;
use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

use super::Component;
use crate::backend::local::LocalBackend;
use crate::backend::FluxBackend;
use crate::tui::action::Action;
use crate::tui::theme;

/// A display-friendly file/directory entry.
pub struct BrowserEntry {
    pub name: String,
    pub full_path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
}

/// File browser component for the TUI Files tab.
///
/// Lists directory contents using the local backend, supporting
/// keyboard navigation to browse the file system.
pub struct FileBrowserComponent {
    current_dir: PathBuf,
    entries: Vec<BrowserEntry>,
    list_state: ListState,
    error_message: Option<String>,
}

impl FileBrowserComponent {
    /// Create a new file browser starting at the current working directory.
    pub fn new() -> Self {
        let start_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut browser = Self {
            current_dir: start_dir.clone(),
            entries: Vec::new(),
            list_state: ListState::default(),
            error_message: None,
        };
        browser.navigate_to(&start_dir);
        browser
    }

    /// Navigate to the given directory, refreshing the entry list.
    ///
    /// On error, sets error_message and keeps current entries.
    pub fn navigate_to(&mut self, path: &Path) {
        let backend = LocalBackend::new();
        match backend.list_dir(path) {
            Ok(file_entries) => {
                self.error_message = None;
                let mut entries: Vec<BrowserEntry> = file_entries
                    .into_iter()
                    .map(|fe| {
                        let name = fe
                            .path
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_else(|| fe.path.to_string_lossy().to_string());
                        BrowserEntry {
                            name,
                            full_path: fe.path,
                            is_dir: fe.stat.is_dir,
                            size: fe.stat.size,
                            modified: fe.stat.modified,
                        }
                    })
                    .collect();

                // Sort: directories first, then alphabetically (case-insensitive)
                entries.sort_by(|a, b| {
                    b.is_dir
                        .cmp(&a.is_dir)
                        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                });

                // Prepend ".." entry if not at root
                let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
                if canonical.parent().is_some() {
                    let parent_path = canonical
                        .parent()
                        .map(|p| p.to_path_buf())
                        .unwrap_or_else(|| canonical.clone());
                    entries.insert(
                        0,
                        BrowserEntry {
                            name: "..".into(),
                            full_path: parent_path,
                            is_dir: true,
                            size: 0,
                            modified: None,
                        },
                    );
                }

                self.current_dir = canonical;
                self.entries = entries;
                if !self.entries.is_empty() {
                    self.list_state.select(Some(0));
                } else {
                    self.list_state.select(None);
                }
            }
            Err(e) => {
                self.error_message = Some(format!("Error: {}", e));
            }
        }
    }

    /// Enter the currently selected directory (or file).
    pub fn enter_selected(&mut self) {
        if let Some(entry) = self.selected_entry_cloned() {
            if entry.is_dir {
                self.navigate_to(&entry.full_path);
            }
            // Files: no-op for now (future: trigger transfer)
        }
    }

    /// Navigate to the parent directory.
    pub fn go_parent(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(|p| p.to_path_buf()) {
            self.navigate_to(&parent);
        }
    }

    /// Return a clone of the currently selected entry.
    fn selected_entry_cloned(&self) -> Option<BrowserEntry> {
        self.list_state
            .selected()
            .and_then(|i| self.entries.get(i))
            .map(|e| BrowserEntry {
                name: e.name.clone(),
                full_path: e.full_path.clone(),
                is_dir: e.is_dir,
                size: e.size,
                modified: e.modified,
            })
    }

    /// Return a reference to the currently selected entry.
    pub fn selected_entry(&self) -> Option<&BrowserEntry> {
        self.list_state
            .selected()
            .and_then(|i| self.entries.get(i))
    }
}

impl Component for FileBrowserComponent {
    fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if !self.entries.is_empty() {
                    let current = self.list_state.selected().unwrap_or(0);
                    let prev = if current == 0 {
                        self.entries.len() - 1
                    } else {
                        current - 1
                    };
                    self.list_state.select(Some(prev));
                }
                Action::ScrollUp
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.entries.is_empty() {
                    let current = self.list_state.selected().unwrap_or(0);
                    let next = (current + 1) % self.entries.len();
                    self.list_state.select(Some(next));
                }
                Action::ScrollDown
            }
            KeyCode::Enter | KeyCode::Char('l') => {
                self.enter_selected();
                Action::Select
            }
            KeyCode::Backspace | KeyCode::Char('h') => {
                self.go_parent();
                Action::Back
            }
            KeyCode::Home => {
                if !self.entries.is_empty() {
                    self.list_state.select(Some(0));
                }
                Action::Noop
            }
            KeyCode::End => {
                if !self.entries.is_empty() {
                    self.list_state.select(Some(self.entries.len() - 1));
                }
                Action::Noop
            }
            _ => Action::Noop,
        }
    }

    fn render(&self, frame: &mut Frame, area: Rect) {
        // Show error message at top if present
        if let Some(ref err) = self.error_message {
            let err_para = Paragraph::new(err.as_str())
                .style(Style::default().fg(Color::Red))
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title(format!(" {} ", self.current_dir.display())),
                );
            frame.render_widget(err_para, area);
            return;
        }

        // Build list items from entries
        let items: Vec<ListItem> = self
            .entries
            .iter()
            .map(|entry| {
                if entry.name == ".." {
                    ListItem::new(Line::from(vec![Span::styled(
                        "  ../",
                        Style::default()
                            .fg(Color::DarkGray)
                            .add_modifier(Modifier::DIM),
                    )]))
                } else if entry.is_dir {
                    ListItem::new(Line::from(vec![Span::styled(
                        format!("  {}/", entry.name),
                        Style::default()
                            .fg(Color::Blue)
                            .add_modifier(Modifier::BOLD),
                    )]))
                } else {
                    let size_str = format!("{}", bytesize::ByteSize(entry.size));
                    ListItem::new(Line::from(vec![
                        Span::styled(
                            format!("  {}", entry.name),
                            Style::default().fg(Color::White),
                        ),
                        Span::styled(
                            format!("  ({})", size_str),
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]))
                }
            })
            .collect();

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!(" {} ", self.current_dir.display())),
            )
            .highlight_style(theme::SELECTED)
            .highlight_symbol(">> ");

        let mut list_state = self.list_state.clone();
        frame.render_stateful_widget(list, area, &mut list_state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_browser_new_has_entries() {
        let browser = FileBrowserComponent::new();
        // Current directory should have at least one entry (.. or files)
        assert!(!browser.entries.is_empty());
        assert!(browser.list_state.selected().is_some());
    }

    #[test]
    fn file_browser_navigate_to_project_root() {
        let mut browser = FileBrowserComponent::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        browser.navigate_to(&manifest_dir);

        // Should contain Cargo.toml and src/
        let names: Vec<&str> = browser.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Cargo.toml"));
        assert!(names.contains(&"src"));
    }

    #[test]
    fn file_browser_directories_sorted_before_files() {
        let mut browser = FileBrowserComponent::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        browser.navigate_to(&manifest_dir);

        // Skip ".." entry, find first file and first dir
        let entries_without_parent: Vec<&BrowserEntry> =
            browser.entries.iter().filter(|e| e.name != "..").collect();

        if entries_without_parent.len() >= 2 {
            let first_file_idx = entries_without_parent
                .iter()
                .position(|e| !e.is_dir);
            let last_dir_idx = entries_without_parent
                .iter()
                .rposition(|e| e.is_dir);

            if let (Some(file_idx), Some(dir_idx)) = (first_file_idx, last_dir_idx) {
                assert!(
                    dir_idx < file_idx,
                    "Directories should come before files"
                );
            }
        }
    }

    #[test]
    fn file_browser_parent_entry_exists() {
        let mut browser = FileBrowserComponent::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let src_dir = manifest_dir.join("src");
        browser.navigate_to(&src_dir);

        // First entry should be ".."
        assert_eq!(browser.entries[0].name, "..");
        assert!(browser.entries[0].is_dir);
    }

    #[test]
    fn file_browser_j_k_navigation() {
        let mut browser = FileBrowserComponent::new();
        assert_eq!(browser.list_state.selected(), Some(0));

        // j moves down
        browser.handle_key_event(test_key(KeyCode::Char('j')));
        assert_eq!(browser.list_state.selected(), Some(1));

        // k moves back up
        browser.handle_key_event(test_key(KeyCode::Char('k')));
        assert_eq!(browser.list_state.selected(), Some(0));
    }

    #[test]
    fn file_browser_enter_directory() {
        let mut browser = FileBrowserComponent::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        browser.navigate_to(&manifest_dir);

        // Find "src" entry and select it
        let src_idx = browser
            .entries
            .iter()
            .position(|e| e.name == "src")
            .expect("src directory should exist");
        browser.list_state.select(Some(src_idx));

        let old_dir = browser.current_dir.clone();
        browser.enter_selected();

        // Should now be in src/
        assert_ne!(browser.current_dir, old_dir);
        assert!(
            browser.current_dir.ends_with("src"),
            "Should be in src dir, got: {}",
            browser.current_dir.display()
        );
    }

    #[test]
    fn file_browser_go_parent() {
        let mut browser = FileBrowserComponent::new();
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let src_dir = manifest_dir.join("src");
        browser.navigate_to(&src_dir);

        let old_dir = browser.current_dir.clone();
        browser.go_parent();

        assert_ne!(browser.current_dir, old_dir);
    }

    #[test]
    fn file_browser_error_on_bad_path() {
        let mut browser = FileBrowserComponent::new();
        browser.navigate_to(Path::new("/nonexistent/path/that/does/not/exist"));
        assert!(browser.error_message.is_some());
    }

    #[test]
    fn file_browser_home_end_keys() {
        let mut browser = FileBrowserComponent::new();
        if browser.entries.len() >= 3 {
            browser.handle_key_event(test_key(KeyCode::End));
            assert_eq!(
                browser.list_state.selected(),
                Some(browser.entries.len() - 1)
            );

            browser.handle_key_event(test_key(KeyCode::Home));
            assert_eq!(browser.list_state.selected(), Some(0));
        }
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
