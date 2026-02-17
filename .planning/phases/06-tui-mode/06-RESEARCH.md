# Phase 6: TUI Mode - Research

**Researched:** 2026-02-16
**Domain:** Terminal User Interface (ratatui + crossterm + tokio)
**Confidence:** HIGH

## Summary

Phase 6 adds an interactive TUI mode to Flux, allowing users to launch a full-screen terminal dashboard with `flux --tui` or `flux ui`. The TUI provides real-time transfer monitoring with speed graphs, a file browser for local/remote navigation, queue management (pause/resume/cancel), and switchable views between active transfers and history.

The established Rust TUI stack is **ratatui v0.30** with **crossterm** (included by default) and **tokio** for async event handling. The user has specifically chosen ratatui. The project already depends on tokio (full features) and tokio-util, so async integration is straightforward. The existing codebase uses synchronous wrappers (`send_file_sync`, `start_receiver_sync`) around async functions -- the TUI will run its own tokio runtime or, more likely, become the async entry point itself.

**Primary recommendation:** Use ratatui 0.30 with the async crossterm event stream pattern (tokio::select! multiplexing tick/render/input events), the Component architecture for organizing views, and integrate with existing QueueStore/HistoryStore/FluxBackend types for data access.

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|-----------------|
| TUI-01 | User can launch interactive TUI mode (`flux --tui` or `flux ui`) | Add `Ui` subcommand to Commands enum and `--tui` global flag to Cli struct. Use ratatui::init() for terminal setup, crossterm for raw mode. Main event loop with tokio::select!. |
| TUI-02 | User can see real-time transfer dashboard with graphs | Use ratatui Sparkline widget for speed history, Gauge/LineGauge for progress, Chart for detailed view. Maintain a ring buffer of speed samples (1 sample/sec). Poll QueueStore for active transfers. |
| TUI-03 | User can browse and select files in TUI | Use ratatui-explorer crate for file browsing or build custom with List widget + FluxBackend::list_dir(). Keyboard navigation (j/k/Enter/Backspace). Support both local and remote backends. |
| TUI-04 | User can manage queue from TUI | Render QueueStore entries in a Table widget with StatefulWidget for selection. Key bindings: p=pause, r=resume, c=cancel, d=delete. Mutate QueueStore and save. |
| TUI-05 | User can switch between transfers view and history view | Use Tabs widget for view switching. Tab key or number keys to switch. Each tab is a Component with its own state and rendering. |
| CLI-02 | User can run sync commands (`flux sync src dest`) | NOTE: This is better addressed in Phase 7 (Sync Mode). Research confirms this is a sync-specific feature, not TUI-specific. Phase 6 should only add the TUI command entry points. |
</phase_requirements>

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| ratatui | 0.30.0 | TUI framework (widgets, layout, rendering) | User-specified choice. 18.4k GitHub stars, used by Netflix/AWS/OpenAI. Immediate-mode rendering, sub-ms performance. |
| crossterm | (bundled with ratatui 0.30) | Terminal backend (raw mode, events, alternate screen) | Default backend for ratatui. Cross-platform (Windows, Linux, macOS). Re-exported via `ratatui::crossterm`. |
| tokio | 1 (already in Cargo.toml) | Async runtime for event loop | Already a dependency. Required for crossterm EventStream and async select! multiplexing. |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| crossterm (event-stream feature) | via ratatui | Async terminal event stream | Required for non-blocking key/mouse input via `crossterm::event::EventStream` |
| futures | 0.3 (already in Cargo.toml) | Stream combinators for EventStream | Already a dependency. `.fuse()` and `StreamExt::next()` for event stream in tokio::select! |
| color-eyre | 0.6 | Error reporting with panic hooks | Provides formatted error display that works with TUI terminal restore. Widely used in ratatui ecosystem. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| ratatui-explorer (file browsing) | Custom List widget + FluxBackend::list_dir() | ratatui-explorer is convenient but doesn't support remote backends. Custom implementation needed anyway for SFTP/SMB/WebDAV browsing via FluxBackend trait. Recommend custom. |
| tui-tree-widget (directory trees) | Flat List with indentation | Tree widget adds a dependency but provides collapsible hierarchy. Flat list is simpler and sufficient for file browsing (like ranger/lf style). Recommend flat List. |
| color-eyre | Manual panic hooks | color-eyre handles both errors and panics elegantly. Manual hooks are more work for same result. Recommend color-eyre for dev, but can use manual hooks since project uses anyhow already. |

