//! Alt/Option+click moves the program's cursor to the clicked cell with arrow keys: a port of
//! xterm.js's `MoveToCell.ts` (`altClickMovesCursor`), quirks included, so a click sends the
//! keys Electron sends.
//!
//! On the normal screen the cursor moves only left or right, by the cells between it and the
//! click in reading order (a shell's line editor wraps between rows). On the alternate screen
//! it first goes back to the start of a wrapped row, then up or down skipping wrapped rows,
//! then left or right by the characters it passes, counted in UTF-16 code units. As in
//! xterm.js, a click there left of the cursor on the row it ends on sends no horizontal keys,
//! and one on a lower row goes right.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{Term, TermMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Up,
    Down,
    Right,
    Left,
}

/// The arrow keys that take the cursor to `column` of screen row `line`. Only meaningful with
/// the viewport at the bottom, where xterm.js alone moves the cursor.
pub fn move_to_cell<T>(term: &Term<T>, column: usize, line: usize) -> Vec<u8> {
    let screen = Screen { term };
    let cursor = term.grid().cursor.point;
    let (start_x, start_y) = (cursor.column.0 as i64, i64::from(cursor.line.0));
    let (target_x, target_y) = (column as i64, line as i64);

    // The alternate screen navigates between rows.
    if term.mode().contains(TermMode::ALT_SCREEN) {
        let mut keys = screen.reset_starting_row(start_x, start_y, target_y);
        keys.extend(screen.move_to_requested_row(start_y, target_y));
        keys.extend(screen.move_to_requested_col(start_x, start_y, target_x, target_y));
        return keys;
    }

    // The normal screen only moves horizontally.
    if start_y == target_y {
        let direction = if start_x > target_x { Direction::Left } else { Direction::Right };
        return screen.repeat((start_x - target_x).abs(), direction);
    }
    let up = start_y > target_y;
    let direction = if up { Direction::Left } else { Direction::Right };
    let row_difference = (start_y - target_y).abs();
    let cells = screen.cols_from_row_end(if up { target_x } else { start_x })
        + (row_difference - 1) * screen.cols()
        + 1 // wrap around one row
        + cols_from_row_beginning(if up { start_x } else { target_x });
    screen.repeat(cells, direction)
}

fn cols_from_row_beginning(x: i64) -> i64 {
    x - 1
}

fn vertical_direction(start_y: i64, target_y: i64) -> Direction {
    if start_y > target_y { Direction::Up } else { Direction::Down }
}

/// xterm.js's buffer as `MoveToCell.ts` reads it: rows are screen rows, and the alternate
/// screen has no scrollback, so its buffer is the screen.
struct Screen<'a, T> {
    term: &'a Term<T>,
}

