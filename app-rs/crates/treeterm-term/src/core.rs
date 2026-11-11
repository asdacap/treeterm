//! One terminal's emulation state, fed from a PTY's output stream.
//!
//! [`TermCore`] owns the parser and is driven from one place, in stream order (the
//! daemon's `PtyStream`). The grid lives behind a [`SharedTerm`] so a renderer can read
//! it from another thread while output is being parsed.
//!
//! Every attached terminal parses the same queries (DA, DECRQM, OSC 11, ...), but the
//! program must see only one answer. Answers are therefore sent only by a core that is
//! [`Gate::set_focused`] and not replaying: the replay holds queries that were answered
//! when they first ran.

use std::sync::atomic::{AtomicBool, AtomicU16, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::{ClipboardType, Config, Osc52, Term};
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Processor};

use crate::palette::Palette;
use crate::view;

/// Scrollback kept per terminal; the Electron app keeps the same.
pub const SCROLLBACK_LINES: usize = 50_000;

/// Largest clipboard write a program may make through OSC 52.
pub const MAX_OSC52_BYTES: usize = 1024 * 1024;

pub type SharedTerm = Arc<FairMutex<Term<Listener>>>;

/// A terminal's grid size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// Display settings the emulator itself needs (the renderer has its own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmulatorSettings {
    pub cursor: CursorShapeSetting,
    pub cursor_blink: bool,
    pub allow_osc52_clipboard: bool,
    pub scrollback: usize,
}

/// `settings.terminal.cursorStyle`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShapeSetting {
    Block,
    Underline,
    Bar,
}

impl CursorShapeSetting {
    fn shape(self) -> CursorShape {
        match self {
            CursorShapeSetting::Block => CursorShape::Block,
            CursorShapeSetting::Underline => CursorShape::Underline,
            CursorShapeSetting::Bar => CursorShape::Beam,
        }
    }
}

impl EmulatorSettings {
    fn config(&self) -> Config {
        Config {
            scrolling_history: self.scrollback,
            default_cursor_style: CursorStyle {
                shape: self.cursor.shape(),
                blinking: self.cursor_blink,
            },
            osc52: if self.allow_osc52_clipboard {
                Osc52::OnlyCopy
            } else {
                Osc52::Disabled
            },
            ..Config::default()
        }
    }
}

/// Settings changed while the terminal is alive. Safe from any thread.
pub fn apply_settings(term: &SharedTerm, gate: &Gate, settings: EmulatorSettings) {
    gate.allow_osc52_clipboard
        .store(settings.allow_osc52_clipboard, Ordering::Release);
    term.lock().set_options(settings.config());
}

/// Write text of our own into the terminal (not program output), e.g. the "Process exited"
/// line after the PTY is gone. Uses a fresh parser: the stream's parser may sit in the middle
/// of an escape sequence the dead program never finished.
pub fn write_local(term: &SharedTerm, bytes: &[u8]) {
    let mut parser: Processor = Processor::new();
    parser.advance(&mut *term.lock(), bytes);
}

/// Collects the emulator's events while the grid is locked; [`TermCore`] handles them once
/// the parser has let go of the lock.
#[derive(Clone, Default)]
pub struct Listener {
    pending: Arc<Mutex<Vec<Event>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            // Redraw scheduling is the owner's business; it redraws after every batch.
            Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            other => self
                .pending
                .lock()
                .expect("listener queue poisoned")
                .push(other),
        }
    }
}

/// State shared between the thread that parses output and the UI thread.
#[derive(Debug, Default)]
pub struct Gate {
    focused: AtomicBool,
    replaying: AtomicBool,
    pinned: AtomicBool,
    allow_osc52_clipboard: AtomicBool,
    cell_width: AtomicU16,
    cell_height: AtomicU16,
}

impl Gate {
    /// Inside the daemon's attach-time replay. Typed input is dropped meanwhile, as in the
    /// Electron app.
    pub fn is_replaying(&self) -> bool {
        self.replaying.load(Ordering::Acquire)
    }

