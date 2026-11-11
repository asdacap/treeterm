//! One terminal tab: an emulator attached to the tab's daemon PTY.
//!
//! Port of the non-view half of `src/renderer/components/BaseTerminal.tsx` (with its
//! `CachedTerminal`): the entity outlives its view, so switching tabs or workspaces keeps
//! the scrollback and the attachment. Output is parsed on a background thread; the UI
//! thread hears about it at most once per batch.
//!
//! Dropping the model detaches from the PTY; it never kills it. Killing is [`Self::kill`].

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use futures::channel::mpsc;
use futures::future::{Either, select};
use futures::{FutureExt, StreamExt};
use gpui_kit::{BackgroundExecutor, Context, EventEmitter, Task};
use treeterm_core::daemon::{ApiError, DaemonConnection};
use treeterm_core::pty::{AttachKind, PtyEvent, PtyInputHandle};
use treeterm_core::rt::Sub;
use treeterm_core::settings::{CursorStyle, TerminalSettings};
use treeterm_term::alacritty_terminal::grid::Scroll;
use treeterm_term::alacritty_terminal::index::{Column, Line, Point, Side};
use treeterm_term::alacritty_terminal::selection::{Selection, SelectionType};
use treeterm_term::alacritty_terminal::vte::ansi::Rgb;
use treeterm_term::core::{
    CursorShapeSetting, EmulatorSettings, Gate, GridSize, Notice, Responder, SCROLLBACK_LINES,
    SharedTerm, TermCore, apply_settings, write_local,
};
use treeterm_term::fit::WidthLimit;
use treeterm_term::idle::{IdleDetector, IdleEdge, IdleOutput};
use treeterm_term::keys::paste_bytes;
use treeterm_term::palette::Palette;
use treeterm_term::view::{self, ScrollPosition};

/// A non-zero exit this soon after attaching means the program failed to start: the
/// overlay explains it instead of the tab silently closing.
const IMMEDIATE_EXIT_WINDOW: Duration = Duration::from_secs(1);

/// Stream events applied per batch before the UI hears about them, so a flood of output
/// cannot starve the UI thread of updates.
const MAX_EVENTS_PER_BATCH: usize = 512;

#[derive(Debug, Clone)]
pub struct TerminalOptions {
    pub pty_id: String,
    pub background: Rgb,
    pub settings: TerminalSettings,
    /// Keep the tab (with a "Process exited" line) when the program exits.
    pub keep_on_exit: bool,
    pub width_limit: WidthLimit,
    /// `None` turns activity detection off for this terminal (e.g. one analysed by an LLM).
    pub idle_timeout: Option<Duration>,
}

/// What covers the terminal, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// Waiting for the daemon's first message.
    Loading,
    Hidden,
    Error(String),
    /// The program failed right after starting.
    ExitedImmediately(i32),
    Disconnected,
}

impl Overlay {
    pub fn message(&self) -> Option<String> {
        match self {
            Overlay::Loading => Some("Loading terminal...".to_owned()),
            Overlay::Hidden => None,
            Overlay::Error(message) => Some(message.clone()),
            Overlay::ExitedImmediately(code) => {
                Some(format!("Process exited immediately with code {code}"))
            }
            Overlay::Disconnected => Some("Terminal disconnected".to_owned()),
        }
    }
}

/// The daemon applied a different size than this terminal asked for (another attachment
/// shares the PTY).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizeMismatch {
    pub requested: GridSize,
    pub actual: GridSize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The program exited and the tab should close.
    CloseRequested { exit_code: i32 },
    /// The program copied text (OSC 52), already allowed by the settings.
    Clipboard(String),
    Bell,
    Activity(String),
    Idle(IdleEdge),
}

impl EventEmitter<TerminalEvent> for TerminalModel {}

/// How text selection extends with the pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionKind {
    Simple,
    /// Word-wise (double click).
    Semantic,
    /// Line-wise (triple click).
    Lines,
}

/// A grid cell under the pointer: `line` counts from the top of the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPoint {
    pub line: usize,
    pub column: usize,
    /// Which half of the cell, so selection includes it only once the pointer passes its middle.
    pub right_half: bool,
}