impl<T> Screen<'_, T> {
    fn cols(&self) -> i64 {
        self.term.columns() as i64
    }

    fn rows(&self) -> i64 {
        self.term.screen_lines() as i64
    }

    fn cols_from_row_end(&self, x: i64) -> i64 {
        self.cols() - x
    }

    fn repeat(&self, count: i64, direction: Direction) -> Vec<u8> {
        let application_cursor = self.term.mode().contains(TermMode::APP_CURSOR);
        let key = [
            0x1b,
            if application_cursor { b'O' } else { b'[' },
            match direction {
                Direction::Up => b'A',
                Direction::Down => b'B',
                Direction::Right => b'C',
                Direction::Left => b'D',
            },
        ];
        (0..count.max(0)).flat_map(|_| key).collect()
    }

    /// xterm.js's `isWrapped`: the row continues the one above it.
    fn is_wrapped(&self, row: i64) -> bool {
        (1..self.rows()).contains(&row)
            && self.term.grid()[Line(row as i32 - 1)][Column(self.term.columns() - 1)]
                .flags
                .contains(Flags::WRAPLINE)
    }

    /// The number of wrapped rows above `row` that make up its line.
    fn wrapped_rows_for_row(&self, mut row: i64) -> i64 {
        let mut count = 0;
        let mut wraps = self.is_wrapped(row);
        while wraps && row >= 0 && row < self.rows() {
            count += 1;
            row -= 1;
            wraps = self.is_wrapped(row);
        }
        count
    }

    /// The wrapped rows between the unwrapped starting and ending rows, which the cursor
    /// skips over.
    fn wrapped_rows_count(&self, start_y: i64, target_y: i64) -> i64 {
        let start_row = start_y - self.wrapped_rows_for_row(start_y);
        let end_row = target_y - self.wrapped_rows_for_row(target_y);
        let step = match vertical_direction(start_y, target_y) {
            Direction::Up => -1,
            Direction::Down | Direction::Right | Direction::Left => 1,
        };
        (0..(start_row - end_row).abs())
            .filter(|i| self.is_wrapped(start_row + step * i))
            .count() as i64
    }

    /// From a wrapped row, back to the start of its line first.
    fn reset_starting_row(&self, start_x: i64, start_y: i64, target_y: i64) -> Vec<u8> {
        if self.move_to_requested_row(start_y, target_y).is_empty() {
            return Vec::new();
        }
        let first_row = start_y - self.wrapped_rows_for_row(start_y);
        self.repeat(self.buffer_line(start_x, start_y, start_x, first_row, false), Direction::Left)
    }

    fn move_to_requested_row(&self, start_y: i64, target_y: i64) -> Vec<u8> {
        let start_row = start_y - self.wrapped_rows_for_row(start_y);
        let end_row = target_y - self.wrapped_rows_for_row(target_y);
        let rows_to_move = (start_row - end_row).abs() - self.wrapped_rows_count(start_y, target_y);
        self.repeat(rows_to_move, vertical_direction(start_y, target_y))
    }

    fn move_to_requested_col(&self, start_x: i64, start_y: i64, target_x: i64, target_y: i64) -> Vec<u8> {
        let start_row = if self.move_to_requested_row(start_y, target_y).is_empty() {
            start_y
        } else {
            target_y - self.wrapped_rows_for_row(target_y)
        };
        let direction = self.horizontal_direction(start_x, start_y, target_x, target_y);
        let count = self.buffer_line(start_x, start_row, target_x, target_y, direction == Direction::Right);
        self.repeat(count, direction)
    }

    fn horizontal_direction(&self, start_x: i64, start_y: i64, target_x: i64, target_y: i64) -> Direction {
        // xterm.js passes the target column as the starting row here.
        let start_row = if self.move_to_requested_row(target_x, target_y).is_empty() {
            start_y
        } else {
            target_y - self.wrapped_rows_for_row(target_y)
        };
        if (start_x < target_x && start_row <= target_y) || (start_x >= target_x && start_row < target_y) {
            Direction::Right
        } else {
            Direction::Left
        }
    }

    /// The length of the text from one cell to another, walking forward or backward and
    /// wrapping between rows, as xterm.js's `bufferLine(...).length`.
    fn buffer_line(&self, mut start_col: i64, start_row: i64, end_col: i64, end_row: i64, forward: bool) -> i64 {
        let mut current_col = start_col;
        let mut current_row = start_row;
        let mut length = 0;
        while (current_col != end_col || current_row != end_row) && current_row >= 0 && current_row < self.rows() {
            current_col += if forward { 1 } else { -1 };
            if forward && current_col > self.cols() - 1 {
                length += self.text_length(current_row, start_col, current_col);
                current_col = 0;
                start_col = 0;
                current_row += 1;
            } else if !forward && current_col < 0 {
                length += self.text_length(current_row, 0, start_col + 1);
                current_col = self.cols() - 1;
                start_col = current_col;
                current_row -= 1;
            }
        }
        length + self.text_length(current_row, start_col, current_col)
    }

    /// xterm.js's `translateBufferLineToString(row, false, start, end).length`: a wide
    /// character counts once, and every character in UTF-16 code units.
    fn text_length(&self, row: i64, start: i64, end: i64) -> i64 {
        if !(0..self.rows()).contains(&row) {
            return 0;
        }
        let grid_row = &self.term.grid()[Line(row as i32)];
        let mut column = start;
        let mut length = 0;
        while column < end {
            if !(0..self.cols()).contains(&column) {
                // Past the row's end xterm.js reads blank cells.
                length += 1;
                column += 1;
                continue;
            }
            let cell = &grid_row[Column(column as usize)];
            let chars = std::iter::once(cell.c).chain(cell.zerowidth().into_iter().flatten().copied());
            length += chars.map(|c| c.len_utf16() as i64).sum::<i64>();
            column += if cell.flags.contains(Flags::WIDE_CHAR) { 2 } else { 1 };
        }
        length
    }
}

#[cfg(test)]
mod tests {
    //! Expected keys are what xterm.js 6.0's `moveToCellSequence` sends for the same buffer.

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    use super::*;
    use crate::core::GridSize;

