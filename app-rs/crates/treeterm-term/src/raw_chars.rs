//! Settings > Terminal > Show Raw Characters: every chunk of a terminal's output logs the
//! last [`RAW_TAIL_CHARS`] characters of its output with the control characters escaped
//! (`BaseTerminal.tsx`'s `formatRawChars` and its `[RAW]` console line).

/// How many characters of the output a `[RAW]` line shows.
pub const RAW_TAIL_CHARS: usize = 50;

/// `text` with its C0 control characters escaped, as `formatRawChars` does: ESC as `\x1b`,
/// CR, LF and TAB as `\r`, `\n` and `\t`, the others as `\x` and two lowercase hex digits.
/// Everything else, DEL and non-ASCII included, passes through.
pub fn format_raw_chars(text: &str) -> String {
    let mut formatted = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\x1b' => formatted.push_str("\\x1b"),
            '\r' => formatted.push_str("\\r"),
            '\n' => formatted.push_str("\\n"),
            '\t' => formatted.push_str("\\t"),
            c if u32::from(c) < 0x20 => formatted.push_str(&format!("\\x{:02x}", u32::from(c))),
            c => formatted.push(c),
        }
    }
    formatted
}

/// What a terminal's output logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawLog {
    Off,
    /// Logging: the last [`RAW_TAIL_CHARS`] characters of the output so far.
    On {
        tail: String,
    },
}

impl RawLog {
    /// Logging from now on when `on`, starting from no output (Electron's `rawChars`
    /// starts empty each time the terminal mounts).
    pub fn new(on: bool) -> Self {
        if on {
            Self::On {
                tail: String::new(),
            }
        } else {
            Self::Off
        }
    }

    /// The output gained `bytes`: what its `[RAW]` line shows, the escaped tail, when
    /// logging. Each chunk is decoded on its own, as Electron's fresh `TextDecoder` per
    /// event does, so a character split between chunks reads as U+FFFD. The tail keeps
    /// characters where Electron's `slice(-50)` keeps UTF-16 units; they differ only for
    /// characters outside the BMP.
    pub fn output(&mut self, bytes: &[u8]) -> Option<String> {
        match self {
            Self::Off => None,
            Self::On { tail } => {
                tail.push_str(&String::from_utf8_lossy(bytes));
                let excess = tail.chars().count().saturating_sub(RAW_TAIL_CHARS);
                if let Some((start, _)) = tail.char_indices().nth(excess) {
                    tail.drain(..start);
                }
                Some(format_raw_chars(tail))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_is_written_as_hex() {
        assert_eq!(format_raw_chars("\x1b[0m"), "\\x1b[0m");
    }

    #[test]
    fn carriage_return_line_feed_and_tab_are_written_as_escapes() {
        assert_eq!(format_raw_chars("a\rb"), "a\\rb");
        assert_eq!(format_raw_chars("a\nb"), "a\\nb");
        assert_eq!(format_raw_chars("a\tb"), "a\\tb");
    }

    #[test]
    fn other_control_characters_are_two_lowercase_hex_digits() {
        assert_eq!(format_raw_chars("\x00"), "\\x00");
        assert_eq!(format_raw_chars("\x07"), "\\x07");
        assert_eq!(format_raw_chars("\x0b"), "\\x0b");
        assert_eq!(format_raw_chars("\x1f"), "\\x1f");
    }

    #[test]
    fn printable_characters_delete_and_non_ascii_pass_through() {
        assert_eq!(format_raw_chars(" hi ~"), " hi ~");
        assert_eq!(format_raw_chars("\x7f"), "\x7f");
        assert_eq!(format_raw_chars("é中\u{80}"), "é中\u{80}");
        assert_eq!(
            format_raw_chars("\\x1b"),
            "\\x1b",
            "a backslash is not escaped"
        );
    }

    #[test]
    fn off_logs_nothing() {
        let mut log = RawLog::new(false);
        assert_eq!(log, RawLog::Off);
        assert_eq!(log.output(b"hi"), None);
    }

    #[test]
    fn on_logs_the_output_so_far_escaped() {
        let mut log = RawLog::new(true);
        assert_eq!(
            log.output(b"$ echo hi\r\n").as_deref(),
            Some("$ echo hi\\r\\n")
        );
        assert_eq!(
            log.output(b"hi\r\n").as_deref(),
            Some("$ echo hi\\r\\nhi\\r\\n")
        );
    }

    #[test]
    fn on_keeps_the_last_fifty_characters_across_chunks() {
        let mut log = RawLog::new(true);
        let first: String = ('a'..='z').collect();
        assert_eq!(log.output(first.as_bytes()), Some(first.clone()));
        let second = "0123456789".repeat(3);
        let expected = format!("{}{second}", &first[6..]);
        assert_eq!(expected.chars().count(), RAW_TAIL_CHARS);
        assert_eq!(log.output(second.as_bytes()), Some(expected));
        // A chunk longer than the tail leaves only its own end.
        let long = "中".repeat(60);
        assert_eq!(
            log.output(long.as_bytes()),
            Some("中".repeat(RAW_TAIL_CHARS))
        );
        assert_eq!(
            log,
            RawLog::On {
                tail: "中".repeat(RAW_TAIL_CHARS)
            }
        );
    }

    #[test]
    fn invalid_or_split_utf8_reads_as_replacement_characters() {
        let mut log = RawLog::new(true);
        let e_acute = "é".as_bytes();
        assert_eq!(log.output(&e_acute[..1]).as_deref(), Some("\u{fffd}"));
        assert_eq!(
            log.output(&e_acute[1..]).as_deref(),
            Some("\u{fffd}\u{fffd}")
        );
        assert_eq!(
            log.output(b"\xffok").as_deref(),
            Some("\u{fffd}\u{fffd}\u{fffd}ok")
        );
    }
}