    /// Pinned to the bottom: output and resizes always bring the newest line into view,
    /// even if something scrolled the viewport.
    pub fn set_pinned(&self, pinned: bool) {
        self.pinned.store(pinned, Ordering::Release);
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned.load(Ordering::Acquire)
    }

    /// Whether this terminal holds keyboard focus in the focused window.
    pub fn set_focused(&self, focused: bool) {
        self.focused.store(focused, Ordering::Release);
    }

    pub fn is_focused(&self) -> bool {
        self.focused.load(Ordering::Acquire)
    }

    /// Cell size in pixels, for programs that ask for the text area size (CSI 14 t).
    pub fn set_cell_size(&self, width: u16, height: u16) {
        self.cell_width.store(width, Ordering::Release);
        self.cell_height.store(height, Ordering::Release);
    }
}

/// Something the UI should act on, produced while parsing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    /// OSC 52 copy, already checked against the setting and size cap.
    Clipboard(String),
    /// OSC 0/2 title; `None` resets it.
    Title(Option<String>),
    Bell,
}

/// Sends bytes to the PTY. Must not block; answers are queued in order.
pub type Responder = Box<dyn Fn(Vec<u8>) + Send>;

pub struct TermCore {
    term: SharedTerm,
    parser: Processor,
    pending: Arc<Mutex<Vec<Event>>>,
    gate: Arc<Gate>,
    respond: Option<Responder>,
    palette: Palette,
}

impl TermCore {
    /// `respond` is `None` for a core that must never write (a background analyzer).
    pub fn new(
        size: GridSize,
        settings: EmulatorSettings,
        palette: Palette,
        respond: Option<Responder>,
    ) -> Self {
        let listener = Listener::default();
        let pending = listener.pending.clone();
        let term = Term::new(settings.config(), &size, listener);
        let gate = Arc::new(Gate::default());
        gate.allow_osc52_clipboard
            .store(settings.allow_osc52_clipboard, Ordering::Release);
        Self {
            term: Arc::new(FairMutex::new(term)),
            parser: Processor::new(),
            pending,
            gate,
            respond,
            palette,
        }
    }

    pub fn term(&self) -> &SharedTerm {
        &self.term
    }

    pub fn gate(&self) -> &Arc<Gate> {
        &self.gate
    }

    pub fn is_replaying(&self) -> bool {
        self.gate.is_replaying()
    }

    /// The daemon's attach-time replay starts. Markers arrive in stream order, so unlike
    /// xterm.js no write-completion callback is needed to know when it ends.
    pub fn replay_start(&mut self) {
        self.gate.replaying.store(true, Ordering::Release);
    }

    pub fn replay_end(&mut self) {
        self.gate.replaying.store(false, Ordering::Release);
    }

