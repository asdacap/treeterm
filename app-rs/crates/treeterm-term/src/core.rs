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
use alacritty_terminal::term::{Config, Osc52, Term};
use alacritty_terminal::vte;
use alacritty_terminal::vte::ansi::{CursorShape, CursorStyle, Processor};
use base64::Engine as _;
use base64::alphabet;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use crate::palette::Palette;
use crate::view;

/// Scrollback kept per terminal; the Electron app keeps the same.
pub const SCROLLBACK_LINES: usize = 50_000;

/// Largest clipboard write a program may make through OSC 52.
pub const MAX_OSC52_BYTES: usize = 1024 * 1024;

/// The characters that end a double-clicked word: xterm.js's default `wordSeparator`, so
/// `src/main.rs:42`, `a|b`, `<tag>` and URLs select whole, as in Electron. Plus a tab:
/// alacritty marks the first cell a tab skips with `'\t'`, where xterm.js leaves empty
/// cells, which count as separators there (as alacritty's blank `' '` cells do here).
pub const XTERM_WORD_SEPARATORS: &str = " ()[]{}',\"`\t";

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
            // TermCore decodes OSC 52 copies itself ([`decode_osc52`]): alacritty's decoding
            // is stricter than Electron's, and reads are never served.
            osc52: Osc52::Disabled,
            semantic_escape_chars: XTERM_WORD_SEPARATORS.to_owned(),
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

/// Base64 as `atob` reads it once its padding is checked: trailing bits are ignored.
const ATOB_BASE64: GeneralPurpose = GeneralPurpose::new(
    &alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::RequireNone)
        .with_decode_allow_trailing_bits(true),
);

/// The text an OSC 52 write copies, decoded as the Electron app's `decodeOsc52Clipboard`
/// does. `params` are the sequence's parameters after `52`: the selection `Pc` and the
/// base64 data `Pd`.
///
/// `None` copies nothing: another selection, a read (`?`, never served), an empty payload,
/// extra parameters, data that `atob` rejects, more than [`MAX_OSC52_BYTES`] or text that is
/// not UTF-8. Padding is optional and trailing bits are ignored, as `atob` does.
pub fn decode_osc52(params: &[&[u8]]) -> Option<String> {
    let [selection, data] = params else {
        return None;
    };
    // The clipboard. Unlike Electron, which wants exactly `c`, an empty selection means the
    // clipboard and only its first letter counts, as alacritty reads it.
    if !matches!(selection.first(), None | Some(b'c')) {
        return None;
    }
    if data.is_empty() || *data == b"?" || data.len() > MAX_OSC52_BYTES.div_ceil(3) * 4 {
        return None;
    }
    // `^[A-Za-z0-9+/]*={0,2}$`, then `atob`: padding counts only where it completes a
    // quad, and a lone symbol in the last quad is no byte.
    let symbols = data
        .strip_suffix(b"==")
        .or_else(|| data.strip_suffix(b"="))
        .unwrap_or(data);
    let padded = symbols.len() != data.len();
    if !symbols
        .iter()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'+' | b'/'))
        || (padded && !data.len().is_multiple_of(4))
    {
        return None;
    }
    let bytes = match ATOB_BASE64.decode(symbols) {
        Ok(bytes) => bytes,
        Err(err) => {
            tracing::debug!("OSC 52 copy ignored, its data is not base64: {err}");
            return None;
        }
    };
    if bytes.len() > MAX_OSC52_BYTES {
        return None;
    }
    match String::from_utf8(bytes) {
        Ok(mut text) => {
            // `TextDecoder` drops a byte order mark.
            if text.starts_with('\u{feff}') {
                text.replace_range(..'\u{feff}'.len_utf8(), "");
            }
            Some(text)
        }
        Err(err) => {
            tracing::debug!("OSC 52 copy ignored, its text is not UTF-8: {err}");
            None
        }
    }
}

/// Finds the OSC 52 copies in program output: a parser of its own, beside alacritty's,
/// because alacritty hands on only part of the sequence.
struct Osc52Scanner<'a>(&'a mut Vec<String>);

