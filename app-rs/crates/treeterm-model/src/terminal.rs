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
use treeterm_core::traffic::ByteCounter;
use treeterm_term::alacritty_terminal::grid::{Dimensions, Scroll};
use treeterm_term::alacritty_terminal::index::{Column, Line, Point, Side};
use treeterm_term::alacritty_terminal::selection::{Selection, SelectionType};
use treeterm_term::alacritty_terminal::term::Term;
use treeterm_term::alacritty_terminal::term::cell::Flags;
use treeterm_term::alacritty_terminal::vte::ansi::Rgb;
use treeterm_term::core::{
    CursorShapeSetting, EmulatorSettings, Gate, GridSize, Notice, Responder, SCROLLBACK_LINES,
    SharedTerm, TermCore, apply_settings, write_local,
};
use treeterm_term::fit::{FitState, WidthLimit};
use treeterm_term::idle::{IdleDetector, IdleEdge, IdleOutput};
use treeterm_term::keys::{Modifiers, paste_bytes};
use treeterm_term::links::{self, Link, LinkKind};
use treeterm_term::mouse;
use treeterm_term::move_to_cell::move_to_cell;
use treeterm_term::palette::Palette;
use treeterm_term::raw_chars::RawLog;
use treeterm_term::view::{self, ScrollPosition};

/// A non-zero exit this soon after attaching means the program failed to start: the
/// overlay explains it instead of the tab silently closing.
const IMMEDIATE_EXIT_WINDOW: Duration = Duration::from_secs(1);

/// Stream events applied per batch before the UI hears about them, so a flood of output
/// cannot starve the UI thread of updates.
const MAX_EVENTS_PER_BATCH: usize = 512;

/// How long a click on the scroll badge waits for a second click (a double-click pins)
/// before it scrolls. Electron's `badgeClickTimer`.
pub const BADGE_DOUBLE_CLICK: Duration = Duration::from_millis(250);

/// How often a selection dragged past the grid's top or bottom scrolls the scrollback
/// (xterm.js's `DRAG_SCROLL_INTERVAL`).
pub const DRAG_SCROLL_INTERVAL: Duration = Duration::from_millis(50);

/// An Alt/Option+click moves the program's cursor only when released this soon after the
/// press (xterm.js's `ALT_CLICK_MOVE_CURSOR_TIME`).
pub const ALT_CLICK_MOVE_CURSOR_TIME: Duration = Duration::from_millis(500);

#[derive(Debug, Clone)]
pub struct TerminalOptions {
    pub pty_id: String,
    pub background: Rgb,
    pub settings: TerminalSettings,
    pub role: TerminalRole,
    pub width_limit: WidthLimit,
    pub scrollbar: ScrollbarVisibility,
    pub selection_copy: SelectionCopy,
    /// `None` turns activity detection off for this terminal (e.g. one analysed by an LLM).
    pub idle_timeout: Option<Duration>,
    /// Output bytes are added here (the workspace's download meter).
    pub traffic: ByteCounter,
    /// Settings > Debug > Show Debug Badge: a purple DEBUG badge on the terminal. A
    /// viewer never shows it (`PtyViewer` is no `BaseTerminal`).
    pub show_debug_badge: bool,
}

/// What the terminal is shown as, which decides what a program's exit does and what the
/// terminal's own banner leaves to its container.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerminalRole {
    /// A workspace tab (`BaseTerminal`). The tab closes when the program exits, or stays
    /// with a "Process exited" line when `keep_on_exit`; a non-zero exit right after
    /// attaching shows why instead.
    Tab { keep_on_exit: bool },
    /// The live view of a PTY in Active Processes or the session's TTY list (`PtyViewer`):
    /// an exit adds a grey "[Process exited]" line and the view stays. The viewer shows
    /// attaching and an attach failure itself.
    Viewer,
}

/// When selected text reaches the clipboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionCopy {
    /// On Copy only, as xterm.js does (Electron's Terminal tabs).
    OnCopy,
    /// Also as soon as a drag, double- or triple-click selects it, as ghostty-web does
    /// (Electron's Ghostty tabs).
    OnSelect,
}

/// Whether the scrollback scrollbar is drawn (and can be dragged). The wheel scrolls the
/// scrollback either way, as it does in Electron since xterm.js 6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollbarVisibility {
    Shown,
    /// An AI harness set to "Disable Scrollbar" (`disable-scrollbar`): it scrolls itself.
    Hidden,
}

/// What covers the terminal, if anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// Waiting for the daemon's first message.
    Loading,
    Hidden,
    Error(String),
    /// The attachment failed before the daemon answered (the connection is down): the
    /// error, as Electron shows an attach call that throws.
    AttachFailed(String),
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
            Overlay::AttachFailed(message) => {
                Some(format!("Failed to reattach terminal: {message}"))
            }
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