    /// Parse PTY output. Returns what the UI should act on.
    ///
    /// A viewport at the bottom stays there; one the user scrolled up stays on the same
    /// lines (alacritty anchors it) unless the terminal is pinned.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Notice> {
        {
            let mut term = self.term.lock();
            self.parser.advance(&mut *term, bytes);
            if self.gate.is_pinned() && term.grid().display_offset() != 0 {
                term.scroll_display(Scroll::Bottom);
            }
        }
        self.drain_events()
    }

    /// When a synchronized update (DEC mode 2026) is buffering output, the moment it must
    /// be flushed even if the program never ends it.
    pub fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    /// Flush a synchronized update whose deadline passed.
    pub fn flush_sync(&mut self) -> Vec<Notice> {
        {
            let mut term = self.term.lock();
            self.parser.stop_sync(&mut *term);
        }
        self.drain_events()
    }

    /// Apply the size the daemon echoed. This is the only way the grid changes size.
    ///
    /// A viewport at the bottom (or pinned) stays at the bottom; otherwise it keeps its
    /// relative position in the scrollback, as the Electron UI does.
    pub fn resize(&mut self, size: GridSize) {
        let mut term = self.term.lock();
        let stay_at_bottom = self.gate.is_pinned() || term.grid().display_offset() == 0;
        let ratio = view::scroll_ratio(&*term);
        term.resize(size);
        if stay_at_bottom {
            term.scroll_display(Scroll::Bottom);
        } else {
            view::scroll_to_ratio(&mut *term, ratio);
        }
    }

    pub fn size(&self) -> GridSize {
        let term = self.term.lock();
        GridSize {
            cols: u16::try_from(term.columns()).unwrap_or(u16::MAX),
            rows: u16::try_from(term.screen_lines()).unwrap_or(u16::MAX),
        }
    }

    fn may_answer(&self) -> bool {
        !self.gate.is_replaying() && self.gate.is_focused()
    }

    fn answer(&self, text: String) {
        if !self.may_answer() {
            return;
        }
        if let Some(respond) = &self.respond {
            respond(text.into_bytes());
        }
    }

    fn drain_events(&mut self) -> Vec<Notice> {
        let events = std::mem::take(&mut *self.pending.lock().expect("listener queue poisoned"));
        let mut notices = Vec::new();
        for event in events {
            match event {
                Event::PtyWrite(text) => self.answer(text),
                Event::ColorRequest(index, format) => {
                    let color = {
                        let term = self.term.lock();
                        self.palette.indexed(term.colors(), index)
                    };
                    self.answer(format(color));
                }
                Event::TextAreaSizeRequest(format) => {
                    let size = self.size();
                    let window = WindowSize {
                        num_lines: size.rows,
                        num_cols: size.cols,
                        cell_width: self.gate.cell_width.load(Ordering::Acquire),
                        cell_height: self.gate.cell_height.load(Ordering::Acquire),
                    };
                    self.answer(format(window));
                }
                Event::ClipboardStore(ClipboardType::Clipboard, text) => {
                    // A replayed copy already happened when it first ran.
                    if !self.gate.is_replaying()
                        && self.gate.allow_osc52_clipboard.load(Ordering::Acquire)
                        && text.len() <= MAX_OSC52_BYTES
                    {
                        notices.push(Notice::Clipboard(text));
                    }
                }
                // Only the system clipboard is supported, and reads are never served: a
                // program must not be able to read what the user copied elsewhere.
                Event::ClipboardStore(ClipboardType::Selection, _) | Event::ClipboardLoad(..) => {}
                Event::Title(title) => notices.push(Notice::Title(Some(title))),
                Event::ResetTitle => notices.push(Notice::Title(None)),
                Event::Bell => {
                    if !self.gate.is_replaying() {
                        notices.push(Notice::Bell);
                    }
                }
                // The PTY belongs to the daemon; its exit arrives on the stream.
                Event::Exit | Event::ChildExit(_) => {}
                Event::Wakeup | Event::MouseCursorDirty | Event::CursorBlinkingChange => {}
            }
        }
        notices
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::palette::DEFAULT_BACKGROUND;
    use crate::view::screen_text;
    use std::sync::mpsc;

    fn settings() -> EmulatorSettings {
        EmulatorSettings {
            cursor: CursorShapeSetting::Block,
            cursor_blink: false,
            allow_osc52_clipboard: true,
            scrollback: 1000,
        }
    }

    fn core_with_answers() -> (TermCore, mpsc::Receiver<Vec<u8>>) {
        let (tx, rx) = mpsc::channel();
        let respond: Responder = Box::new(move |bytes| {
            tx.send(bytes).unwrap();
        });
        let core = TermCore::new(
            GridSize { cols: 20, rows: 4 },
            settings(),
            Palette::new(DEFAULT_BACKGROUND),
            Some(respond),
        );
        (core, rx)
    }

    #[test]
    fn output_lands_in_the_grid() {
        let (mut core, _) = core_with_answers();
        core.feed(b"hello\r\nworld");
        assert_eq!(screen_text(&core.term().lock()), "hello\nworld\n\n");
    }

    #[test]
    fn queries_are_answered_only_when_focused_and_not_replaying() {
        let (mut core, answers) = core_with_answers();
        // Primary device attributes.
        core.feed(b"\x1b[c");
        assert!(answers.try_recv().is_err(), "unfocused core answered");

        core.gate().set_focused(true);
        core.replay_start();
        core.feed(b"\x1b[c");
        assert!(answers.try_recv().is_err(), "replayed query answered");

        core.replay_end();
        core.feed(b"\x1b[c");
        let answer = answers.try_recv().expect("focused live query answered");
        assert!(answer.starts_with(b"\x1b[?"), "{answer:?}");
    }

    #[test]
    fn a_core_without_a_responder_never_writes() {
        let mut core = TermCore::new(
            GridSize { cols: 20, rows: 4 },
            settings(),
            Palette::new(DEFAULT_BACKGROUND),
            None,
        );
        core.gate().set_focused(true);
        core.feed(b"\x1b[c\x1b]11;?\x07");
    }

    #[test]
    fn background_colour_queries_report_the_theme() {
        let (mut core, answers) = core_with_answers();
        core.gate().set_focused(true);
        core.feed(b"\x1b]11;?\x07");
        let answer = String::from_utf8(answers.try_recv().unwrap()).unwrap();
        assert!(answer.contains("rgb:1e1e/1e1e/1e1e"), "{answer}");
    }

    #[test]
    fn osc52_copies_respect_the_setting_and_replay() {
        let (mut core, _) = core_with_answers();
        // "hi" in base64.
        assert_eq!(
            core.feed(b"\x1b]52;c;aGk=\x07"),
            vec![Notice::Clipboard("hi".to_owned())]
        );
        core.replay_start();
        assert!(core.feed(b"\x1b]52;c;aGk=\x07").is_empty());
        core.replay_end();

        let mut disabled = TermCore::new(
            GridSize { cols: 20, rows: 4 },
            EmulatorSettings {
                allow_osc52_clipboard: false,
                ..settings()
            },
            Palette::new(DEFAULT_BACKGROUND),
            None,
        );
        assert!(disabled.feed(b"\x1b]52;c;aGk=\x07").is_empty());
    }

    #[test]
    fn titles_and_bells_are_reported() {
        let (mut core, _) = core_with_answers();
        assert_eq!(
            core.feed(b"\x1b]2;my title\x07\x07"),
            vec![Notice::Title(Some("my title".to_owned())), Notice::Bell]
        );
    }

    #[test]
    fn resize_changes_the_grid() {
        let (mut core, _) = core_with_answers();
        core.resize(GridSize { cols: 40, rows: 10 });
        assert_eq!(core.size(), GridSize { cols: 40, rows: 10 });
    }

    #[test]
    fn a_pinned_terminal_follows_output_even_when_scrolled() {
        let (mut core, _) = core_with_answers();
        core.feed(b"1\r\n2\r\n3\r\n4\r\n5\r\n6");
        core.term().lock().scroll_display(Scroll::Top);
        core.feed(b"\r\n7");
        assert_ne!(core.term().lock().grid().display_offset(), 0, "unpinned stays put");

        core.gate().set_pinned(true);
        core.feed(b"\r\n8");
        assert_eq!(core.term().lock().grid().display_offset(), 0);
    }

    #[test]
    fn resize_keeps_a_scrolled_viewport_at_its_relative_position() {
        let (mut core, _) = core_with_answers();
        for i in 0..40 {
            core.feed(format!("{i}\r\n").as_bytes());
        }
        core.term().lock().scroll_display(Scroll::Top);
        core.resize(GridSize { cols: 20, rows: 8 });
        assert_eq!(view::scroll_position(&*core.term().lock()), view::ScrollPosition::Top);

        core.term().lock().scroll_display(Scroll::Bottom);
        core.resize(GridSize { cols: 20, rows: 4 });
        assert_eq!(view::scroll_position(&*core.term().lock()), view::ScrollPosition::Bottom);
    }

    #[test]
    fn synchronized_updates_buffer_until_flushed() {
        let (mut core, _) = core_with_answers();
        core.feed(b"\x1b[?2026hpending");
        assert!(core.sync_deadline().is_some());
        assert_eq!(screen_text(&core.term().lock()), "\n\n\n");
        core.flush_sync();
        assert!(core.sync_deadline().is_none());
        assert!(screen_text(&core.term().lock()).starts_with("pending"));
    }
}
