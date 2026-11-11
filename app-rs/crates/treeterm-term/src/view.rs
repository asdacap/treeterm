//! Read-only questions about a terminal's buffer and viewport.

use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::Term;
use alacritty_terminal::term::cell::Flags;

/// Where the viewport sits in the scrollback. Mirrors `ScrollPosition` in the Electron UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollPosition {
    Top,
    Middle,
    Bottom,
}

impl ScrollPosition {
    /// The badge text the Electron UI shows.
    pub fn label(self) -> &'static str {
        match self {
            ScrollPosition::Top => "TOP",
            ScrollPosition::Middle => "MIDDLE",
            ScrollPosition::Bottom => "BOTTOM",
        }
    }
}

/// Text of one grid line with trailing whitespace removed, like xterm.js's
/// `translateToString(true)`.
pub fn line_text<T>(term: &Term<T>, line: Line) -> String {
    let row = &term.grid()[line];
    let mut text = String::with_capacity(term.columns());
    for column in 0..term.columns() {
        let cell = &row[Column(column)];
        if cell
            .flags
            .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
        {
            continue;
        }
        text.push(cell.c);
        if let Some(zerowidth) = cell.zerowidth() {
            text.extend(zerowidth.iter());
        }
    }
    let trimmed = text.trim_end_matches([' ', '\t', '\0']).len();
    text.truncate(trimmed);
    text
}

/// The screen's text, top to bottom, ignoring where the user scrolled.
///
/// Used for activity detection (port of `snapshotViewport` in `terminal/engine.ts`):
/// comparing consecutive snapshots tells whether the screen actually changed, so a program
/// that repaints identical content reads as idle. It tracks the program's newest output
/// whatever the user scrolled to, and colour- or cursor-only repaints count as no change.
pub fn snapshot_viewport<T>(term: &Term<T>) -> String {
    screen_text(term)
}

/// The screen's lines joined with `\n`.
pub fn screen_text<T>(term: &Term<T>) -> String {
    (0..term.screen_lines())
        .map(|line| line_text(term, Line(line as i32)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Every line of the buffer, oldest scrollback first. For automation and tests.
pub fn buffer_lines<T>(term: &Term<T>) -> Vec<String> {
    let top = term.topmost_line().0;
    let bottom = term.bottommost_line().0;
    (top..=bottom).map(|line| line_text(term, Line(line))).collect()
}

pub fn scroll_position<T>(term: &Term<T>) -> ScrollPosition {
    let offset = term.grid().display_offset();
    if offset == 0 {
        ScrollPosition::Bottom
    } else if offset >= term.history_size() {
        ScrollPosition::Top
    } else {
        ScrollPosition::Middle
    }
}

/// 0 = the oldest line is at the top, 1 = the newest output is in view.
pub fn scroll_ratio<T>(term: &Term<T>) -> f64 {
    let history = term.history_size();
    if history == 0 {
        return 1.0;
    }
    1.0 - term.grid().display_offset() as f64 / history as f64
}

pub fn scroll_to_ratio<T: alacritty_terminal::event::EventListener>(
    term: &mut Term<T>,
    ratio: f64,
) {
    let history = term.history_size();
    let target = ((1.0 - ratio.clamp(0.0, 1.0)) * history as f64).round() as usize;
    let current = term.grid().display_offset();
    let delta = target as i32 - current as i32;
    if delta != 0 {
        term.scroll_display(Scroll::Delta(delta));
    }
}

pub fn is_alternate_screen<T>(term: &Term<T>) -> bool {
    term.mode()
        .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{CursorShapeSetting, EmulatorSettings, GridSize, TermCore};
    use crate::palette::{DEFAULT_BACKGROUND, Palette};

    fn core(rows: u16) -> TermCore {
        TermCore::new(
            GridSize { cols: 10, rows },
            EmulatorSettings {
                cursor: CursorShapeSetting::Block,
                cursor_blink: false,
                allow_osc52_clipboard: false,
                scrollback: 100,
            },
            Palette::new(DEFAULT_BACKGROUND),
            None,
        )
    }

    #[test]
    fn snapshot_is_the_screen_with_trailing_space_trimmed() {
        let mut core = core(3);
        core.feed(b"a  \r\n\x1b[31mb\x1b[0m");
        assert_eq!(snapshot_viewport(&core.term().lock()), "a\nb\n");
    }

    #[test]
    fn wide_characters_appear_once() {
        let mut core = core(2);
        core.feed("日本".as_bytes());
        assert_eq!(line_text(&core.term().lock(), Line(0)), "日本");
    }

    #[test]
    fn snapshot_ignores_the_users_scroll() {
        let mut core = core(2);
        core.feed(b"1\r\n2\r\n3\r\n4");
        let mut term = core.term().lock();
        term.scroll_display(Scroll::Top);
        assert_eq!(snapshot_viewport(&term), "3\n4");
    }

    #[test]
    fn scroll_position_and_ratio_follow_the_viewport() {
        let mut core = core(2);
        assert_eq!(scroll_position(&core.term().lock()), ScrollPosition::Bottom);
        core.feed(b"1\r\n2\r\n3\r\n4\r\n5");
        let mut term = core.term().lock();
        assert_eq!(scroll_position(&term), ScrollPosition::Bottom);
        assert_eq!(scroll_ratio(&term), 1.0);

        term.scroll_display(Scroll::Top);
        assert_eq!(scroll_position(&term), ScrollPosition::Top);
        assert_eq!(scroll_ratio(&term), 0.0);

        scroll_to_ratio(&mut term, 0.5);
        assert_eq!(scroll_position(&term), ScrollPosition::Middle);
        scroll_to_ratio(&mut term, 1.0);
        assert_eq!(scroll_position(&term), ScrollPosition::Bottom);
    }

    #[test]
    fn buffer_lines_include_scrollback() {
        let mut core = core(2);
        core.feed(b"1\r\n2\r\n3");
        assert_eq!(buffer_lines(&core.term().lock()), vec!["1", "2", "3"]);
    }

    #[test]
    fn alternate_screen_is_detected() {
        let mut core = core(2);
        core.feed(b"\x1b[?1049h");
        assert!(is_alternate_screen(&core.term().lock()));
        core.feed(b"\x1b[?1049l");
        assert!(!is_alternate_screen(&core.term().lock()));
    }
}