**Installation:**
```toml
# Add to Cargo.toml [dependencies]
ratatui = "0.30"
color-eyre = "0.6"
# crossterm event-stream feature is needed for async events:
# ratatui re-exports crossterm, but for event-stream we need explicit dep
crossterm = { version = "0.28", features = ["event-stream"] }

# Already present:
# tokio = { version = "1", features = ["full"] }
# futures = "0.3"
```

**Important version note:** Ratatui 0.30 bundles crossterm and re-exports it via `ratatui::crossterm`. To avoid version conflicts, prefer using the re-export. However, the `event-stream` feature must be enabled on the crossterm crate for `EventStream`. Check if ratatui 0.30's default feature set includes it; if not, add explicit crossterm dependency with `event-stream` feature.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── tui/
│   ├── mod.rs              # TUI module root, public API: launch_tui()
│   ├── app.rs              # App struct (root state + event loop)
│   ├── event.rs            # EventHandler (crossterm EventStream + tick/render intervals)
│   ├── terminal.rs         # Terminal setup/restore helpers
│   ├── action.rs           # Action enum (all possible TUI actions)
│   ├── components/
│   │   ├── mod.rs          # Component trait definition
│   │   ├── dashboard.rs    # Active transfers view (TUI-02)
│   │   ├── file_browser.rs # File browser view (TUI-03)
│   │   ├── queue_view.rs   # Queue management view (TUI-04)
│   │   ├── history_view.rs # Transfer history view (TUI-05)
│   │   └── status_bar.rs   # Bottom status bar (key hints)
│   └── theme.rs            # Colors and styles
├── cli/
│   └── args.rs             # Add `Ui` command + `--tui` flag
└── main.rs                 # Route `Ui` / `--tui` to tui::launch_tui()
```

### Pattern 1: Async Event Loop with tokio::select!
**What:** Multiplex terminal input, tick timer, and render timer in a single async loop
**When to use:** Always -- this is the standard ratatui+tokio pattern
**Example:**
```rust
// Source: https://ratatui.rs/tutorials/counter-async-app/async-event-stream/
pub enum Event {
    Tick,
    Render,
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Quit,
}

pub struct EventHandler {
    tx: mpsc::UnboundedSender<Event>,
    rx: mpsc::UnboundedReceiver<Event>,
    task: JoinHandle<()>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration, render_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        let _tx = tx.clone();
        let task = tokio::spawn(async move {
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval = tokio::time::interval(tick_rate);
            let mut render_interval = tokio::time::interval(render_rate);
            loop {
                let tick_delay = tick_interval.tick();
                let render_delay = render_interval.tick();
                let crossterm_event = reader.next().fuse();
                tokio::select! {
                    maybe_event = crossterm_event => {
                        match maybe_event {
                            Some(Ok(evt)) => {
                                match evt {
                                    crossterm::event::Event::Key(key) if key.kind == KeyEventKind::Press => {
                                        _tx.send(Event::Key(key)).unwrap();
                                    }
                                    crossterm::event::Event::Mouse(mouse) => {
                                        _tx.send(Event::Mouse(mouse)).unwrap();
                                    }
                                    crossterm::event::Event::Resize(x, y) => {
                                        _tx.send(Event::Resize(x, y)).unwrap();
                                    }
                                    _ => {}
                                }
                            }
                            _ => {}
                        }
                    }
                    _ = tick_delay => { _tx.send(Event::Tick).unwrap(); }
                    _ = render_delay => { _tx.send(Event::Render).unwrap(); }
                }
            }
        });
        Self { tx, rx, task }
    }

    pub async fn next(&mut self) -> Event {
        self.rx.recv().await.unwrap_or(Event::Quit)
    }
}
```

### Pattern 2: Component Architecture
**What:** Each view implements a Component trait with init/handle_events/update/render methods
**When to use:** Multi-view applications with tabs (exactly our case)
**Example:**
```rust
// Source: https://ratatui.rs/concepts/application-patterns/component-architecture/
pub trait Component {
    fn init(&mut self) -> Result<()> { Ok(()) }

    fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        Action::Noop
    }

    fn update(&mut self, action: Action) -> Action {
        Action::Noop
    }

    fn render(&mut self, frame: &mut Frame, area: Rect);
}
```

### Pattern 3: App State with Active Tab
**What:** Root App struct holds active tab enum and dispatches events to active component
**When to use:** Tab-based navigation (TUI-05)
**Example:**
```rust
pub enum ActiveTab {
    Dashboard,
    FileBrowser,
    Queue,
    History,
}

pub struct App {
    active_tab: ActiveTab,
    dashboard: DashboardComponent,
    file_browser: FileBrowserComponent,
    queue_view: QueueViewComponent,
    history_view: HistoryViewComponent,
    should_quit: bool,
}

impl App {
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        // Global keys first (tab switching, quit)
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('1') => { self.active_tab = ActiveTab::Dashboard; return Action::Noop; }
            KeyCode::Char('2') => { self.active_tab = ActiveTab::FileBrowser; return Action::Noop; }
            KeyCode::Char('3') => { self.active_tab = ActiveTab::Queue; return Action::Noop; }
            KeyCode::Char('4') => { self.active_tab = ActiveTab::History; return Action::Noop; }
            _ => {}
        }
        // Delegate to active component
        match self.active_tab {
            ActiveTab::Dashboard => self.dashboard.handle_key_event(key),
            ActiveTab::FileBrowser => self.file_browser.handle_key_event(key),
            ActiveTab::Queue => self.queue_view.handle_key_event(key),
            ActiveTab::History => self.history_view.handle_key_event(key),
        }
    }
}
```

### Pattern 4: Ring Buffer for Speed Graph Data
**What:** Fixed-size circular buffer storing speed samples for Sparkline/Chart rendering
**When to use:** TUI-02 (real-time speed graphs)
**Example:**
```rust
pub struct SpeedHistory {
    samples: VecDeque<u64>,  // bytes/sec samples
    capacity: usize,         // e.g., 60 for 60 seconds of history
}

impl SpeedHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, bytes_per_sec: u64) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(bytes_per_sec);
    }

    pub fn as_slice(&self) -> Vec<u64> {
        self.samples.iter().copied().collect()
    }
}

// Render with Sparkline:
let sparkline = Sparkline::default()
    .block(Block::bordered().title("Speed"))
    .data(&speed_history.as_slice())
    .style(Style::default().fg(Color::Cyan));