/// Everything the parser thread has to tell the UI thread, merged until the UI takes it.
#[derive(Debug, Default)]
struct Batch {
    received: bool,
    data: bool,
    resized: Option<GridSize>,
    exit: Option<i32>,
    end: Option<StreamEnd>,
    notices: Vec<Notice>,
}

#[derive(Debug, Clone)]
enum StreamEnd {
    Closed,
    Failed(String),
}

/// One attachment generation. Refreshing the stream replaces it whole.
struct Attachment {
    term: SharedTerm,
    gate: Arc<Gate>,
    input: PtyInputHandle,
    connected_at: Instant,
    _parser: Task<()>,
    _deliver: Task<()>,
}

pub struct TerminalModel {
    conn: DaemonConnection,
    options: TerminalOptions,
    attachment: Attachment,
    overlay: Overlay,
    title: Option<String>,
    requested: Option<GridSize>,
    size_mismatch: Option<SizeMismatch>,
    scroll_position: ScrollPosition,
    alternate_screen: bool,
    exited: Option<i32>,
    /// Bumped on every batch with output; lets tests and automation wait for output.
    data_version: u64,
    idle: Option<IdleDetector>,
    idle_timer: Option<Task<()>>,
}

fn emulator_settings(settings: &TerminalSettings) -> EmulatorSettings {
    EmulatorSettings {
        cursor: match settings.cursor_style {
            CursorStyle::Block => CursorShapeSetting::Block,
            CursorStyle::Underline => CursorShapeSetting::Underline,
            CursorStyle::Bar => CursorShapeSetting::Bar,
        },
        cursor_blink: settings.cursor_blink,
        allow_osc52_clipboard: settings.allow_osc52_clipboard,
        scrollback: SCROLLBACK_LINES,
    }
}

impl TerminalModel {
    pub fn new(conn: DaemonConnection, options: TerminalOptions, cx: &mut Context<Self>) -> Self {
        let attachment = Self::attach(&conn, &options, cx);
        let now = cx.background_executor().now();
        let idle = options.idle_timeout.map(|timeout| {
            IdleDetector::new(view::snapshot_viewport(&*attachment.term.lock()), timeout, now)
        });
        Self {
            conn,
            options,
            attachment,
            overlay: Overlay::Loading,
            title: None,
            requested: None,
            size_mismatch: None,
            scroll_position: ScrollPosition::Bottom,
            alternate_screen: false,
            exited: None,
            data_version: 0,
            idle,
            idle_timer: None,
        }
    }

    fn attach(conn: &DaemonConnection, options: &TerminalOptions, cx: &mut Context<Self>) -> Attachment {
        let attachment = conn.attach_pty(&options.pty_id, AttachKind::Terminal);
        let input = attachment.input.clone();
        let answers = attachment.input.clone();
        let respond: Responder = Box::new(move |bytes| {
            if answers.send(&bytes).is_err() {
                tracing::debug!("query answer dropped: pty stream closed");
            }
        });
        let core = TermCore::new(
            GridSize { cols: 80, rows: 24 },
            emulator_settings(&options.settings),
            Palette::new(options.background),
            Some(respond),
        );
        let term = core.term().clone();
        let gate = core.gate().clone();

        let inbox = Arc::new(Mutex::new(Batch::default()));
        // Capacity 1 with try_send: at most one wake-up is ever pending, however fast
        // output arrives; the UI drains everything merged so far when it wakes.
        let (wake_tx, mut wake_rx) = mpsc::channel::<()>(1);
        let executor = cx.background_executor().clone();
        let parser = cx.background_executor().spawn(drive(
            core,
            attachment.events,
            inbox.clone(),
            wake_tx,
            executor,
        ));
        let deliver = cx.spawn(async move |this, cx| {
            while wake_rx.next().await.is_some() {
                let batch = std::mem::take(&mut *inbox.lock().expect("terminal inbox poisoned"));
                if this.update(cx, |model, cx| model.on_batch(batch, cx)).is_err() {
                    return; // model dropped
                }
            }
        });
        Attachment {
            term,
            gate,
            input,
            connected_at: cx.background_executor().now(),
            _parser: parser,
            _deliver: deliver,
        }
    }

    pub fn pty_id(&self) -> &str {
        &self.options.pty_id
    }

