/// Actions that can be dispatched between TUI components.
///
/// Each component's `handle_key_event` returns an Action,
/// and the App's main loop processes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// No operation -- event was handled but no further action needed.
    Noop,
    /// Quit the TUI application.
    Quit,
    /// Switch to the tab at the given index (0-based).
    SwitchTab(usize),
    /// Scroll up in the active view.
    ScrollUp,
    /// Scroll down in the active view.
    ScrollDown,
    /// Select the highlighted item.
    Select,
    /// Go back / navigate up.
    Back,
    /// Pause the selected transfer.
    Pause,
    /// Resume the selected transfer.
    Resume,
    /// Cancel the selected transfer.
    Cancel,
    /// Refresh data in the active view.
    Refresh,
}