frame.render_widget(sparkline, area);
```

### Anti-Patterns to Avoid
- **Blocking the event loop:** Never call `std::thread::sleep()` or blocking I/O in the event handler or render loop. All I/O must be async or offloaded to a background task.
- **Rendering in update:** Keep rendering (view) separate from state mutation (update). The TEA/Component pattern enforces this.
- **Direct terminal writes:** Never use `print!`/`eprintln!` while TUI is active. All output must go through ratatui's Frame.
- **Forgetting terminal restore:** If the app panics or errors, the terminal must be restored. Use ratatui::init() (which sets up panic hooks automatically) or install manual panic hooks.
- **Shared mutable state without channels:** Don't share QueueStore across threads with Arc<Mutex>. Instead, use message passing (mpsc channels) to request state changes from a single owner.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Terminal raw mode setup/teardown | Custom crossterm enable/disable sequences | `ratatui::init()` / `ratatui::restore()` | Handles alternate screen, raw mode, mouse capture, AND panic hooks in one call |
| Event multiplexing | Custom polling loop with `crossterm::event::poll()` | `crossterm::event::EventStream` + `tokio::select!` | Non-blocking, composable with tick/render timers, no busy-waiting |
| Layout calculation | Manual coordinate math | `ratatui::layout::Layout` with constraints | Cassowary constraint solver handles resize, edge cases, overflow |
| Progress bars | Custom ASCII art progress rendering | `ratatui::widgets::Gauge` / `LineGauge` | Handles fractional progress, styling, labels, terminal width |
| Speed graphs | Custom character plotting | `ratatui::widgets::Sparkline` or `Chart` | Handles scaling, terminal width adaptation, multiple data series |
| Scrollable lists | Manual scroll offset tracking | `ratatui::widgets::List` + `ListState` | Handles selection, scrolling, overflow, keyboard navigation |
| Tab bar | Custom tab rendering | `ratatui::widgets::Tabs` | Handles highlighting, dividers, overflow |
| Table rendering | Manual column alignment | `ratatui::widgets::Table` + `TableState` | Handles column widths, selection, scrolling, borders |

**Key insight:** ratatui's widget system is specifically designed for these exact UI patterns. Every custom solution will be worse (handling terminal resize, Unicode width, color themes, accessibility).

## Common Pitfalls

### Pitfall 1: Terminal Left in Raw Mode After Panic
**What goes wrong:** Application panics, terminal stays in raw mode -- user sees garbled input, no echo, broken shell
**Why it happens:** Raw mode + alternate screen aren't restored before the panic unwinds
**How to avoid:** Use `ratatui::init()` which installs a panic hook automatically (since v0.28.1). Or install a manual panic hook that calls `ratatui::restore()` before the default handler.
**Warning signs:** Testing with `panic!()` in any code path -- if terminal breaks, hooks are missing

### Pitfall 2: Crossterm Version Mismatch
**What goes wrong:** Compilation errors or runtime crashes from incompatible crossterm types
**Why it happens:** ratatui 0.30 bundles a specific crossterm version. Adding a separate crossterm dep with a different version creates conflicts.
**How to avoid:** Use `ratatui::crossterm` re-export for all crossterm types. Only add explicit crossterm dependency for the `event-stream` feature, and match the version ratatui uses (0.28.x for ratatui 0.30). Check ratatui's Cargo.toml for exact version.
**Warning signs:** Duplicate type errors mentioning `crossterm::event::KeyEvent`

### Pitfall 3: Blocking the Render Loop
**What goes wrong:** TUI freezes, becomes unresponsive to keyboard input
**Why it happens:** A synchronous operation (file listing, network call, disk I/O) blocks the async event loop
**How to avoid:** Run all I/O in `tokio::spawn()` background tasks. Communicate results back via channels. The event loop should only do: receive event -> update state -> render.
**Warning signs:** TUI pauses when navigating into large directories or when transfers start

### Pitfall 4: Immediate Mode Rendering Confusion
**What goes wrong:** Widgets disappear, state resets, partial rendering
**Why it happens:** Ratatui uses immediate-mode rendering -- the entire UI must be redrawn every frame. If a widget is conditionally rendered, it vanishes when the condition changes.
**How to avoid:** Always render ALL visible widgets in every frame. Store all persistent state in the App/Component structs, not in widget instances. Widgets are ephemeral view objects.
**Warning signs:** Flickering, disappearing components on tab switch

### Pitfall 5: indicatif Conflicts with ratatui
**What goes wrong:** Progress bars and TUI rendering fight over terminal output
**Why it happens:** Both indicatif (existing) and ratatui try to control the terminal
**How to avoid:** When TUI mode is active, DO NOT use indicatif. Replace all progress reporting with ratatui widgets (Gauge, Sparkline). The existing `create_file_progress()` / `create_directory_progress()` functions in `src/progress/bar.rs` must not be called in TUI mode. Instead, pipe progress updates through a channel to the TUI's App state.
**Warning signs:** Garbled output, cursor jumping, mixed rendering

### Pitfall 6: QueueStore File Locking
**What goes wrong:** Multiple processes (CLI + TUI, or multiple TUI instances) corrupt queue.json
**Why it happens:** QueueStore uses read-modify-write without file locking
**How to avoid:** For Phase 6, document that only one Flux instance should modify the queue at a time. The TUI should reload QueueStore before displaying and after modifications. Consider advisory file locking in a future phase.
**Warning signs:** Missing queue entries, duplicate IDs, JSON parse errors

## Code Examples

Verified patterns from official sources:

### Terminal Setup and Main Loop (ratatui 0.30+)
```rust
// Source: https://ratatui.rs/tutorials/hello-ratatui/
// Source: https://ratatui.rs/recipes/apps/terminal-and-event-handler/

use std::time::Duration;
use ratatui::crossterm::event::{self, KeyCode, KeyEventKind};

#[tokio::main]
async fn main() -> color_eyre::Result<()> {
    color_eyre::install()?;
    let mut terminal = ratatui::init();

    let mut app = App::new()?;
    let mut events = EventHandler::new(
        Duration::from_millis(250),  // tick rate
        Duration::from_millis(16),   // ~60fps render rate
    );

    loop {
        let event = events.next().await;
        match event {
            Event::Render => {
                terminal.draw(|frame| app.render(frame, frame.area()))?;
            }
            Event::Tick => {
                app.on_tick();  // Update speed samples, poll transfers, etc.
            }
            Event::Key(key) => {
                let action = app.handle_key_event(key);
                if matches!(action, Action::Quit) {
                    break;
                }
            }
            Event::Resize(w, h) => {
                // ratatui handles resize automatically on next draw
            }
            _ => {}
        }
    }

    ratatui::restore();
    Ok(())
}
```

### Dashboard Layout (TUI-02)
```rust
// Source: https://ratatui.rs/concepts/layout/
// Adapted for Flux transfer dashboard