impl vte::Perform for Osc52Scanner<'_> {
    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        if let [b"52", rest @ ..] = params
            && let Some(text) = decode_osc52(rest)
        {
            self.0.push(text);
        }
    }
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
    /// Sees the same stream as `parser`, for [`Osc52Scanner`].
    osc52: vte::Parser,
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
            osc52: vte::Parser::new(),
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
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Notice> {
        self.apply_output(|parser, term| parser.advance(term, bytes));
        let mut notices = self.drain_events();
        let mut copies = Vec::new();
        self.osc52.advance(&mut Osc52Scanner(&mut copies), bytes);
        // A replayed copy already happened when it first ran.
        if !self.gate.is_replaying() && self.gate.allow_osc52_clipboard.load(Ordering::Acquire) {
            notices.extend(copies.into_iter().map(Notice::Clipboard));
        }
        notices
    }

    /// Put output on the grid. A pinned terminal, or a viewport at the bottom (within one
    /// line, see [`view::scroll_position`]), shows the newest line afterwards; one the user
    /// scrolled further up stays on the same lines (alacritty anchors it).
    fn apply_output(&mut self, apply: impl FnOnce(&mut Processor, &mut Term<Listener>)) {
        let mut term = self.term.lock();
        let follow = self.gate.is_pinned()
            || view::scroll_position(&*term) == view::ScrollPosition::Bottom;
        apply(&mut self.parser, &mut term);
        if follow && term.grid().display_offset() != 0 {
            term.scroll_display(Scroll::Bottom);
        }
    }

    /// When a synchronized update (DEC mode 2026) is buffering output, the moment it must
    /// be flushed even if the program never ends it.
    pub fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    /// Flush a synchronized update whose deadline passed.
    pub fn flush_sync(&mut self) -> Vec<Notice> {
        self.apply_output(|parser, term| parser.stop_sync(term));
        self.drain_events()
    }

    /// Apply the size the daemon echoed. This is the only way the grid changes size.
    ///
    /// A viewport at the bottom (within one line) or pinned stays at the bottom; otherwise
    /// it keeps its relative position in the scrollback, as the Electron UI does.
    pub fn resize(&mut self, size: GridSize) {
        let mut term = self.term.lock();
        let stay_at_bottom = self.gate.is_pinned()
            || view::scroll_position(&*term) == view::ScrollPosition::Bottom;
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
                // alacritty's OSC 52 is off: `feed` finds the copies, and reads are never
                // served (a program must not read what the user copied elsewhere).
                Event::ClipboardStore(..) | Event::ClipboardLoad(..) => {}
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

        // Turned off while the terminal runs.
        let (mut core, _) = core_with_answers();
        apply_settings(
            core.term(),
            core.gate(),
            EmulatorSettings {
                allow_osc52_clipboard: false,
                ..settings()
            },
        );
        let notices = core.feed(b"\x1b]52;c;aGk=\x07");
        assert!(
            !notices
                .iter()
                .any(|notice| matches!(notice, Notice::Clipboard(_))),
            "{notices:?}"
        );
    }

    fn copies(output: &[u8]) -> Vec<Notice> {
        let (mut core, _) = core_with_answers();
        core.feed(output)
    }

    fn copied(text: &str) -> Vec<Notice> {
        vec![Notice::Clipboard(text.to_owned())]
    }

    #[test]
    fn osc52_copies_decode_base64_as_atob_does() {
        assert_eq!(copies(b"\x1b]52;c;aGk\x07"), copied("hi"), "unpadded");
        assert_eq!(copies(b"\x1b]52;c;aGl=\x07"), copied("hi"), "trailing bits");
        assert_eq!(copies(b"\x1b]52;c;aGk=\x1b\\"), copied("hi"), "ended by ST");
        assert_eq!(
            copies(b"\x1b]52;;aGk=\x07"),
            copied("hi"),
            "empty selection"
        );
    }

    #[test]
    fn osc52_sequences_electron_ignores_copy_nothing() {
        for output in [
            &b"\x1b]52;c;\x07"[..],
            b"\x1b]52;c;aGk=;x\x07",
            b"\x1b]52;p;aGk=\x07",
            b"\x1b]52;c;?\x07",
            b"\x1b]52;c;a\x07",
            b"\x1b]52;c;/w==\x07",
            b"\x1b]52;aGk=\x07",
        ] {
            assert!(copies(output).is_empty(), "{}", output.escape_ascii());
        }
    }

    #[test]
    fn an_osc52_copy_split_across_reads_is_found() {
        let (mut core, _) = core_with_answers();
        assert!(core.feed(b"out\x1b]52;c;a").is_empty());
        assert_eq!(core.feed(b"Gk=\x07put"), copied("hi"));
        assert!(screen_text(&core.term().lock()).starts_with("output"));
    }

    #[test]
    fn an_osc52_copy_in_a_synchronized_update_is_found() {
        let (mut core, _) = core_with_answers();
        assert_eq!(core.feed(b"\x1b[?2026h\x1b]52;c;aGk=\x07"), copied("hi"));
        assert!(core.flush_sync().is_empty(), "copied once");
    }

    #[test]
    fn osc52_copies_come_with_the_other_notices() {
        assert_eq!(
            copies(b"\x1b]2;t\x07\x1b]52;c;aGk=\x07"),
            vec![
                Notice::Title(Some("t".to_owned())),
                Notice::Clipboard("hi".to_owned())
            ]
        );
    }

    fn decoded(selection: &str, data: &[u8]) -> Option<String> {
        decode_osc52(&[selection.as_bytes(), data])
    }

    #[test]
    fn decode_osc52_reads_the_clipboard_selection_only() {
        assert_eq!(decoded("c", b"aGk="), Some("hi".to_owned()));
        // Lenient, as alacritty: empty means the clipboard, and the first letter counts.
        assert_eq!(decoded("", b"aGk="), Some("hi".to_owned()));
        assert_eq!(decoded("cs", b"aGk="), Some("hi".to_owned()));
        assert_eq!(decoded("p", b"aGk="), None);
        assert_eq!(decoded("s", b"aGk="), None);
        assert_eq!(decoded("0", b"aGk="), None);
    }

    #[test]
    fn decode_osc52_wants_a_selection_and_data_only() {
        assert_eq!(decode_osc52(&[b"c", b"aGk=", b"x"]), None);
        assert_eq!(decode_osc52(&[b"c", b"aGk=", b""]), None);
        assert_eq!(decode_osc52(&[b"aGk="]), None);
        assert_eq!(decode_osc52(&[]), None);
    }

    #[test]
    fn decode_osc52_ignores_reads_and_empty_payloads() {
        assert_eq!(decoded("c", b"?"), None);
        assert_eq!(decoded("c", b""), None);
    }

    #[test]
    fn decode_osc52_takes_padding_as_atob_does() {
        assert_eq!(decoded("c", b"aGk"), Some("hi".to_owned()));
        assert_eq!(decoded("c", b"aA=="), Some("h".to_owned()));
        assert_eq!(decoded("c", b"aA"), Some("h".to_owned()));
        assert_eq!(decoded("c", b"aGVsbG8="), Some("hello".to_owned()));
        assert_eq!(decoded("c", b"aGVsbG8"), Some("hello".to_owned()));
        // Padding that does not complete a quad, or too much of it.
        assert_eq!(decoded("c", b"aA="), None);
        assert_eq!(decoded("c", b"aGk=="), None);
        assert_eq!(decoded("c", b"aGk==="), None);
        assert_eq!(decoded("c", b"aGVs=="), None);
        assert_eq!(decoded("c", b"=="), None);
        assert_eq!(decoded("c", b"="), None);
        // A lone symbol in the last quad.
        assert_eq!(decoded("c", b"a"), None);
        assert_eq!(decoded("c", b"aGVsb"), None);
    }

    #[test]
    fn decode_osc52_ignores_trailing_bits() {
        assert_eq!(decoded("c", b"aGl="), Some("hi".to_owned()));
        assert_eq!(decoded("c", b"aGl"), Some("hi".to_owned()));
        assert_eq!(decoded("c", b"aB=="), Some("h".to_owned()));
    }

    #[test]
    fn decode_osc52_wants_base64_symbols_only() {
        assert_eq!(decoded("c", b"aG k"), None);
        assert_eq!(decoded("c", b"aGk-"), None);
        assert_eq!(decoded("c", b"aG_k"), None);
        assert_eq!(decoded("c", b"aG=k"), None);
        assert_eq!(decoded("c", b"aGk=aGk="), None);
        assert_eq!(decoded("c", "aGké".as_bytes()), None);
        assert_eq!(decoded("c", b"Pz8/"), Some("???".to_owned()));
        assert_eq!(decoded("c", b"+/+/"), None, "decodes, but not to UTF-8");
    }

    #[test]
    fn decode_osc52_wants_utf8_and_drops_a_byte_order_mark() {
        assert_eq!(decoded("c", b"/w=="), None);
        assert_eq!(decoded("c", b"w6k="), Some("é".to_owned()));
        assert_eq!(decoded("c", b"77u/aGk="), Some("hi".to_owned()));
        assert_eq!(
            decoded("c", b"77u/"),
            Some(String::new()),
            "a copied mark copies nothing"
        );
        assert_eq!(
            decoded("c", b"77u/77u/"),
            Some("\u{feff}".to_owned()),
            "only the first"
        );
    }

    #[test]
    fn decode_osc52_caps_the_copy_at_a_mebibyte() {
        use base64::engine::general_purpose::STANDARD;
        let encoded = |len: usize| STANDARD.encode("a".repeat(len));
        assert_eq!(
            decoded("c", encoded(MAX_OSC52_BYTES).as_bytes()).map(|text| text.len()),
            Some(MAX_OSC52_BYTES)
        );
        // As long encoded as the largest copy, but one byte more.
        let over = encoded(MAX_OSC52_BYTES + 1);
        assert_eq!(over.len(), encoded(MAX_OSC52_BYTES).len());
        assert_eq!(decoded("c", over.as_bytes()), None);
        // Too long to decode at all.
        assert_eq!(decoded("c", encoded(MAX_OSC52_BYTES + 3).as_bytes()), None);
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

    /// Lines 0..40 fed, viewport scrolled `lines` up from the bottom (0: at the bottom).
    fn scrolled_up(lines: i32) -> TermCore {
        let (mut core, _) = core_with_answers();
        for i in 0..40 {
            core.feed(format!("{i}\r\n").as_bytes());
        }
        core.term().lock().scroll_display(Scroll::Delta(lines));
        core
    }

    #[test]
    fn output_follows_from_one_line_up() {
        let mut core = scrolled_up(1);
        core.feed(b"new\r\n");
        assert_eq!(core.term().lock().grid().display_offset(), 0);
    }

    #[test]
    fn output_leaves_two_lines_up_anchored() {
        let mut core = scrolled_up(2);
        core.feed(b"new\r\n");
        assert_eq!(core.term().lock().grid().display_offset(), 3);
    }

    #[test]
    fn a_synchronized_update_flushed_at_its_deadline_follows_from_the_bottom() {
        for (scrolled, offset) in [(1, 0), (2, 4)] {
            let mut core = scrolled_up(0);
            core.feed(b"\x1b[?2026hnew\r\nlines\r\n");
            core.term().lock().scroll_display(Scroll::Delta(scrolled));
            core.flush_sync();
            assert_eq!(core.term().lock().grid().display_offset(), offset, "{scrolled} up");
        }
    }

    #[test]
    fn resize_stays_at_the_bottom_from_one_line_up() {
        let mut core = scrolled_up(1);
        core.resize(GridSize { cols: 20, rows: 8 });
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

    /// The text a double-click at `column` of the first row of `text` selects.
    fn double_clicked(text: &str, column: usize) -> String {
        use alacritty_terminal::index::{Column, Line, Point, Side};
        use alacritty_terminal::selection::{Selection, SelectionType};
        let (mut core, _) = core_with_answers();
        core.resize(GridSize { cols: 40, rows: 2 });
        core.feed(text.as_bytes());
        let mut term = core.term().lock();
        let at = Point::new(Line(0), Column(column));
        term.selection = Some(Selection::new(SelectionType::Semantic, at, Side::Left));
        term.selection_to_string().unwrap()
    }

    #[test]
    fn a_double_click_selects_a_word_bounded_by_xterm_js_separators() {
        assert_eq!(double_clicked("see src/main.rs:42 now", 6), "src/main.rs:42");
        assert_eq!(double_clicked("open https://x.io/a?b=1 now", 8), "https://x.io/a?b=1");
        assert_eq!(double_clicked("a<b>c|d e", 2), "a<b>c|d");
        assert_eq!(double_clicked("call f(x) now", 5), "f", "stops at the paren");
        assert_eq!(double_clicked("say 'yo',\"hi\"", 5), "yo");
        assert_eq!(double_clicked("4.0K\tfile", 1), "4.0K", "a tab ends a word, as in `du` output");
        assert_eq!(double_clicked("abcdefg\tx", 0), "abcdefg", "a one-cell tab gap too");
        assert_eq!(double_clicked("abcdefg\tx", 8), "x");
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
