//! Component trait and TUI view modules.
//!
//! Each tab view implements the `Component` trait, providing
//! key event handling, state updates, and rendering.

pub mod status_bar;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::crossterm::event::KeyEvent;

use super::action::Action;

/// Trait for TUI view components.
///
/// Components handle keyboard input, update their internal state,
/// and render themselves into a given area of the terminal frame.
pub trait Component {
    /// Handle a key press event. Returns an `Action` to be processed
    /// by the App's main loop.
    ///
    /// Default: ignore the key event and return `Action::Noop`.
    fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        let _ = key;
        Action::Noop
    }

    /// Periodic state update, called on each tick event.
    fn update(&mut self) {}

    /// Render the component into the given area of the frame.
    fn render(&self, frame: &mut Frame, area: Rect);
}