fn render_dashboard(frame: &mut Frame, area: Rect, state: &DashboardState) {
    // Top-level vertical split: transfers list | speed graph | details
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),     // Tab bar
            Constraint::Min(5),        // Active transfers table
            Constraint::Length(7),     // Speed sparkline
            Constraint::Length(3),     // Status bar
        ])
        .split(area);

    // Tab bar
    let tabs = Tabs::new(vec!["Dashboard", "Files", "Queue", "History"])
        .select(0)
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[0]);

    // Active transfers table
    let rows: Vec<Row> = state.active_transfers.iter().map(|t| {
        Row::new(vec![
            Cell::from(format!("#{}", t.id)),
            Cell::from(t.source.clone()),
            Cell::from(t.dest.clone()),
            Cell::from(format!("{}%", t.progress)),
            Cell::from(format!("{}/s", ByteSize(t.speed))),
        ])
    }).collect();

    let table = Table::new(rows, [
        Constraint::Length(5),
        Constraint::Percentage(30),
        Constraint::Percentage(30),
        Constraint::Length(8),
        Constraint::Length(12),
    ])
    .header(Row::new(["ID", "SOURCE", "DEST", "PROGRESS", "SPEED"]))
    .block(Block::bordered().title("Active Transfers"));

    frame.render_stateful_widget(table, chunks[1], &mut state.table_state);

    // Speed sparkline
    let sparkline = Sparkline::default()
        .block(Block::bordered().title("Transfer Speed"))
        .data(&state.speed_history.as_slice())
        .style(Style::default().fg(Color::Cyan));
    frame.render_widget(sparkline, chunks[2]);
}
```

### File Browser Component (TUI-03)
```rust
// Custom file browser using FluxBackend::list_dir()
// Source: https://ratatui.rs/concepts/widgets/ (List + ListState pattern)

pub struct FileBrowserState {
    pub current_dir: PathBuf,
    pub entries: Vec<FileEntry>,
    pub list_state: ListState,
    pub backend: Box<dyn FluxBackend>,
}

impl FileBrowserState {
    pub fn navigate_to(&mut self, path: &Path) -> Result<()> {
        self.entries = self.backend.list_dir(path)?;
        self.entries.sort_by(|a, b| {
            // Directories first, then alphabetical
            b.stat.is_dir.cmp(&a.stat.is_dir)
                .then(a.path.cmp(&b.path))
        });
        self.current_dir = path.to_path_buf();
        self.list_state.select(Some(0));
        Ok(())
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.entries.iter().map(|entry| {
            let name = entry.path.file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let display = if entry.stat.is_dir {
                format!("  {}/", name)
            } else {
                format!("  {} ({})", name, ByteSize(entry.stat.size))
            };
            ListItem::new(display)
        }).collect();

        let list = List::new(items)
            .block(Block::bordered().title(format!(" {} ", self.current_dir.display())))
            .highlight_style(Style::default().bg(Color::DarkGray).add_modifier(Modifier::BOLD))
            .highlight_symbol(">> ");

        frame.render_stateful_widget(list, area, &mut self.list_state);
    }
}
```

### Queue Management from TUI (TUI-04)
```rust
// Integrating with existing QueueStore
// Key bindings: p=pause, r=resume, c=cancel

