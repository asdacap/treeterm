//! Keyboard input → bytes for the PTY, following xterm.js's `evaluateKeyboardEvent` so
//! programs see the same sequences they saw in the Electron app.
//!
//! Plain text (letters, digits, punctuation, composed characters, IME commits) is not
//! encoded here: it reaches the terminal through the platform's text input path, and
//! [`encode_key`] returns `None` for it.

use alacritty_terminal::term::TermMode;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    /// Cmd on macOS, Super elsewhere. Combinations with it belong to the app, never the PTY.
    pub platform: bool,
}

impl Modifiers {
    /// xterm's modifier parameter (`CSI 1 ; <param> X`), or `None` without modifiers.
    fn param(self) -> Option<u8> {
        let bits = u8::from(self.shift) | u8::from(self.alt) << 1 | u8::from(self.control) << 2;
        (bits != 0).then_some(bits + 1)
    }
}

/// A key press, named the way GPUI names keys (`"up"`, `"pagedown"`, `"f5"`, `"a"`, ...).
#[derive(Debug, Clone, Copy)]
pub struct KeyInput<'a> {
    pub key: &'a str,
    pub modifiers: Modifiers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KeyOptions {
    /// Whether Alt/Option combined with a character sends ESC + character. On macOS the
    /// Electron app leaves this off, so Option composes characters (Option-e, Option-u, ...)
    /// through the text input path instead.
    pub alt_sends_escape: bool,
    /// Word-wise Alt+Left/Right as on macOS (`ESC b` / `ESC f`) rather than xterm's
    /// `CSI 1;5 D/C`.
    pub mac_word_motion: bool,
}

impl KeyOptions {
    pub const fn for_host() -> Self {
        if cfg!(target_os = "macos") {
            Self {
                alt_sends_escape: false,
                mac_word_motion: true,
            }
        } else {
            Self {
                alt_sends_escape: true,
                mac_word_motion: false,
            }
        }
    }
}

const ESC: u8 = 0x1b;

/// The bytes `input` sends, or `None` when the key is not the terminal's to encode (plain
/// text, or a Cmd/Super shortcut).
pub fn encode_key(input: KeyInput<'_>, mode: TermMode, options: KeyOptions) -> Option<Vec<u8>> {
    let m = input.modifiers;
    if m.platform {
        return None;
    }
    let app_cursor = mode.contains(TermMode::APP_CURSOR);

    let csi_mod = |final_byte: u8| -> Vec<u8> {
        match m.param() {
            Some(param) => format!("\x1b[1;{param}{}", final_byte as char).into_bytes(),
            None if app_cursor => vec![ESC, b'O', final_byte],
            None => vec![ESC, b'[', final_byte],
        }
    };
    let tilde = |code: u8| -> Vec<u8> {
        match m.param() {
            Some(param) => format!("\x1b[{code};{param}~").into_bytes(),
            None => format!("\x1b[{code}~").into_bytes(),
        }
    };
    let ss3_fn = |final_byte: u8| -> Vec<u8> {
        match m.param() {
            Some(param) => format!("\x1b[1;{param}{}", final_byte as char).into_bytes(),
            None => vec![ESC, b'O', final_byte],
        }
    };
    let alt_prefixed = |bytes: &[u8]| -> Vec<u8> {
        let mut out = Vec::with_capacity(bytes.len() + 1);
        if m.alt {
            out.push(ESC);
        }
        out.extend_from_slice(bytes);
        out
    };

    let bytes = match input.key {
        "up" | "down" | "left" | "right" => {
            let final_byte = match input.key {
                "up" => b'A',
                "down" => b'B',
                "right" => b'C',
                _ => b'D',
            };
            let only_alt = m.alt && !m.control && !m.shift;
            if only_alt && matches!(final_byte, b'C' | b'D') {
                // xterm.js makes Alt+Left/Right word motion.
                if options.mac_word_motion {
                    vec![ESC, if final_byte == b'D' { b'b' } else { b'f' }]
                } else {
                    format!("\x1b[1;5{}", final_byte as char).into_bytes()
                }
            } else if only_alt && !options.mac_word_motion {
                format!("\x1b[1;5{}", final_byte as char).into_bytes()
            } else {
                csi_mod(final_byte)
            }
        }
        "home" => csi_mod(b'H'),
        "end" => csi_mod(b'F'),
        "insert" => tilde(2),
        "delete" => tilde(3),
        "pageup" => tilde(5),
        "pagedown" => tilde(6),
        "f1" => ss3_fn(b'P'),
        "f2" => ss3_fn(b'Q'),
        "f3" => ss3_fn(b'R'),
        "f4" => ss3_fn(b'S'),
        "f5" => tilde(15),
        "f6" => tilde(17),
        "f7" => tilde(18),
        "f8" => tilde(19),
        "f9" => tilde(20),
        "f10" => tilde(21),
        "f11" => tilde(23),
        "f12" => tilde(24),
        "backspace" => alt_prefixed(if m.control { b"\x08" } else { b"\x7f" }),
        "enter" => alt_prefixed(b"\r"),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => alt_prefixed(b"\t"),
        "escape" => alt_prefixed(&[ESC]),
        "space" if m.control => alt_prefixed(&[0]),
        "space" if m.alt && options.alt_sends_escape => vec![ESC, b' '],
        key if m.control => {
            let code = control_code(key)?;
            alt_prefixed(&[code])
        }
        key if m.alt && options.alt_sends_escape => {
            let mut chars = key.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let c = if m.shift { c.to_ascii_uppercase() } else { c };
            let mut out = vec![ESC];
            let mut buf = [0; 4];
            out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            out
        }
        _ => return None,
    };
    Some(bytes)
}