    pub fn term(&self) -> &SharedTerm {
        &self.attachment.term
    }

    pub fn options(&self) -> &TerminalOptions {
        &self.options
    }

    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    pub fn size_mismatch(&self) -> Option<SizeMismatch> {
        self.size_mismatch
    }

    pub fn scroll_position(&self) -> ScrollPosition {
        self.scroll_position
    }

    pub fn is_pinned(&self) -> bool {
        self.attachment.gate.is_pinned()
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.alternate_screen
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.exited
    }

    pub fn data_version(&self) -> u64 {
        self.data_version
    }

    pub fn grid_size(&self) -> GridSize {
        let term = self.attachment.term.lock();
        use treeterm_term::alacritty_terminal::grid::Dimensions;
        GridSize {
            cols: u16::try_from(term.columns()).unwrap_or(u16::MAX),
            rows: u16::try_from(term.screen_lines()).unwrap_or(u16::MAX),
        }
    }

    /// The screen's text, for activity detection, automation and tests.
    pub fn screen_text(&self) -> String {
        view::screen_text(&*self.attachment.term.lock())
    }

    // ---- input -------------------------------------------------------------------------

    /// Typed input or encoded keys. Dropped while replaying or unfocused: keystrokes can only
    /// come from the focused terminal, and anything else would be the emulator answering a
    /// query that was answered when it first ran.
    pub fn write_input(&mut self, bytes: &[u8]) {
        let gate = &self.attachment.gate;
        if gate.is_replaying() || !gate.is_focused() {
            return;
        }
        if self.attachment.input.send(bytes).is_err() {
            tracing::debug!(pty = %self.options.pty_id, "input dropped: pty stream closed");
        }
        // Typing returns to the newest output, as xterm.js does.
        self.scroll_to_bottom_quietly();
    }

    /// Paste text, bracketed when the program asked for it. Not gated on focus: the context
    /// menu pastes into a terminal that may not hold keyboard focus.
    pub fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        let mode = *self.attachment.term.lock().mode();
        let bytes = paste_bytes(text, mode);
        let write = self.attachment.input.write(&bytes);
        cx.spawn(async move |this, cx| {
            if write.await.is_err() {
                match this.update(cx, |model, cx| {
                    model.overlay = Overlay::Error("pty stream closed".to_owned());
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: model dropped
                }
            }
        })
        .detach();
        self.scroll_to_bottom_quietly();
    }

    /// Whether this terminal holds keyboard focus in the focused window. Gaining it claims
    /// the PTY's input: of the terminal attachments, only the last focused may write.
    pub fn set_focused(&mut self, focused: bool, cx: &mut Context<Self>) {
        let gate = &self.attachment.gate;
        if gate.is_focused() == focused {
            return;
        }
        gate.set_focused(focused);
        if focused {
            if self.attachment.input.focus().is_err() {
                tracing::debug!(pty = %self.options.pty_id, "focus claim dropped: stream closed");
            }
            let mode = *self.attachment.term.lock().mode();
            if let Some(report) = treeterm_term::keys::focus_report(true, mode)
                && self.attachment.input.send(report).is_err()
            {
                tracing::debug!("focus report dropped: stream closed");
            }
        }
        // Cursor turns hollow when unfocused.
        cx.notify();
    }

    pub fn is_focused(&self) -> bool {
        self.attachment.gate.is_focused()
    }

    /// Cell size in pixels, for programs that ask for the text area size.
    pub fn set_cell_size(&self, width: f32, height: f32) {
        self.attachment
            .gate
            .set_cell_size(width.round() as u16, height.round() as u16);
    }

    /// Ask the daemon for a size that fits the view. Nothing resizes locally: the daemon
    /// echoes the size it applied, and only then does the grid change. Does not notify, so
    /// it is safe to call while laying out.
    pub fn request_size(&mut self, size: GridSize) {
        let current = self.grid_size();
        if self.requested == Some(size) || (self.requested.is_none() && current == size) {
            return;
        }
        self.requested = Some(size);
        if self.attachment.input.resize(size.cols, size.rows).is_err() {
            tracing::debug!(pty = %self.options.pty_id, "resize dropped: stream closed");
        }
    }

    pub fn width_limit(&self) -> WidthLimit {
        self.options.width_limit
    }