/// Which viewport changes the idle detector looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdleSnapshots {
    /// After output, as a terminal tab's detector does in Electron (`BaseTerminal`).
    AfterOutput,
    /// After output and after a resize (a reflowed screen is a change), as Electron's
    /// analyzer does with the AI harness it reads. A blank screen reads as nothing at any
    /// size, as there (`extractBuffer() ?? ''`): the first fit of a harness that has printed
    /// nothing yet is no change.
    AfterOutputAndResize,
}

impl IdleSnapshots {
    /// The viewport as the idle detector compares it.
    fn snapshot(self, viewport: String) -> String {
        match self {
            Self::AfterOutputAndResize if viewport.trim().is_empty() => String::new(),
            Self::AfterOutput | Self::AfterOutputAndResize => viewport,
        }
    }
}

/// Whether the grid has the PTY's size. The daemon reports it in every attachment's replay;
/// until then the grid is a placeholder that no fit may be compared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PtySize {
    Unknown,
    Reported,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The program exited and the tab should close.
    CloseRequested { exit_code: i32 },
    /// Text for the clipboard: the program copied it (OSC 52, already allowed by the
    /// settings), or a selection finished with [`SelectionCopy::OnSelect`].
    Clipboard(String),
    Bell,
    Activity(String),
    Idle(IdleEdge),
    /// The user typed (text or encoded keys) or pasted, as written to the PTY, for the
    /// AI-harness analyzer (xterm.js's `onData`).
    Input(String),
    /// The program exited (whether or not the tab closes).
    Exited(i32),
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
    /// A rectangle of columns (Alt/Option+drag).
    Block,
}

/// A grid cell under the pointer: `line` counts from the top of the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellPoint {
    pub line: usize,
    pub column: usize,
    /// Which half of the cell, so selection includes it only once the pointer passes its middle.
    pub right_half: bool,
}

/// The pointer over the grid, for the link under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Pointer {
    pub at: CellPoint,
    /// Cmd (macOS) or Ctrl (other OSes) is held: URLs in the text open then.
    pub secondary: bool,
}

/// What a left press leaves to the terminal's own handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkPress {
    /// It goes on to select text, or to a program reporting the mouse: there is no link under
    /// it, or an OSC 8 hyperlink, whose click xterm.js's linkifier sees beside them.
    Shared,
    /// It is the press of a URL in the text under Cmd/Ctrl, which selects nothing.
    Taken,
}

/// A left press that selects, until its release.
struct SelectionGesture {
    pressed_at: Instant,
    /// The pointer is past the grid's top or bottom edge.
    drag_scroll: Option<DragScroll>,
}

/// A selection dragged past the grid's top or bottom: the scrollback scrolls every
/// [`DRAG_SCROLL_INTERVAL`], and the selection with it.
struct DragScroll {
    /// Lines per tick, negative toward older output, as xterm.js counts them.
    lines: i32,
    /// The pointer, clamped to the grid; a block selection keeps its column.
    at: CellPoint,
    _ticks: Task<()>,
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
    /// The stream failed before its first event: the attach call itself failed.
    AttachFailed(String),
    Failed(String),
}

/// Whether an attachment's stream opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AttachStatus {
    /// Attaching or attached: the stream has not failed before its first event.
    Open,
    /// The attach call failed. Electron has no tty then, so nothing typed, pasted or
    /// reported by the mouse is written, and the attach failure keeps showing.
    Failed,
}

/// Settings > Terminal > Show Raw Characters for one attachment: while it is on, the
/// parser thread logs each chunk of output as a `[RAW]` line, as Electron's `BaseTerminal`
/// logs it to its console.
#[derive(Clone)]
struct RawOutput {
    pty_id: String,
    log: Arc<Mutex<RawLog>>,
}

impl RawOutput {
    fn new(pty_id: &str, on: bool) -> Self {
        Self {
            pty_id: pty_id.to_owned(),
            log: Arc::new(Mutex::new(RawLog::new(on))),
        }
    }

    /// On or off from now on, starting from no output (Electron mounts its terminals again
    /// on a settings change, which empties its `rawChars`).
    fn set(&self, on: bool) {
        *self.log.lock().expect("raw output log poisoned") = RawLog::new(on);
    }

    fn data(&self, bytes: &[u8]) {
        let line = self.log.lock().expect("raw output log poisoned").output(bytes);
        if let Some(escaped) = line {
            tracing::info!(pty = %self.pty_id, "[RAW] {escaped}");
        }
    }
}

/// One attachment generation. Refreshing the stream replaces it whole.
struct Attachment {
    term: SharedTerm,
    gate: Arc<Gate>,
    input: PtyInputHandle,
    raw_output: RawOutput,
    status: AttachStatus,
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
    pty_size: PtySize,
    fit: FitState,
    size_mismatch: Option<SizeMismatch>,
    scroll_position: ScrollPosition,
    alternate_screen: bool,
    exited: Option<i32>,
    /// Bumped on every batch with output; lets tests and automation wait for output.
    data_version: u64,
    idle: Option<IdleDetector>,
    idle_snapshots: IdleSnapshots,
    idle_timer: Option<Task<()>>,
    /// A scroll badge click waiting [`BADGE_DOUBLE_CLICK`] for a second one.
    badge_click: Option<Task<()>>,
    /// The pointer over the grid; `None` when it is elsewhere.
    pointer: Option<Pointer>,
    /// The link a left press went down on, opened if the release comes on it too.
    pressed_link: Option<Link>,
    selection_gesture: Option<SelectionGesture>,
}

