use std::time::Duration;

use crossterm::event::{EventStream, KeyEventKind};
use futures::StreamExt;
use ratatui::crossterm::event::{KeyEvent, MouseEvent};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Events produced by the TUI event handler.
#[derive(Debug, Clone)]
pub enum Event {
    /// Periodic tick for state updates (e.g., poll transfers).
    Tick,
    /// Render tick -- time to redraw the UI.
    Render,
    /// A key was pressed.
    Key(KeyEvent),
    /// A mouse event occurred.
    Mouse(MouseEvent),
    /// Terminal was resized.
    Resize(u16, u16),
    /// Quit signal (channel closed).
    Quit,
}

/// Async event handler that multiplexes terminal input, tick, and render events
/// using `tokio::select!`.
///
/// Spawns a background tokio task that reads from crossterm's `EventStream`
/// and two interval timers, sending all events through an mpsc channel.
pub struct EventHandler {
    _tx: mpsc::UnboundedSender<Event>,
    rx: mpsc::UnboundedReceiver<Event>,
    _task: JoinHandle<()>,
}

impl EventHandler {
    /// Create a new EventHandler with the given tick and render rates.
    ///
    /// - `tick_rate`: How often to send `Event::Tick` (e.g., 250ms for 4Hz).
    /// - `render_rate`: How often to send `Event::Render` (e.g., 50ms for 20fps).
    pub fn new(tick_rate: Duration, render_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let event_tx = tx.clone();

        let task = tokio::spawn(async move {
            let mut reader = EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_rate);
            let mut render_interval = tokio::time::interval(render_rate);

            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next();

                tokio::select! {
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    crossterm::event::Event::Key(key) if key.kind == KeyEventKind::Press => {
                                        if event_tx.send(Event::Key(key)).is_err() {
                                            return;
                                        }
                                    }
                                    crossterm::event::Event::Mouse(mouse) => {
                                        if event_tx.send(Event::Mouse(mouse)).is_err() {
                                            return;
                                        }
                                    }
                                    crossterm::event::Event::Resize(x, y) => {
                                        if event_tx.send(Event::Resize(x, y)).is_err() {
                                            return;
                                        }
                                    }
                                    _ => {}
                                }
                            }
                            Some(Err(_)) => {}
                            None => {
                                // Stream ended
                                return;
                            }
                        }
                    }
                    _ = tick_delay => {
                        if event_tx.send(Event::Tick).is_err() {
                            return;
                        }
                    }
                    _ = render_delay => {
                        if event_tx.send(Event::Render).is_err() {
                            return;
                        }
                    }
                }
            }
        });

        Self {
            _tx: tx,
            rx,
            _task: task,
        }
    }

    /// Wait for the next event from the event handler.
    ///
    /// Returns `Event::Quit` if the channel is closed.
    pub async fn next(&mut self) -> Event {
        self.rx.recv().await.unwrap_or(Event::Quit)
    }
}