    pub fn set_width_limit(&mut self, limit: WidthLimit, cx: &mut Context<Self>) {
        self.options.width_limit = limit;
        cx.notify();
    }

    // ---- viewport ----------------------------------------------------------------------

    /// Scroll by `lines` (positive = toward older output). Scrolling up unpins.
    pub fn scroll_lines(&mut self, lines: i32, cx: &mut Context<Self>) {
        if lines > 0 && self.is_pinned() {
            self.attachment.gate.set_pinned(false);
        }
        self.attachment.term.lock().scroll_display(Scroll::Delta(lines));
        self.refresh_viewport_state();
        cx.notify();
    }

    pub fn scroll_page(&mut self, up: bool, cx: &mut Context<Self>) {
        if up && self.is_pinned() {
            self.attachment.gate.set_pinned(false);
        }
        self.attachment.term.lock().scroll_display(if up {
            Scroll::PageUp
        } else {
            Scroll::PageDown
        });
        self.refresh_viewport_state();
        cx.notify();
    }

    pub fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        self.attachment.gate.set_pinned(false);
        self.attachment.term.lock().scroll_display(Scroll::Top);
        self.refresh_viewport_state();
        cx.notify();
    }

    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        self.scroll_to_bottom_quietly();
        cx.notify();
    }

    fn scroll_to_bottom_quietly(&mut self) {
        let mut term = self.attachment.term.lock();
        if term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
        drop(term);
        self.refresh_viewport_state();
    }

    /// Pinned: output always brings the newest line into view.
    pub fn set_pinned(&mut self, pinned: bool, cx: &mut Context<Self>) {
        self.attachment.gate.set_pinned(pinned);
        if pinned {
            self.scroll_to_bottom_quietly();
        }
        cx.notify();
    }

    // ---- selection ---------------------------------------------------------------------

    fn grid_point(&self, at: CellPoint) -> (Point, Side) {
        let offset = self.attachment.term.lock().grid().display_offset();
        let point = Point::new(Line(at.line as i32 - offset as i32), Column(at.column));
        (point, if at.right_half { Side::Right } else { Side::Left })
    }

    pub fn start_selection(&mut self, at: CellPoint, kind: SelectionKind, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(at);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Semantic => SelectionType::Semantic,
            SelectionKind::Lines => SelectionType::Lines,
        };
        self.attachment.term.lock().selection = Some(Selection::new(ty, point, side));
        cx.notify();
    }

    pub fn extend_selection(&mut self, to: CellPoint, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(to);
        if let Some(selection) = self.attachment.term.lock().selection.as_mut() {
            selection.update(point, side);
        }
        cx.notify();
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        let mut term = self.attachment.term.lock();
        if term.selection.take().is_some() {
            cx.notify();
        }
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let mut term = self.attachment.term.lock();
        let mut selection = Selection::new(SelectionType::Simple, Point::default(), Side::Left);
        selection.include_all();
        term.selection = Some(selection);
        cx.notify();
    }

    pub fn selection_text(&self) -> Option<String> {
        self.attachment
            .term
            .lock()
            .selection_to_string()
            .filter(|text| !text.is_empty())
    }

    // ---- lifecycle ---------------------------------------------------------------------

    /// Settings changed. Font settings are the view's; the emulator takes the rest.
    pub fn apply_settings(&mut self, settings: TerminalSettings, cx: &mut Context<Self>) {
        apply_settings(
            &self.attachment.term,
            &self.attachment.gate,
            emulator_settings(&settings),
        );
        if let WidthLimit::Capped(_) = self.options.width_limit {
            self.options.width_limit = WidthLimit::Capped(settings.max_cols);
        }
        self.options.settings = settings;
        cx.notify();
    }

    /// Throw away the emulator and attach again: the daemon replays the PTY's state into a
    /// fresh grid. The "refresh stream" button.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let focused = self.attachment.gate.is_focused();
        let pinned = self.attachment.gate.is_pinned();
        self.attachment = Self::attach(&self.conn, &self.options, cx);
        self.attachment.gate.set_focused(focused);
        self.attachment.gate.set_pinned(pinned);
        self.overlay = Overlay::Loading;
        self.requested = None;
        self.size_mismatch = None;
        self.exited = None;
        self.title = None;
        self.idle_timer = None;
        let now = cx.background_executor().now();
        self.idle = self.options.idle_timeout.map(|timeout| {
            IdleDetector::new(view::snapshot_viewport(&*self.attachment.term.lock()), timeout, now)
        });
        self.refresh_viewport_state();
        cx.notify();
    }

    pub fn dismiss_overlay(&mut self, cx: &mut Context<Self>) {
        self.overlay = Overlay::Hidden;
        cx.notify();
    }

    /// Destroy the PTY. This is close, not detach; the stream then reports the exit.
    pub fn kill(&self, cx: &mut Context<Self>) {
        let kill = self.conn.kill_pty(&self.options.pty_id);
        let pty = self.options.pty_id.clone();
        cx.spawn(async move |this, cx| {
            let failure = match kill.await {
                Ok(Ok(())) => return,
                Ok(Err(error)) => error.to_string(),
                Err(lost) => lost.to_string(),
            };
            tracing::error!(%pty, %failure, "kill pty failed");
            match this.update(cx, |model, cx| {
                model.overlay = Overlay::Error(format!("Failed to kill terminal: {failure}"));
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: model dropped
            }
        })
        .detach();
    }

    // ---- stream ------------------------------------------------------------------------

    fn on_batch(&mut self, batch: Batch, cx: &mut Context<Self>) {
        if batch.received && self.overlay == Overlay::Loading {
            self.overlay = Overlay::Hidden;
        }
        if batch.data {
            self.data_version += 1;
            self.overlay = Overlay::Hidden;
        }
        if let Some(actual) = batch.resized {
            self.size_mismatch = match self.requested {
                Some(requested) if requested != actual => Some(SizeMismatch { requested, actual }),
                Some(_) | None => None,
            };
            self.requested = None;
        }
        for notice in batch.notices {
            match notice {
                Notice::Clipboard(text) => cx.emit(TerminalEvent::Clipboard(text)),
                Notice::Title(title) => self.title = title,
                Notice::Bell => cx.emit(TerminalEvent::Bell),
            }
        }
        if let Some(code) = batch.exit {
            self.on_exit(code, cx);
        }
        match batch.end {
            Some(StreamEnd::Failed(message)) => {
                tracing::error!(pty = %self.options.pty_id, %message, "pty stream failed");
                self.overlay = Overlay::Error(message);
            }
            Some(StreamEnd::Closed) => {
                // An exit or an error already explained why the stream ended.
                let unexplained = matches!(self.overlay, Overlay::Loading | Overlay::Hidden);
                if self.exited.is_none() && unexplained {
                    self.overlay = Overlay::Disconnected;
                }
            }
            None => {}
        }
        self.refresh_viewport_state();
        if batch.data {
            self.detect_activity(cx);
        }
        cx.notify();
    }

    fn on_exit(&mut self, code: i32, cx: &mut Context<Self>) {
        self.exited = Some(code);
        let elapsed = cx.background_executor().now() - self.attachment.connected_at;
        if code != 0 && elapsed < IMMEDIATE_EXIT_WINDOW {
            self.overlay = Overlay::ExitedImmediately(code);
        } else if self.options.keep_on_exit {
            write_local(
                &self.attachment.term,
                format!("\r\n\x1b[2mProcess exited with exit code {code}\x1b[0m\r\n").as_bytes(),
            );
        } else {
            cx.emit(TerminalEvent::CloseRequested { exit_code: code });
        }
    }

    fn refresh_viewport_state(&mut self) {
        let term = self.attachment.term.lock();
        self.scroll_position = view::scroll_position(&*term);
        self.alternate_screen = view::is_alternate_screen(&*term);
    }

    fn detect_activity(&mut self, cx: &mut Context<Self>) {
        let (Some(detector), Some(timeout)) = (self.idle.as_mut(), self.options.idle_timeout) else {
            return;
        };
        let snapshot = view::snapshot_viewport(&*self.attachment.term.lock());
        let now = cx.background_executor().now();
        let outputs = detector.process_snapshot(snapshot, now, timeout);
        self.emit_idle(outputs, cx);
        self.schedule_idle_timer(cx);
    }

    fn schedule_idle_timer(&mut self, cx: &mut Context<Self>) {
        let Some(deadline) = self.idle.as_ref().and_then(IdleDetector::deadline) else {
            self.idle_timer = None;
            return;
        };
        let executor = cx.background_executor().clone();
        let wait = deadline.saturating_duration_since(executor.now());
        self.idle_timer = Some(cx.spawn(async move |this, cx| {
            executor.timer(wait).await;
            match this.update(cx, |model, cx| {
                let now = cx.background_executor().now();
                if let Some(detector) = model.idle.as_mut() {
                    let outputs = detector.on_timer(now);
                    model.emit_idle(outputs, cx);
                }
                model.schedule_idle_timer(cx);
            }) {
                Ok(()) | Err(_) => {} // Err: model dropped
            }
        }));
    }

    fn emit_idle(&mut self, outputs: Vec<IdleOutput>, cx: &mut Context<Self>) {
        for output in outputs {
            match output {
                IdleOutput::Activity(snapshot) => cx.emit(TerminalEvent::Activity(snapshot)),
                IdleOutput::Idle(edge) => cx.emit(TerminalEvent::Idle(edge)),
            }
        }
    }
}