impl QueueViewComponent {
    pub fn handle_key_event(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('p') => {
                if let Some(id) = self.selected_id() {
                    if let Err(e) = self.queue_store.pause(id) {
                        self.error_message = Some(format!("{}", e));
                    } else {
                        let _ = self.queue_store.save();
                    }
                }
                Action::Noop
            }
            KeyCode::Char('r') => {
                if let Some(id) = self.selected_id() {
                    if let Err(e) = self.queue_store.resume(id) {
                        self.error_message = Some(format!("{}", e));
                    } else {
                        let _ = self.queue_store.save();
                    }
                }
                Action::Noop
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.table_state.select_previous();
                Action::Noop
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.table_state.select_next();
                Action::Noop
            }
            _ => Action::Noop,
        }
    }
}
```

## Integration with Existing Codebase

### Key Integration Points

1. **CLI Entry Point (`src/cli/args.rs`):** Add `Ui` subcommand to `Commands` enum. Optionally add `--tui` global flag to `Cli` struct. Route to `tui::launch_tui()` from `main.rs`.

2. **QueueStore (`src/queue/state.rs`):** Already has the right API: `list()`, `get()`, `pause()`, `resume()`, `cancel()`, `save()`. The TUI component loads QueueStore from the data directory and mutates it directly. No changes needed to QueueStore itself.

3. **HistoryStore (`src/queue/history.rs`):** Already has `list()` returning `&[HistoryEntry]`. The TUI history view renders these in a Table widget. No changes needed.

4. **FluxBackend (`src/backend/mod.rs`):** The `list_dir()` method returns `Vec<FileEntry>` with `FileStat` (size, is_dir, is_file, modified). This is exactly what the file browser needs. The `create_backend()` factory creates backends from Protocol enum. File browser can use this to browse remote directories.

5. **Progress Reporting:** Currently tied to indicatif (`src/progress/bar.rs`). The TUI MUST NOT use indicatif. Instead, a new progress channel mechanism is needed: transfers report bytes via `mpsc::Sender<ProgressUpdate>`, and the TUI App receives these on tick events to update speed graphs and gauges.

6. **Transfer Execution:** Currently `execute_copy()` in `src/transfer/mod.rs` is synchronous and prints to stderr. For TUI-initiated transfers, a separate async wrapper is needed that: (a) accepts a progress channel sender, (b) runs the copy in a background tokio task, (c) reports progress without indicatif. This is the biggest integration challenge.

### Existing Types to Reuse
- `QueueEntry`, `QueueStatus` -- render in queue/transfers table
- `HistoryEntry` -- render in history table
- `FileEntry`, `FileStat` -- render in file browser
- `FluxBackend` trait + `create_backend()` -- browse remote filesystems
- `Protocol`, `detect_protocol()` -- parse user-entered paths in file browser
- `CpArgs` -- construct from TUI file selections to initiate transfers

### Changes Required to Existing Code
- `src/cli/args.rs`: Add `Ui` command variant
- `src/main.rs`: Add match arm for `Commands::Ui`, call `tui::launch_tui()`
- `src/progress/bar.rs`: No changes, but TUI mode must bypass these functions
- `src/transfer/mod.rs`: Needs a way to report progress via channel instead of indicatif (may require a trait or callback parameter in a future refactor, or a parallel code path)

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `Terminal::new()` + manual panic hooks | `ratatui::init()` with auto panic hooks | ratatui v0.28.1 (2024) | Much simpler setup, fewer bugs |
| `ratatui::init()` + manual `restore()` | `ratatui::run(closure)` for simple apps | ratatui v0.30.0 (2025) | Even simpler for basic apps, but async apps still need init/restore |
| Separate `crossterm` crate import | `ratatui::crossterm` re-export | ratatui v0.27.0 (2024) | Avoids version mismatch bugs |
| `crossterm::event::poll()` (sync) | `crossterm::event::EventStream` (async) | crossterm 0.27+ | Non-blocking, composable with tokio::select! |
| tui-rs (abandoned) | ratatui (active fork) | 2023 | Active maintenance, new features, community |
| Monolithic ratatui crate | Modular workspace (ratatui-core, ratatui-widgets) | ratatui v0.30.0 (2025) | Better compile times, API stability |

**Deprecated/outdated:**
- `tui-rs`: Original crate, abandoned since 2023. ratatui is the successor.
- Manual `crossterm::terminal::enable_raw_mode()` calls: Use `ratatui::init()` instead.
- `crossterm::event::poll()` in async apps: Use `EventStream` + `tokio::select!`.

## Open Questions

1. **Progress channel for TUI-initiated transfers**
   - What we know: `execute_copy()` uses indicatif directly. TUI needs a different progress mechanism.
   - What's unclear: Should we add a callback/channel parameter to `execute_copy()`, or create a parallel `execute_copy_with_channel()` function, or refactor `execute_copy()` to accept a trait for progress reporting?
   - Recommendation: Create a `ProgressReporter` trait with two implementations: `IndicatifReporter` (existing behavior) and `ChannelReporter` (TUI mode). Pass as parameter. This is a refactor of existing code but keeps it clean. Alternative: just create a parallel function to avoid touching working code.

2. **Async vs sync transfer execution in TUI**
   - What we know: Current transfer code is synchronous (blocking I/O with rayon for parallelism). TUI event loop is async (tokio).
   - What's unclear: Should TUI spawn transfers in `tokio::task::spawn_blocking()` or refactor transfers to be async?
   - Recommendation: Use `tokio::task::spawn_blocking()` to run existing synchronous transfer code in a background thread. This avoids a massive refactor of the transfer engine and is the standard pattern for mixing sync+async in tokio. The background task communicates progress via channel.

3. **Remote file browsing performance**
   - What we know: `FluxBackend::list_dir()` is synchronous and may be slow for network backends (SFTP, WebDAV).
   - What's unclear: How to keep the TUI responsive during slow remote listings?
   - Recommendation: Run `list_dir()` in `spawn_blocking()`. Show a loading indicator while waiting. Cache directory listings for recently visited paths.

4. **CLI-02 (sync commands) scope**
   - What we know: CLI-02 is listed in Phase 6 requirements but describes `flux sync src dest`.
   - What's unclear: Why it's in Phase 6 instead of Phase 7 (Sync Mode).
   - Recommendation: Phase 6 should only add the TUI entry points (`flux ui` / `flux --tui`). CLI-02 sync functionality should be deferred to Phase 7 where it belongs. Note this in the plan as intentionally deferred.

## Sources

### Primary (HIGH confidence)
- [ratatui.rs](https://ratatui.rs/) - Official docs: installation, widgets, layout, application patterns
- [ratatui.rs/concepts/application-patterns/component-architecture/](https://ratatui.rs/concepts/application-patterns/component-architecture/) - Component trait pattern
- [ratatui.rs/concepts/application-patterns/the-elm-architecture/](https://ratatui.rs/concepts/application-patterns/the-elm-architecture/) - TEA pattern with full code
- [ratatui.rs/tutorials/counter-async-app/async-event-stream/](https://ratatui.rs/tutorials/counter-async-app/async-event-stream/) - Async EventHandler with tokio::select!
- [ratatui.rs/recipes/apps/terminal-and-event-handler/](https://ratatui.rs/recipes/apps/terminal-and-event-handler/) - Tui struct with event handler
- [ratatui.rs/recipes/apps/panic-hooks/](https://ratatui.rs/recipes/apps/panic-hooks/) - Panic hook setup
- [ratatui.rs/concepts/layout/](https://ratatui.rs/concepts/layout/) - Layout constraints, nesting, Flex
- [ratatui.rs/installation/](https://ratatui.rs/installation/) - Version 0.30.0, crossterm bundled
- [docs.rs/ratatui/latest/ratatui/widgets/](https://docs.rs/ratatui/latest/ratatui/widgets/index.html) - Widget catalog: Chart, Sparkline, Gauge, Table, List, Tabs
- [docs.rs/ratatui-explorer](https://docs.rs/ratatui-explorer/latest/ratatui_explorer/struct.FileExplorer.html) - FileExplorer API

### Secondary (MEDIUM confidence)
- [github.com/ratatui/ratatui](https://github.com/ratatui/ratatui) - 18.4k stars, v0.30.0
- [github.com/EdJoPaTo/tui-rs-tree-widget](https://github.com/EdJoPaTo/tui-rs-tree-widget) - Tree widget v0.24.0
- [ratatui.rs/examples/widgets/chart/](https://ratatui.rs/examples/widgets/chart/) - Chart widget example
- [ratatui.rs/examples/widgets/sparkline/](https://ratatui.rs/examples/widgets/sparkline/) - Sparkline example

### Tertiary (LOW confidence)
- [github.com/ratatui/async-template](https://github.com/ratatui-org/ratatui-async-template) - Opinionated async template (version may vary)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - ratatui is user-specified, version verified via official docs, crossterm bundled, tokio already in project
- Architecture: HIGH - Component pattern and async EventHandler are official ratatui patterns with full documentation and code examples
- Pitfalls: HIGH - Terminal restore, version mismatch, blocking render loop are all documented in official ratatui docs
- Integration: MEDIUM - Integration with existing QueueStore/HistoryStore is straightforward. Progress reporting refactor needs design during planning.

**Research date:** 2026-02-16
**Valid until:** 2026-03-16 (ratatui is actively developed but 0.30 is stable)