    fn term(cols: u16, rows: u16, bytes: &str) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &GridSize { cols, rows }, VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, bytes.as_bytes());
        term
    }

    /// `keys` as text, ESC written `\e`.
    fn keys(term: &Term<VoidListener>, column: usize, line: usize) -> String {
        String::from_utf8(move_to_cell(term, column, line)).unwrap().replace('\x1b', "\\e")
    }

    fn arrows(arrow: &str, count: usize) -> String {
        arrow.repeat(count)
    }

    /// The normal screen with the cursor at (`column`, `line`), 10 columns by 3 rows.
    fn normal_screen(column: usize, line: usize, application_cursor: bool) -> Term<VoidListener> {
        let mode = if application_cursor { "\x1b[?1h" } else { "" };
        term(10, 3, &format!("{mode}\x1b[{};{}H", line + 1, column + 1))
    }

    /// The alternate screen showing `text`, with the cursor at (`column`, `line`).
    fn alternate_screen(cols: u16, rows: u16, text: &str, column: usize, line: usize) -> Term<VoidListener> {
        term(cols, rows, &format!("\x1b[?1049h{text}\x1b[{};{}H", line + 1, column + 1))
    }

    const FULL_ROWS: &str = "abcdefghij\r\nabcdefghij\r\nabcdefghij";

    #[test]
    fn the_normal_screen_moves_along_the_row() {
        assert_eq!(keys(&normal_screen(2, 1, false), 7, 1), arrows("\\e[C", 5));
        assert_eq!(keys(&normal_screen(7, 1, true), 2, 1), arrows("\\eOD", 5));
        assert_eq!(keys(&normal_screen(4, 1, false), 4, 1), "");
    }

    #[test]
    fn the_normal_screen_wraps_between_rows_in_reading_order() {
        assert_eq!(keys(&normal_screen(8, 0, false), 3, 1), arrows("\\e[C", 5));
        assert_eq!(keys(&normal_screen(3, 2, false), 8, 0), arrows("\\e[D", 15));
    }

    #[test]
    fn the_alternate_screen_moves_up_or_down_then_along_the_row() {
        let screen = |column, line| alternate_screen(10, 3, FULL_ROWS, column, line);
        assert_eq!(keys(&screen(2, 1), 7, 1), arrows("\\e[C", 5));
        assert_eq!(keys(&screen(2, 0), 7, 2), arrows("\\e[B", 2) + &arrows("\\e[C", 5));
        assert_eq!(keys(&screen(2, 2), 7, 0), arrows("\\e[A", 2) + &arrows("\\e[C", 5));
    }

    #[test]
    fn the_alternate_screen_keeps_xterm_js_quirks_going_left() {
        let screen = |column, line| alternate_screen(10, 3, FULL_ROWS, column, line);
        assert_eq!(keys(&screen(7, 1), 2, 1), "", "no keys left along the cursor's row");
        assert_eq!(keys(&screen(7, 0), 2, 2), arrows("\\e[B", 2) + &arrows("\\e[C", 3), "right on a lower row");
        assert_eq!(keys(&screen(7, 2), 2, 0), arrows("\\e[A", 2), "only up on a higher one");
    }

    #[test]
    fn application_cursor_mode_sends_ss3_arrows() {
        let screen = alternate_screen(10, 3, &format!("\x1b[?1h{FULL_ROWS}"), 7, 2);
        assert_eq!(keys(&screen, 2, 0), arrows("\\eOA", 2));
    }

    #[test]
    fn the_alternate_screen_skips_wrapped_rows() {
        // Row 2 continues row 1.
        let wrapped = "abcdefghij\r\nabcdefghijklmnop\r\nxyz";
        let screen = |column, line| alternate_screen(10, 4, wrapped, column, line);
        assert_eq!(keys(&screen(1, 0), 4, 2), "\\e[B".to_owned() + &arrows("\\e[C", 13));
        // From the wrapped row, back to its line's start first.
        assert_eq!(keys(&screen(3, 2), 5, 3), arrows("\\e[D", 4) + "\\e[B" + &arrows("\\e[C", 2));
        assert_eq!(keys(&screen(3, 2), 5, 0), arrows("\\e[D", 4) + "\\e[A" + &arrows("\\e[C", 2));
    }

    #[test]
    fn a_wide_character_counts_once() {
        let screen = alternate_screen(10, 2, "ab中文cd\r\n中x文yz", 0, 0);
        assert_eq!(keys(&screen, 6, 1), "\\e[B".to_owned() + &arrows("\\e[C", 4));
        let screen = alternate_screen(10, 2, "ab\r\n中x文yz", 0, 1);
        assert_eq!(keys(&screen, 6, 1), arrows("\\e[C", 4));
    }
}
