//! Mouse reports for programs that enable mouse tracking (DEC modes 1000/1002/1003, with
//! SGR 1006 or UTF-8 1005 coordinates), and wheel-to-arrow translation on the alternate
//! screen (mode 1007).

use alacritty_terminal::term::TermMode;

use crate::keys::Modifiers;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Middle,
    Right,
}

impl MouseButton {
    fn code(self) -> u8 {
        match self {
            MouseButton::Left => 0,
            MouseButton::Middle => 1,
            MouseButton::Right => 2,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseAction {
    Press(MouseButton),
    Release(MouseButton),
    /// Pointer motion, with the button held while moving (a drag), if any.
    Move(Option<MouseButton>),
    WheelUp,
    WheelDown,
}

/// A mouse event at a grid cell (0-based, relative to the top of the screen).
#[derive(Debug, Clone, Copy)]
pub struct MouseInput {
    pub action: MouseAction,
    pub column: usize,
    pub line: usize,
    pub modifiers: Modifiers,
}

/// Whether the program is tracking the mouse at all. While it is, clicks and drags belong
/// to the program, not to text selection (Shift overrides, as in xterm.js).
pub fn mouse_mode_active(mode: TermMode) -> bool {
    mode.intersects(TermMode::MOUSE_MODE)
}

/// The report for `input`, or `None` when the program did not ask for this kind of event
/// or the position cannot be encoded.
pub fn encode_mouse(input: MouseInput, mode: TermMode) -> Option<Vec<u8>> {
    if !mouse_mode_active(mode) {
        return None;
    }
    let (mut code, release) = match input.action {
        MouseAction::Press(button) => (button.code(), false),
        MouseAction::Release(button) => (button.code(), true),
        MouseAction::WheelUp => (64, false),
        MouseAction::WheelDown => (65, false),
        MouseAction::Move(held) => {
            let reported = match held {
                Some(_) => mode.intersects(TermMode::MOUSE_DRAG | TermMode::MOUSE_MOTION),
                None => mode.contains(TermMode::MOUSE_MOTION),
            };
            if !reported {
                return None;
            }
            (32 + held.map_or(3, MouseButton::code), false)
        }
    };
    let m = input.modifiers;
    code += u8::from(m.shift) * 4 + u8::from(m.alt) * 8 + u8::from(m.control) * 16;

    let column = input.column + 1;
    let line = input.line + 1;
    if mode.contains(TermMode::SGR_MOUSE) {
        let final_byte = if release { 'm' } else { 'M' };
        return Some(format!("\x1b[<{code};{column};{line}{final_byte}").into_bytes());
    }

    // The legacy encodings cannot say which button was released.
    if release {
        code = (code & !0b11) | 3;
    }
    let mut out = vec![0x1b, b'[', b'M', 32 + code];
    if mode.contains(TermMode::UTF8_MOUSE) {
        for position in [column, line] {
            let value = u32::try_from(32 + position).ok()?;
            let c = char::from_u32(value).filter(|_| value < 2048)?;
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
        }
    } else {
        for position in [column, line] {
            out.push(u8::try_from(32 + position).ok()?);
        }
    }
    Some(out)
}

/// On the alternate screen with alternate scroll on and no mouse tracking, the wheel sends
/// cursor keys so pagers and editors scroll. `lines` > 0 scrolls up (toward older text).
pub fn alternate_scroll(lines: i32, mode: TermMode) -> Option<Vec<u8>> {
    let applies = mode.contains(TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL)
        && !mouse_mode_active(mode);
    if !applies || lines == 0 {
        return None;
    }
    let final_byte = if lines > 0 { b'A' } else { b'B' };
    let prefix = if mode.contains(TermMode::APP_CURSOR) {
        b'O'
    } else {
        b'['
    };
    let mut out = Vec::new();
    for _ in 0..lines.unsigned_abs() {
        out.extend_from_slice(&[0x1b, prefix, final_byte]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(action: MouseAction, column: usize, line: usize) -> MouseInput {
        MouseInput {
            action,
            column,
            line,
            modifiers: Modifiers::default(),
        }
    }

    #[test]
    fn nothing_is_reported_without_tracking() {
        assert_eq!(
            encode_mouse(at(MouseAction::Press(MouseButton::Left), 0, 0), TermMode::empty()),
            None
        );
    }

    #[test]
    fn sgr_reports_carry_the_released_button() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        assert_eq!(
            encode_mouse(at(MouseAction::Press(MouseButton::Left), 4, 2), mode).unwrap(),
            b"\x1b[<0;5;3M"
        );
        assert_eq!(
            encode_mouse(at(MouseAction::Release(MouseButton::Right), 4, 2), mode).unwrap(),
            b"\x1b[<2;5;3m"
        );
        assert_eq!(
            encode_mouse(at(MouseAction::WheelDown, 0, 0), mode).unwrap(),
            b"\x1b[<65;1;1M"
        );
    }

    #[test]
    fn modifiers_are_added_to_the_button_code() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let mut input = at(MouseAction::Press(MouseButton::Left), 0, 0);
        input.modifiers.control = true;
        input.modifiers.shift = true;
        assert_eq!(encode_mouse(input, mode).unwrap(), b"\x1b[<20;1;1M");
    }

    #[test]
    fn motion_needs_drag_or_any_motion_mode() {
        let click = TermMode::MOUSE_REPORT_CLICK | TermMode::SGR_MOUSE;
        let drag = TermMode::MOUSE_DRAG | TermMode::SGR_MOUSE;
        let motion = TermMode::MOUSE_MOTION | TermMode::SGR_MOUSE;
        let dragging = at(MouseAction::Move(Some(MouseButton::Left)), 1, 1);
        let hovering = at(MouseAction::Move(None), 1, 1);
        assert_eq!(encode_mouse(dragging, click), None);
        assert_eq!(encode_mouse(dragging, drag).unwrap(), b"\x1b[<32;2;2M");
        assert_eq!(encode_mouse(hovering, drag), None);
        assert_eq!(encode_mouse(hovering, motion).unwrap(), b"\x1b[<35;2;2M");
    }

    #[test]
    fn legacy_reports_are_offset_bytes_and_lose_the_released_button() {
        let mode = TermMode::MOUSE_REPORT_CLICK;
        assert_eq!(
            encode_mouse(at(MouseAction::Press(MouseButton::Middle), 0, 0), mode).unwrap(),
            vec![0x1b, b'[', b'M', 33, 33, 33]
        );
        assert_eq!(
            encode_mouse(at(MouseAction::Release(MouseButton::Middle), 0, 0), mode).unwrap(),
            vec![0x1b, b'[', b'M', 35, 33, 33]
        );
        // Column 300 does not fit in a byte.
        assert_eq!(
            encode_mouse(at(MouseAction::Press(MouseButton::Left), 300, 0), mode),
            None
        );
    }

    #[test]
    fn utf8_reports_extend_the_coordinate_range() {
        let mode = TermMode::MOUSE_REPORT_CLICK | TermMode::UTF8_MOUSE;
        let report = encode_mouse(at(MouseAction::Press(MouseButton::Left), 300, 0), mode).unwrap();
        assert_eq!(&report[..4], &[0x1b, b'[', b'M', 32]);
        assert_eq!(std::str::from_utf8(&report[4..]).unwrap(), "\u{14d}!");
    }

    #[test]
    fn wheel_sends_arrows_on_the_alternate_screen() {
        let mode = TermMode::ALT_SCREEN | TermMode::ALTERNATE_SCROLL;
        assert_eq!(alternate_scroll(2, mode).unwrap(), b"\x1b[A\x1b[A");
        assert_eq!(
            alternate_scroll(-1, mode | TermMode::APP_CURSOR).unwrap(),
            b"\x1bOB"
        );
        assert_eq!(alternate_scroll(1, TermMode::ALTERNATE_SCROLL), None);
        assert_eq!(
            alternate_scroll(1, mode | TermMode::MOUSE_REPORT_CLICK),
            None
        );
    }
}