/// Ctrl + key → C0 control code, as xterm.js maps it.
fn control_code(key: &str) -> Option<u8> {
    let mut chars = key.chars();
    let (Some(c), None) = (chars.next(), chars.next()) else {
        return None;
    };
    let code = match c.to_ascii_lowercase() {
        c @ 'a'..='z' => c as u8 - b'a' + 1,
        '@' | '2' => 0,
        '[' | '3' => ESC,
        '\\' | '4' => 0x1c,
        ']' | '5' => 0x1d,
        '^' | '6' => 0x1e,
        '_' | '-' | '/' | '7' => 0x1f,
        '8' => 0x7f,
        _ => return None,
    };
    Some(code)
}

/// Text from a paste, ready for the PTY: line endings normalised to CR as xterm.js does,
/// and wrapped in bracketed-paste markers when the program asked for them.
pub fn paste_bytes(text: &str, mode: TermMode) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\r").replace('\n', "\r");
    if mode.contains(TermMode::BRACKETED_PASTE) {
        // A paste must not be able to end the bracket early and smuggle in keystrokes.
        let safe = normalized.replace("\x1b[201~", "");
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(safe.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        normalized.into_bytes()
    }
}

/// Focus reporting (DEC mode 1004): what to tell the program when focus changes.
pub fn focus_report(focused: bool, mode: TermMode) -> Option<&'static [u8]> {
    if !mode.contains(TermMode::FOCUS_IN_OUT) {
        return None;
    }
    Some(if focused { b"\x1b[I" } else { b"\x1b[O" })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAC: KeyOptions = KeyOptions {
        alt_sends_escape: false,
        mac_word_motion: true,
    };
    const LINUX: KeyOptions = KeyOptions {
        alt_sends_escape: true,
        mac_word_motion: false,
    };

    fn key(key: &str) -> KeyInput<'_> {
        KeyInput {
            key,
            modifiers: Modifiers::default(),
        }
    }

    fn with(key: &str, f: impl FnOnce(&mut Modifiers)) -> KeyInput<'_> {
        let mut modifiers = Modifiers::default();
        f(&mut modifiers);
        KeyInput { key, modifiers }
    }

    fn enc(input: KeyInput<'_>) -> Option<Vec<u8>> {
        encode_key(input, TermMode::empty(), MAC)
    }

    #[test]
    fn plain_text_is_left_to_the_text_input_path() {
        assert_eq!(enc(key("a")), None);
        assert_eq!(enc(with("a", |m| m.shift = true)), None);
        assert_eq!(enc(key("space")), None);
        assert_eq!(enc(with("e", |m| m.alt = true)), None, "Option composes on macOS");
    }

    #[test]
    fn platform_shortcuts_never_reach_the_pty() {
        assert_eq!(enc(with("c", |m| m.platform = true)), None);
        assert_eq!(enc(with("up", |m| m.platform = true)), None);
    }

    #[test]
    fn cursor_keys_follow_application_cursor_mode() {
        assert_eq!(enc(key("up")).unwrap(), b"\x1b[A");
        assert_eq!(
            encode_key(key("up"), TermMode::APP_CURSOR, MAC).unwrap(),
            b"\x1bOA"
        );
        assert_eq!(enc(with("left", |m| m.shift = true)).unwrap(), b"\x1b[1;2D");
        assert_eq!(enc(with("right", |m| m.control = true)).unwrap(), b"\x1b[1;5C");
        assert_eq!(enc(key("home")).unwrap(), b"\x1b[H");
        assert_eq!(enc(key("end")).unwrap(), b"\x1b[F");
    }

    #[test]
    fn alt_arrows_move_by_word() {
        assert_eq!(enc(with("left", |m| m.alt = true)).unwrap(), b"\x1bb");
        assert_eq!(enc(with("right", |m| m.alt = true)).unwrap(), b"\x1bf");
        let linux = |k| encode_key(with(k, |m| m.alt = true), TermMode::empty(), LINUX);
        assert_eq!(linux("left").unwrap(), b"\x1b[1;5D");
        assert_eq!(linux("up").unwrap(), b"\x1b[1;5A");
    }

    #[test]
    fn editing_and_function_keys() {
        assert_eq!(enc(key("delete")).unwrap(), b"\x1b[3~");
        assert_eq!(enc(with("delete", |m| m.shift = true)).unwrap(), b"\x1b[3;2~");
        assert_eq!(enc(key("pageup")).unwrap(), b"\x1b[5~");
        assert_eq!(enc(key("f1")).unwrap(), b"\x1bOP");
        assert_eq!(enc(with("f1", |m| m.shift = true)).unwrap(), b"\x1b[1;2P");
        assert_eq!(enc(key("f5")).unwrap(), b"\x1b[15~");
        assert_eq!(enc(key("f12")).unwrap(), b"\x1b[24~");
    }

    #[test]
    fn control_keys() {
        assert_eq!(enc(key("enter")).unwrap(), b"\r");
        assert_eq!(enc(with("enter", |m| m.alt = true)).unwrap(), b"\x1b\r");
        assert_eq!(enc(key("backspace")).unwrap(), b"\x7f");
        assert_eq!(enc(with("backspace", |m| m.control = true)).unwrap(), b"\x08");
        assert_eq!(enc(with("backspace", |m| m.alt = true)).unwrap(), b"\x1b\x7f");
        assert_eq!(enc(key("tab")).unwrap(), b"\t");
        assert_eq!(enc(with("tab", |m| m.shift = true)).unwrap(), b"\x1b[Z");
        assert_eq!(enc(key("escape")).unwrap(), b"\x1b");
    }

    #[test]
    fn ctrl_letters_and_symbols_are_c0_codes() {
        assert_eq!(enc(with("c", |m| m.control = true)).unwrap(), b"\x03");
        assert_eq!(enc(with("a", |m| m.control = true)).unwrap(), b"\x01");
        assert_eq!(enc(with("z", |m| m.control = true)).unwrap(), b"\x1a");
        assert_eq!(enc(with("[", |m| m.control = true)).unwrap(), b"\x1b");
        assert_eq!(enc(with("space", |m| m.control = true)).unwrap(), b"\x00");
        assert_eq!(enc(with("/", |m| m.control = true)).unwrap(), b"\x1f");
        assert_eq!(
            enc(with("c", |m| {
                m.control = true;
                m.alt = true;
            }))
            .unwrap(),
            b"\x1b\x03"
        );
    }

    #[test]
    fn alt_characters_send_escape_where_alt_is_meta() {
        let linux = |input| encode_key(input, TermMode::empty(), LINUX);
        assert_eq!(linux(with("x", |m| m.alt = true)).unwrap(), b"\x1bx");
        assert_eq!(
            linux(with("x", |m| {
                m.alt = true;
                m.shift = true;
            }))
            .unwrap(),
            b"\x1bX"
        );
    }

    #[test]
    fn pastes_normalise_newlines_and_honour_bracketed_mode() {
        assert_eq!(paste_bytes("a\nb\r\nc", TermMode::empty()), b"a\rb\rc");
        assert_eq!(
            paste_bytes("x\x1b[201~y", TermMode::BRACKETED_PASTE),
            b"\x1b[200~xy\x1b[201~"
        );
    }

    #[test]
    fn focus_reports_only_when_requested() {
        assert_eq!(focus_report(true, TermMode::empty()), None);
        assert_eq!(focus_report(true, TermMode::FOCUS_IN_OUT).unwrap(), b"\x1b[I");
        assert_eq!(focus_report(false, TermMode::FOCUS_IN_OUT).unwrap(), b"\x1b[O");
    }
}