/// Where a single click on the scroll badge goes, decided when it is clicked.
#[derive(Debug, Clone, Copy)]
enum BadgeScroll {
    Top,
    Bottom,
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
        let idle_snapshots = IdleSnapshots::AfterOutput;
        let idle = options.idle_timeout.map(|timeout| {
            let snapshot = idle_snapshots.snapshot(view::snapshot_viewport(&*attachment.term.lock()));
            IdleDetector::new(snapshot, timeout, now)
        });
        Self {
            conn,
            options,
            attachment,
            overlay: Overlay::Loading,
            title: None,
            pty_size: PtySize::Unknown,
            fit: FitState::default(),
            size_mismatch: None,
            scroll_position: ScrollPosition::Bottom,
            alternate_screen: false,
            exited: None,
            data_version: 0,
            idle,
            idle_snapshots,
            idle_timer: None,
            badge_click: None,
            pointer: None,
            pressed_link: None,
            selection_gesture: None,
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
        let raw_output = RawOutput::new(&options.pty_id, options.settings.show_raw_chars);

        let inbox = Arc::new(Mutex::new(Batch::default()));
        // Capacity 1 with try_send: at most one wake-up is ever pending, however fast
        // output arrives; the UI drains everything merged so far when it wakes.
        let (wake_tx, mut wake_rx) = mpsc::channel::<()>(1);
        let executor = cx.background_executor().clone();
        let parser = cx.background_executor().spawn(drive(
            core,
            attachment.events,
            options.traffic.clone(),
            raw_output.clone(),
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
            raw_output,
            status: AttachStatus::Open,
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

    /// The overlay the terminal draws over itself. A viewer leaves attaching and an attach
    /// failure to the viewer around it, which shows them in its own way (`PtyViewer`).
    pub fn banner(&self) -> Overlay {
        match (self.options.role, &self.overlay) {
            (TerminalRole::Viewer, Overlay::Loading | Overlay::AttachFailed(_)) => Overlay::Hidden,
            (TerminalRole::Viewer | TerminalRole::Tab { .. }, overlay) => overlay.clone(),
        }
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Whether the purple DEBUG badge shows (Settings > Debug > Show Debug Badge).
    pub fn show_debug_badge(&self) -> bool {
        self.options.show_debug_badge
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
    /// query that was answered when it first ran. Dropped too after the attach failed, as
    /// Electron then has no tty. A write that fails shows why, as Electron does.
    pub fn write_input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let gate = &self.attachment.gate;
        if gate.is_replaying() || !gate.is_focused() {
            return;
        }
        match self.attachment.status {
            AttachStatus::Failed => return,
            AttachStatus::Open => {}
        }
        if let Err(error) = self.attachment.input.send(bytes) {
            tracing::error!(pty = %self.options.pty_id, %error, "pty write failed");
            self.overlay = Overlay::Error(error.to_string());
            cx.notify();
        }
        cx.emit(TerminalEvent::Input(String::from_utf8_lossy(bytes).into_owned()));
        // Typing returns to the newest output, as xterm.js does.
        self.scroll_to_bottom_quietly();
    }

    /// The visible screen as the idle detector and analyzer read it.
    pub fn viewport(&self) -> String {
        view::snapshot_viewport(&*self.attachment.term.lock())
    }

    /// The viewport as the idle detector compares it ([`IdleSnapshots`]).
    fn idle_snapshot(&self) -> String {
        self.idle_snapshots.snapshot(self.viewport())
    }

    /// Change how long output must be quiet before an idle edge (`None`: no detection).
    /// Takes effect from the next burst.
    pub fn set_idle_timeout(&mut self, timeout: Option<Duration>, cx: &mut Context<Self>) {
        if self.options.idle_timeout == timeout {
            return;
        }
        self.options.idle_timeout = timeout;
        match (timeout, self.idle.is_some()) {
            (Some(timeout), false) => {
                let now = cx.background_executor().now();
                self.idle = Some(IdleDetector::new(self.idle_snapshot(), timeout, now));
            }
            (None, true) => {
                self.idle = None;
                self.idle_timer = None;
            }
            (Some(_), true) | (None, false) => {}
        }
    }

    /// Which viewport changes the idle detector looks at from now on. Detection starts over
    /// from the screen as now read, so set it before output arrives (the analyzer does, as it
    /// is created with the terminal).
    pub fn set_idle_snapshots(&mut self, snapshots: IdleSnapshots, cx: &mut Context<Self>) {
        if self.idle_snapshots == snapshots {
            return;
        }
        self.idle_snapshots = snapshots;
        self.idle_timer = None;
        let now = cx.background_executor().now();
        self.idle = self
            .options
            .idle_timeout
            .map(|timeout| IdleDetector::new(self.idle_snapshot(), timeout, now));
    }

    /// Report an idle edge now if output is quiet, else at the end of the current burst.
    pub fn request_idle_edge(&mut self, cx: &mut Context<Self>) {
        let Some(detector) = self.idle.as_mut() else {
            return;
        };
        let outputs = detector.request_idle_edge();
        self.emit_idle(outputs, cx);
        self.schedule_idle_timer(cx);
    }

    /// Something the viewport text does not show changed (the analyzer's configuration): the
    /// idle detector counts it as a change of screen, as it would new output.
    pub fn mark_viewport_changed(&mut self, cx: &mut Context<Self>) {
        let (Some(detector), Some(timeout)) = (self.idle.as_mut(), self.options.idle_timeout) else {
            return;
        };
        let now = cx.background_executor().now();
        let outputs = detector.mark_changed(now, timeout);
        self.emit_idle(outputs, cx);
        self.schedule_idle_timer(cx);
    }

    /// Paste text, bracketed when the program asked for it. Not gated on focus: the context
    /// menu pastes into a terminal that may not hold keyboard focus. Claims the PTY's input
    /// first, as the key press or right-click that pastes does in Electron. The paste is
    /// input like typing (a pasted line ending starts the analyzer's title capture). A paste
    /// that fails shows why, as Electron does, even over a newer attachment: one pending
    /// across a refresh fails with "pty stream detached". Nothing is pasted after the attach
    /// failed, as Electron then has no tty.
    pub fn paste(&mut self, text: &str, cx: &mut Context<Self>) {
        match self.attachment.status {
            AttachStatus::Failed => return,
            AttachStatus::Open => {}
        }
        self.claim_input();
        let mode = *self.attachment.term.lock().mode();
        let bytes = paste_bytes(text, mode);
        let write = self.attachment.input.write(&bytes);
        cx.emit(TerminalEvent::Input(String::from_utf8_lossy(&bytes).into_owned()));
        let pty = self.options.pty_id.clone();
        cx.spawn(async move |this, cx| {
            if let Err(error) = write.await {
                tracing::error!(%pty, %error, "paste write failed");
                match this.update(cx, |model, cx| {
                    model.overlay = Overlay::Error(error.to_string());
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
            self.claim_input();
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

    /// Claim the PTY's input for this attachment: the daemon takes writes only from the
    /// terminal attachment that claimed last. Sent whenever the user arrives at or acts in
    /// the terminal (Electron's `onKeyboardFocus`), since another machine or window may have
    /// claimed since and no focus change fires at a terminal that kept focus. Sends no
    /// focus report: those go out on focus changes only.
    pub fn claim_input(&self) {
        if self.attachment.input.focus().is_err() {
            tracing::debug!(pty = %self.options.pty_id, "focus claim dropped: stream closed");
        }
    }

    /// A key press in this terminal, before it is written: claims the PTY's input while the
    /// terminal holds keyboard focus, as Electron does on every keydown.
    pub fn key_pressed(&self) {
        if self.is_focused() {
            self.claim_input();
        }
    }

    /// Cell size in pixels, for programs that ask for the text area size.
    pub fn set_cell_size(&self, width: f32, height: f32) {
        self.attachment
            .gate
            .set_cell_size(width.round() as u16, height.round() as u16);
    }

    /// The view laid out and `wanted` fits it. The daemon is asked for it when the view's fit
    /// changed and the grid does not have it already; a size another attachment applied is
    /// otherwise kept (and flagged by [`Self::size_mismatch`]). Nothing resizes locally: the
    /// daemon echoes the size it applied, and only then does the grid change. Does not
    /// notify, so it is safe to call while laying out.
    pub fn fit(&mut self, wanted: GridSize) {
        if self.pty_size == PtySize::Unknown {
            return;
        }
        let Some(size) = self.fit.propose(wanted, self.grid_size()) else {
            return;
        };
        if self.attachment.input.resize(size.cols, size.rows).is_err() {
            tracing::debug!(pty = %self.options.pty_id, "resize dropped: stream closed");
        }
    }

    /// The view came into view again, as an Electron terminal mounts again: its next fit is
    /// asked for even if unchanged, taking the PTY back from a size another attachment set.
    pub fn refit(&mut self, cx: &mut Context<Self>) {
        self.fit.reset();
        self.size_mismatch = None;
        cx.notify();
    }

    pub fn width_limit(&self) -> WidthLimit {
        self.options.width_limit
    }

    /// Electron fits again when the switch flips, comparing the fit with the grid even if the
    /// limit leaves it unchanged (a container narrower than the cap): that takes the PTY back
    /// from a size another attachment set. The badge stays until the daemon echoes.
    pub fn set_width_limit(&mut self, limit: WidthLimit, cx: &mut Context<Self>) {
        if self.options.width_limit == limit {
            return;
        }
        self.options.width_limit = limit;
        self.fit.forget_fit();
        cx.notify();
    }

    /// The application's "Disable Scrollbar" changed.
    pub fn set_scrollbar(&mut self, scrollbar: ScrollbarVisibility, cx: &mut Context<Self>) {
        if self.options.scrollbar == scrollbar {
            return;
        }
        self.options.scrollbar = scrollbar;
        cx.notify();
    }

    /// The application's background changed. The emulator answers a program's OSC 11 query
    /// with the background it attached with until the stream is refreshed.
    pub fn set_background(&mut self, background: Rgb, cx: &mut Context<Self>) {
        if self.options.background == background {
            return;
        }
        self.options.background = background;
        cx.notify();
    }

    /// Settings > Debug > Show Debug Badge changed.
    pub fn set_debug_badge(&mut self, shown: bool, cx: &mut Context<Self>) {
        if self.options.show_debug_badge == shown {
            return;
        }
        self.options.show_debug_badge = shown;
        cx.notify();
    }

    // ---- viewport ----------------------------------------------------------------------

    /// The mouse wheel turned toward older output. That unpins, whatever the wheel then does
    /// (scroll the scrollback, report to a mouse-mode program or send arrow keys).
    pub fn wheel_up(&mut self, cx: &mut Context<Self>) {
        if self.is_pinned() {
            self.attachment.gate.set_pinned(false);
            cx.notify();
        }
    }

    /// Scroll by `lines` (positive = toward older output).
    pub fn scroll_lines(&mut self, lines: i32, cx: &mut Context<Self>) {
        self.attachment.term.lock().scroll_display(Scroll::Delta(lines));
        self.after_scroll(cx);
    }

    /// Shift+PageUp/PageDown.
    pub fn scroll_page(&mut self, up: bool, cx: &mut Context<Self>) {
        self.attachment.term.lock().scroll_display(if up {
            Scroll::PageUp
        } else {
            Scroll::PageDown
        });
        self.after_scroll(cx);
    }

    /// Jump the viewport to `ratio` of the scrollback (0 = oldest, 1 = newest), as the
    /// scrollbar does.
    pub fn scroll_to_ratio(&mut self, ratio: f64, cx: &mut Context<Self>) {
        view::scroll_to_ratio(&mut *self.attachment.term.lock(), ratio);
        self.after_scroll(cx);
    }

    /// A pinned terminal goes back to the bottom whatever scrolled it away (a page key, the
    /// scrollbar), as Electron's `onScroll` does; unpinning is the wheel's and the badge's.
    fn after_scroll(&mut self, cx: &mut Context<Self>) {
        if self.is_pinned() {
            let mut term = self.attachment.term.lock();
            if view::scroll_position(&*term) != ScrollPosition::Bottom {
                term.scroll_display(Scroll::Bottom);
            }
        }
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

    /// A click on the scroll badge. Pinned (output always brings the newest line into view):
    /// unpins and stays put. Otherwise a second click within [`BADGE_DOUBLE_CLICK`] pins,
    /// and a single click then scrolls: to the top from the bottom, else to the bottom.
    pub fn click_scroll_badge(&mut self, cx: &mut Context<Self>) {
        if self.is_pinned() {
            self.attachment.gate.set_pinned(false);
            cx.notify();
        } else if self.badge_click.take().is_some() {
            self.attachment.gate.set_pinned(true);
            self.scroll_to_bottom(cx);
        } else {
            let target = match self.scroll_position {
                ScrollPosition::Bottom => BadgeScroll::Top,
                ScrollPosition::Top | ScrollPosition::Middle => BadgeScroll::Bottom,
            };
            let executor = cx.background_executor().clone();
            self.badge_click = Some(cx.spawn(async move |this, cx| {
                executor.timer(BADGE_DOUBLE_CLICK).await;
                match this.update(cx, |model, cx| {
                    model.badge_click = None;
                    match target {
                        BadgeScroll::Top => model.scroll_to_top(cx),
                        BadgeScroll::Bottom => model.scroll_to_bottom(cx),
                    }
                }) {
                    Ok(()) | Err(_) => {} // Err: model dropped
                }
            }));
        }
    }

    // ---- selection ---------------------------------------------------------------------

    fn grid_point(&self, at: CellPoint) -> (Point, Side) {
        grid_point(&self.attachment.term.lock(), at)
    }

    pub fn start_selection(&mut self, at: CellPoint, kind: SelectionKind, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(at);
        let ty = match kind {
            SelectionKind::Simple => SelectionType::Simple,
            SelectionKind::Semantic => SelectionType::Semantic,
            SelectionKind::Lines => SelectionType::Lines,
            SelectionKind::Block => SelectionType::Block,
        };
        self.attachment.term.lock().selection = Some(Selection::new(ty, point, side));
        cx.notify();
    }

    /// A left press that selects: not one a link takes, nor one reported to a program. With
    /// Shift, and no program tracking the mouse, it extends the selection to the press
    /// (xterm.js's incremental click; a plain click leaves an empty selection to extend).
    /// Otherwise it starts one: by character, by column with Alt/Option (on every platform,
    /// as xterm.js's `shouldColumnSelect`), by word on a double click, by line on a triple.
    pub fn press_selection(&mut self, at: CellPoint, click_count: usize, modifiers: Modifiers, cx: &mut Context<Self>) {
        self.selection_gesture = Some(SelectionGesture {
            pressed_at: cx.background_executor().now(),
            drag_scroll: None,
        });
        let mode = *self.attachment.term.lock().mode();
        if modifiers.shift && !mouse::mouse_mode_active(mode) {
            self.extend_selection(at, cx);
            return;
        }
        let kind = match click_count {
            0 | 1 if modifiers.alt => SelectionKind::Block,
            0 | 1 => SelectionKind::Simple,
            2 => SelectionKind::Semantic,
            _ => SelectionKind::Lines,
        };
        self.start_selection(at, kind, cx);
    }

    /// Whether a left press is selecting, until its release.
    pub fn is_selecting(&self) -> bool {
        self.selection_gesture.is_some()
    }

    fn extend_selection(&mut self, to: CellPoint, cx: &mut Context<Self>) {
        let (point, side) = self.grid_point(to);
        if let Some(selection) = self.attachment.term.lock().selection.as_mut() {
            selection.update(point, side);
            cx.notify();
        }
    }

    /// The pointer moved with the left button held after [`Self::press_selection`], to `to`
    /// (clamped to the grid) and `overshoot` pixels above (negative) or below (positive) the
    /// grid, 0 over it. The selection follows it. Past the top or bottom it ends at the
    /// edge row's start or end (a block selection keeps its column), and the scrollback
    /// scrolls every [`DRAG_SCROLL_INTERVAL`], faster the further out, until the pointer
    /// comes back or the button comes up: xterm.js's drag scroll.
    pub fn drag_selection(&mut self, to: CellPoint, overshoot: f32, cx: &mut Context<Self>) {
        let Some(gesture) = self.selection_gesture.as_mut() else {
            return;
        };
        let lines = mouse::drag_scroll_lines(overshoot);
        gesture.drag_scroll = match (lines, gesture.drag_scroll.take()) {
            (0, _) => None,
            (lines, Some(scroll)) => Some(DragScroll { lines, at: to, ..scroll }),
            (lines, None) => Some(DragScroll {
                lines,
                at: to,
                _ticks: Self::drag_scroll_ticks(cx),
            }),
        };
        if drag_end(&mut *self.attachment.term.lock(), to, lines) {
            cx.notify();
        }
    }

    fn drag_scroll_ticks(cx: &mut Context<Self>) -> Task<()> {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                executor.timer(DRAG_SCROLL_INTERVAL).await;
                if this.update(cx, |model, cx| model.drag_scroll_tick(cx)).is_err() {
                    return; // model dropped
                }
            }
        })
    }

    /// Scroll a selection dragged past the grid's edge, and take it to the row that came
    /// into view there. A selection that went meanwhile (cleared by output) stays gone.
    fn drag_scroll_tick(&mut self, cx: &mut Context<Self>) {
        let Some(scroll) = self.selection_gesture.as_ref().and_then(|gesture| gesture.drag_scroll.as_ref()) else {
            return;
        };
        let (lines, at) = (scroll.lines, scroll.at);
        if self.attachment.term.lock().selection.is_none() {
            return;
        }
        self.scroll_lines(-lines, cx);
        let mut term = self.attachment.term.lock();
        let edge = if lines > 0 { term.screen_lines() - 1 } else { 0 };
        drag_end(&mut *term, CellPoint { line: edge, ..at }, lines);
    }

    /// The left button came up at `at` after [`Self::press_selection`]. An Alt/Option+click
    /// released within [`ALT_CLICK_MOVE_CURSOR_TIME`] that selected at most one character
    /// moves the program's cursor there with arrow keys, when the newest output is in view
    /// (xterm.js's `altClickMovesCursor`). Otherwise a terminal with
    /// [`SelectionCopy::OnSelect`] copies what was selected, as ghostty-web does.
    pub fn release_selection(&mut self, at: CellPoint, modifiers: Modifiers, cx: &mut Context<Self>) {
        let Some(gesture) = self.selection_gesture.take() else {
            return;
        };
        let text = self.selection_text();
        let held = cx.background_executor().now().saturating_duration_since(gesture.pressed_at);
        // xterm.js counts the selected text in UTF-16 code units.
        let selected = text.as_ref().map_or(0, |text| text.encode_utf16().count());
        if modifiers.alt && held < ALT_CLICK_MOVE_CURSOR_TIME && selected <= 1 {
            self.move_cursor_to(at, cx);
            return;
        }
        match (self.options.selection_copy, text) {
            (SelectionCopy::OnSelect, Some(text)) => cx.emit(TerminalEvent::Clipboard(text)),
            (SelectionCopy::OnSelect, None) | (SelectionCopy::OnCopy, _) => {}
        }
    }

    /// Send the arrow keys that take the program's cursor to `at`, as typed input (it clears
    /// a selection, as xterm.js's user input does).
    fn move_cursor_to(&mut self, at: CellPoint, cx: &mut Context<Self>) {
        let keys = {
            let term = self.attachment.term.lock();
            if term.grid().display_offset() != 0 {
                return;
            }
            move_to_cell(&term, at.column, at.line)
        };
        if self.selection_text().is_some() {
            self.clear_selection(cx);
        }
        if !keys.is_empty() {
            self.write_input(&keys, cx);
        }
    }

    /// A right-click on macOS, before its menu opens: selects the word under the pointer
    /// unless the pointer is in the selection, so the menu's Copy copies it (xterm.js's
    /// `rightClickSelectsWord`). On blank space it selects nothing, and a selection by
    /// character or column that it is outside of goes, as in xterm.js; one by word or line
    /// stays.
    pub fn select_word_unless_selected(&mut self, at: CellPoint, cx: &mut Context<Self>) {
        let mut term = self.attachment.term.lock();
        let (point, side) = grid_point(&*term, at);
        let selection = term.selection.as_ref();
        if selection.and_then(|selection| selection.to_range(&*term)).is_some_and(|range| range.contains(point)) {
            return;
        }
        let cell = &term.grid()[point];
        let blank = !cell.flags.contains(Flags::WIDE_CHAR_SPACER) && (cell.c.is_whitespace() || cell.c == '\0');
        if !blank {
            term.selection = Some(Selection::new(SelectionType::Semantic, point, side));
            cx.notify();
            return;
        }
        match term.selection.as_ref().map(|selection| selection.ty) {
            Some(SelectionType::Simple | SelectionType::Block) => {
                term.selection = None;
                cx.notify();
            }
            Some(SelectionType::Semantic | SelectionType::Lines) | None => {}
        }
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

    // ---- links -------------------------------------------------------------------------

    /// The link at a cell: an OSC 8 `http(s)` hyperlink, or a URL in the text (see
    /// [`links`]).
    pub fn link_at(&self, at: CellPoint) -> Option<Link> {
        let (point, _) = self.grid_point(at);
        links::link_at(&*self.attachment.term.lock(), point)
    }

    /// The link under the pointer that a click there opens: underlined, with a pointer
    /// cursor, as xterm.js shows a hovered link. A URL in the text is one only while Cmd/Ctrl
    /// is held.
    pub fn hovered_link(&self) -> Option<Link> {
        self.link_to_open(self.pointer?)
    }

    /// The link a click at `pointer` opens.
    fn link_to_open(&self, pointer: Pointer) -> Option<Link> {
        self.link_at(pointer.at).filter(|link| link.opens(pointer.secondary))
    }

    /// The pointer moved, onto the grid or off it (`None`).
    pub fn hover(&mut self, pointer: Option<Pointer>, cx: &mut Context<Self>) {
        let before = self.hovered_link();
        self.pointer = pointer;
        if self.hovered_link() != before {
            cx.notify();
        }
    }

    /// Cmd/Ctrl went down or up with the pointer where it was.
    pub fn hover_secondary(&mut self, secondary: bool, cx: &mut Context<Self>) {
        let pointer = self.pointer.map(|pointer| Pointer { secondary, ..pointer });
        self.hover(pointer, cx);
    }

    /// A left press, on the grid or off it (`None`): the link a click there opens is held
    /// until the release, as xterm.js's linkifier holds its mouse-down link.
    pub fn press_link(&mut self, pointer: Option<Pointer>) -> LinkPress {
        self.pressed_link = pointer.and_then(|pointer| self.link_to_open(pointer));
        match self.pressed_link.as_ref().map(|link| link.kind) {
            Some(LinkKind::Url) => LinkPress::Taken,
            Some(LinkKind::Hyperlink) | None => LinkPress::Shared,
        }
    }

    /// The left button came up, on the grid or off it (`None`). A release on the link it
    /// went down on opens it in the browser, as xterm.js activates a link on mouse-up; one
    /// dragged off it opens nothing.
    pub fn release_link(&mut self, at: Option<CellPoint>, cx: &mut Context<Self>) {
        if let Some(link) = self.pressed_link.take()
            && at.and_then(|at| self.link_at(at)).as_ref() == Some(&link)
        {
            cx.open_url(&link.uri);
        }
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
        self.attachment.raw_output.set(settings.show_raw_chars);
        self.options.settings = settings;
        // Electron mounts its terminals again on a settings change, which fits them again.
        self.fit.reset();
        cx.notify();
    }

    /// Throw away the emulator and attach again: the daemon replays the PTY's state into a
    /// fresh grid. The "refresh stream" button.
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let focused = self.attachment.gate.is_focused();
        let pinned = self.attachment.gate.is_pinned();
        self.attachment = Self::attach(&self.conn, &self.options, cx);
        self.attachment.gate.set_focused(focused);
        if focused {
            // The new attachment owns the input at once, as Electron's remount claims.
            self.claim_input();
        }
        self.attachment.gate.set_pinned(pinned);
        self.overlay = Overlay::Loading;
        self.pty_size = PtySize::Unknown;
        self.fit.reset();
        self.size_mismatch = None;
        self.exited = None;
        self.title = None;
        self.idle_timer = None;
        let now = cx.background_executor().now();
        self.idle = self
            .options
            .idle_timeout
            .map(|timeout| IdleDetector::new(self.idle_snapshot(), timeout, now));
        self.refresh_viewport_state();
        cx.notify();
    }

    /// A click on the banner. "Loading terminal..." has no click handler in Electron: it
    /// stays until the daemon answers.
    pub fn dismiss_overlay(&mut self, cx: &mut Context<Self>) {
        match self.overlay {
            Overlay::Loading | Overlay::Hidden => {}
            Overlay::Error(_)
            | Overlay::AttachFailed(_)
            | Overlay::ExitedImmediately(_)
            | Overlay::Disconnected => {
                self.overlay = Overlay::Hidden;
                cx.notify();
            }
        }
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
            self.pty_size = PtySize::Reported;
            // The last request stands: a size another attachment applies later shows too.
            self.size_mismatch = self
                .fit
                .mismatch(actual)
                .map(|(requested, actual)| SizeMismatch { requested, actual });
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
            Some(StreamEnd::AttachFailed(message)) => {
                tracing::error!(pty = %self.options.pty_id, %message, "failed to attach to PTY");
                self.attachment.status = AttachStatus::Failed;
                self.overlay = Overlay::AttachFailed(message);
            }
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
        let resize_snapshot = self.idle_snapshots == IdleSnapshots::AfterOutputAndResize;
        if batch.data || (batch.resized.is_some() && resize_snapshot) {
            self.detect_activity(cx);
        }
        cx.notify();
    }

    fn on_exit(&mut self, code: i32, cx: &mut Context<Self>) {
        self.exited = Some(code);
        cx.emit(TerminalEvent::Exited(code));
        let keep_on_exit = match self.options.role {
            TerminalRole::Tab { keep_on_exit } => keep_on_exit,
            TerminalRole::Viewer => {
                write_local(&self.attachment.term, b"\r\n\x1b[90m[Process exited]\x1b[0m\r\n");
                return;
            }
        };
        let elapsed = cx.background_executor().now() - self.attachment.connected_at;
        if code != 0 && elapsed < IMMEDIATE_EXIT_WINDOW {
            self.overlay = Overlay::ExitedImmediately(code);
        } else if keep_on_exit {
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
        if self.idle.is_none() {
            return;
        }
        let snapshot = self.idle_snapshot();
        let (Some(detector), Some(timeout)) = (self.idle.as_mut(), self.options.idle_timeout) else {
            return;
        };
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

/// The grid point of a viewport cell, and the side of it the pointer is on.
fn grid_point<T>(term: &Term<T>, at: CellPoint) -> (Point, Side) {
    let offset = term.grid().display_offset();
    let point = Point::new(Line(at.line as i32 - offset as i32), Column(at.column));
    (point, if at.right_half { Side::Right } else { Side::Left })
}

/// End the selection at `at`, dragged there with the scrollback scrolling `lines` per tick
/// (0: not scrolling). While it scrolls, the end goes to the row's start going up and its
/// end going down, unless the selection is a block. Whether there was a selection to end.
fn drag_end<T>(term: &mut Term<T>, at: CellPoint, lines: i32) -> bool {
    let (point, side) = grid_point(term, at);
    let last_column = term.last_column();
    let Some(selection) = term.selection.as_mut() else {
        return false;
    };
    let (point, side) = match (lines.signum(), selection.ty) {
        (0, _) | (_, SelectionType::Block) => (point, side),
        (1, _) => (Point::new(point.line, last_column), Side::Right),
        (_, _) => (Point::new(point.line, Column(0)), Side::Left),
    };
    selection.update(point, side);
    true
}

/// The parser thread: applies the PTY's events to the emulator in stream order and merges
/// what the UI needs to know into `inbox`.
async fn drive(
    mut core: TermCore,
    mut events: Sub<Result<PtyEvent, ApiError>>,
    received: ByteCounter,
    raw_output: RawOutput,
    inbox: Arc<Mutex<Batch>>,
    mut wake: mpsc::Sender<()>,
    executor: BackgroundExecutor,
) {
    // The daemon answers every attach with its replay before anything can fail, so an error
    // before the first event is the attach call failing (the connection is down).
    let mut opened = false;
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
                        Some(Ok(event)) => {
                            opened = true;
                            apply(&mut core, event, &received, &raw_output, &mut batch);
                        }
                        Some(Err(error)) => {
                            let message = error.to_string();
                            batch.end = Some(if opened {
                                StreamEnd::Failed(message)
                            } else {
                                StreamEnd::AttachFailed(message)
                            });
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

fn apply(
    core: &mut TermCore,
    event: PtyEvent,
    received: &ByteCounter,
    raw_output: &RawOutput,
    batch: &mut Batch,
) {
    batch.received = true;
    match event {
        PtyEvent::Data(bytes) => {
            received.add(bytes.len() as u64);
            batch.data = true;
            batch.notices.extend(core.feed(&bytes));
            // Replayed output too, as in Electron.
            raw_output.data(&bytes);
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