/// The parser thread: applies the PTY's events to the emulator in stream order and merges
/// what the UI needs to know into `inbox`.
async fn drive(
    mut core: TermCore,
    mut events: Sub<Result<PtyEvent, ApiError>>,
    inbox: Arc<Mutex<Batch>>,
    mut wake: mpsc::Sender<()>,
    executor: BackgroundExecutor,
) {
    loop {
        // A synchronized update must be flushed by its deadline even if no output follows.
        let next = match core.sync_deadline() {
            Some(deadline) => {
                let wait = deadline.saturating_duration_since(Instant::now());
                match select(events.next(), executor.timer(wait)).await {
                    Either::Left((next, _timer)) => Some(next),
                    Either::Right(((), _next)) => None,
                }
            }
            None => Some(events.next().await),
        };
        let mut batch = Batch::default();
        let mut ended = false;
        match next {
            None => batch.notices.extend(core.flush_sync()),
            Some(first) => {
                let mut item = first;
                let mut applied = 0;
                loop {
                    match item {
                        Some(Ok(event)) => apply(&mut core, event, &mut batch),
                        Some(Err(error)) => {
                            batch.end = Some(StreamEnd::Failed(error.to_string()));
                            ended = true;
                            break;
                        }
                        None => {
                            batch.end = Some(StreamEnd::Closed);
                            ended = true;
                            break;
                        }
                    }
                    applied += 1;
                    if applied >= MAX_EVENTS_PER_BATCH {
                        break;
                    }
                    match events.next().now_or_never() {
                        Some(ready) => item = ready,
                        None => break,
                    }
                }
            }
        }
        merge(&inbox, batch);
        // Full = a wake-up is already pending and will pick this batch up too.
        match wake.try_send(()) {
            Ok(()) => {}
            Err(error) if error.is_full() => {}
            Err(_disconnected) => return, // the model is gone
        }
        if ended {
            return;
        }
    }
}

fn apply(core: &mut TermCore, event: PtyEvent, batch: &mut Batch) {
    batch.received = true;
    match event {
        PtyEvent::Data(bytes) => {
            batch.data = true;
            batch.notices.extend(core.feed(&bytes));
        }
        PtyEvent::Resize { cols, rows } => {
            let size = GridSize { cols, rows };
            core.resize(size);
            batch.resized = Some(size);
        }
        PtyEvent::ReplayStart => core.replay_start(),
        PtyEvent::ReplayEnd => core.replay_end(),
        PtyEvent::Exit { code, signal: _ } => batch.exit = Some(code),
    }
}

fn merge(inbox: &Mutex<Batch>, batch: Batch) {
    let mut pending = inbox.lock().expect("terminal inbox poisoned");
    pending.received |= batch.received;
    pending.data |= batch.data;
    if batch.resized.is_some() {
        pending.resized = batch.resized;
    }
    if batch.exit.is_some() {
        pending.exit = batch.exit;
    }
    if batch.end.is_some() {
        pending.end = batch.end;
    }
    pending.notices.extend(batch.notices);
}
