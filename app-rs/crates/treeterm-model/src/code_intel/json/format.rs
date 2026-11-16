//! Format Document and Format Selection in a JSON file: jsonc-parser's `format` as Monaco's
//! JSON worker runs it (vscode-json-languageservice's `format`): the editor's indentation,
//! the file's own line break, no lines kept, no final newline added.
//!
//! Every token goes on a line of its own after `{`, `[`, `,` and a line comment, `}` and `]`
//! one level out; `:` is followed by one space and a block comment preceded by one. Around a
//! token the scanner cannot read (and after a value something other than `,` follows)
//! nothing is changed.

use std::ops::Range;

use super::{ScanError, Scanner, Token};
use crate::code_intel::TextEdit;
use crate::indentation::{IndentUnit, Indentation};

fn is_eol(byte: u8) -> bool {
    byte == b'\r' || byte == b'\n'
}

/// The file's first line break; `\n` in a file of one line.
fn eol_of(text: &str) -> &'static str {
    let bytes = text.as_bytes();
    match bytes.iter().position(|byte| is_eol(*byte)) {
        Some(at) if bytes[at] == b'\r' && bytes.get(at + 1) == Some(&b'\n') => "\r\n",
        Some(at) if bytes[at] == b'\r' => "\r",
        Some(_) | None => "\n",
    }
}

/// `computeIndentLevel`: how many indent steps a line's leading spaces and tabs make.
fn indent_level(line: &str, tab_size: usize) -> usize {
    let columns: usize = line.bytes().map_while(|byte| match byte {
        b' ' => Some(1),
        b'\t' => Some(tab_size),
        _ => None,
    }).sum();
    columns / tab_size
}

/// The edits that format `document`, or the lines `range` touches of it (indented from
/// where the first of them is), with `indentation`.
pub(in crate::code_intel) fn format(document: &str, range: Option<Range<usize>>, indentation: Indentation) -> Vec<TextEdit> {
    let bytes = document.as_bytes();
    let tab_size = if indentation.tab_size == 0 { 4 } else { indentation.tab_size };
    let (start, text, initial_level) = match &range {
        Some(range) => {
            let start = (0..range.start).rev().find(|&at| is_eol(bytes[at])).map_or(0, |at| at + 1);
            let end = (range.end..document.len()).find(|&at| is_eol(bytes[at])).unwrap_or(document.len());
            let text = &document[start..end];
            (start, text, indent_level(text, tab_size))
        }
        None => (0, document, 0),
    };
    let eol = eol_of(document);
    let indent = match indentation.unit {
        IndentUnit::Spaces => " ".repeat(tab_size),
        IndentUnit::Tab => "\t".to_owned(),
    };
    let mut scanner = Scanner::new(text);
    let mut line_breaks = 0;
    let mut level: isize = 0;
    let mut has_error = false;
    let mut edits: Vec<TextEdit> = Vec::new();

    let new_line = |level: isize| {
        let steps = (initial_level as isize + level).max(0) as usize;
        format!("{eol}{}", indent.repeat(steps))
    };
    let scan_next = |scanner: &mut Scanner<'_>, line_breaks: &mut usize, has_error: &mut bool| {
        let mut token = scanner.scan();
        *line_breaks = 0;
        while matches!(token, Token::Trivia | Token::LineBreak) {
            if token == Token::LineBreak {
                *line_breaks = 1;
            }
            token = scanner.scan();
        }
        *has_error = token == Token::Unknown || scanner.error != ScanError::None;
        token
    };
    let add_edit = |edits: &mut Vec<TextEdit>, has_error: bool, new_text: String, from: usize, to: usize| {
        let to = to.min(document.len());
        let in_range = range.as_ref().is_none_or(|range| from < range.end && to > range.start);
        if !has_error && in_range && from <= to && document[from..to] != new_text {
            edits.push(TextEdit { range: from..to, text: new_text });
        }
    };
    let token_end = |scanner: &Scanner<'_>| start + scanner.token_offset + scanner.token_len();

    let mut first = scan_next(&mut scanner, &mut line_breaks, &mut has_error);
    if first != Token::Eof {
        let first_start = start + scanner.token_offset;
        add_edit(&mut edits, has_error, indent.repeat(initial_level), start, first_start);
    }
    while first != Token::Eof {
        let mut first_end = token_end(&scanner);
        let mut second = scan_next(&mut scanner, &mut line_breaks, &mut has_error);
        let mut replace = String::new();
        let mut needs_line_break = false;
        while line_breaks == 0 && matches!(second, Token::LineComment | Token::BlockComment) {
            let comment_start = start + scanner.token_offset;
            add_edit(&mut edits, has_error, " ".to_owned(), first_end, comment_start);
            first_end = token_end(&scanner);
            needs_line_break = second == Token::LineComment;
            replace = if needs_line_break { new_line(level) } else { String::new() };
            second = scan_next(&mut scanner, &mut line_breaks, &mut has_error);
        }
        let closes = match second {
            Token::CloseBrace => Some(Token::OpenBrace),
            Token::CloseBracket => Some(Token::OpenBracket),
            _ => None,
        };
        if let Some(open) = closes {
            if first != open {
                level -= 1;
                replace = new_line(level);
            }
        } else {
            match first {
                Token::OpenBracket | Token::OpenBrace => {
                    level += 1;
                    replace = new_line(level);
                }
                Token::Comma | Token::LineComment => replace = new_line(level),
                Token::BlockComment if line_breaks > 0 => replace = new_line(level),
                Token::BlockComment | Token::Colon if !needs_line_break => replace = " ".to_owned(),
                Token::String if second == Token::Colon && !needs_line_break => replace = String::new(),
                Token::Null | Token::True | Token::False | Token::Number | Token::CloseBrace | Token::CloseBracket => {
                    if matches!(second, Token::LineComment | Token::BlockComment) && !needs_line_break {
                        replace = " ".to_owned();
                    } else if second != Token::Comma && second != Token::Eof {
                        has_error = true;
                    }
                }
                Token::Unknown => has_error = true,
                _ => {}
            }
            if line_breaks > 0 && matches!(second, Token::LineComment | Token::BlockComment) {
                replace = new_line(level);
            }
        }
        if second == Token::Eof {
            replace = String::new();
        }
        let second_start = start + scanner.token_offset;
        add_edit(&mut edits, has_error, replace, first_end, second_start);
        first = second;
    }
    edits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_intel::apply_edits;

    const SPACES: Indentation = Indentation::MONACO_DEFAULT;

    fn formatted(text: &str, range: Option<Range<usize>>, indentation: Indentation) -> String {
        apply_edits(text, &format(text, range, indentation), 0..0).0
    }

    /// Checked against jsonc-parser's `format` with the options Monaco's JSON worker passes.
    #[test]
    fn a_document_is_laid_out_one_value_to_a_line() {
        let text = "{\"a\":1,\"b\":[true,null,{}],\"c\":{ \"d\" :\"x\"}, \"e\": []}";
        assert_eq!(
            formatted(text, None, SPACES),
            "{\n    \"a\": 1,\n    \"b\": [\n        true,\n        null,\n        {}\n    ],\n    \"c\": {\n        \"d\": \"x\"\n    },\n    \"e\": []\n}"
        );
        let tabs = Indentation { unit: IndentUnit::Tab, tab_size: 2 };
        assert_eq!(formatted("[1,[2]]\n", None, tabs), "[\n\t1,\n\t[\n\t\t2\n\t]\n]");
        let two = Indentation { unit: IndentUnit::Spaces, tab_size: 2 };
        assert_eq!(formatted("{\r\n\"a\":1}", None, two), "{\r\n  \"a\": 1\r\n}");
    }

    #[test]
    fn comments_keep_their_lines() {
        let text = "{ // top\n\"a\": 1, /* b */ \"c\": 2 /* d */\n// last\n}";
        assert_eq!(formatted(text, None, SPACES), "{ // top\n    \"a\": 1, /* b */\n    \"c\": 2 /* d */\n    // last\n}");
    }

    #[test]
    fn what_the_scanner_cannot_read_is_left_as_it_is() {
        assert_eq!(formatted("{\"a\": tru e,\"b\":1}", None, SPACES), "{\n    \"a\": tru e,\n    \"b\": 1\n}");
        assert_eq!(formatted("[1 2]", None, SPACES), "[\n    1 2\n]");
        assert_eq!(formatted("", None, SPACES), "");
        assert_eq!(formatted("/*", None, SPACES), "/*");
    }

    #[test]
    fn a_selection_formats_the_lines_it_touches_from_their_indent() {
        let text = "{\n    \"a\": {\"b\":1,\n    \"c\":2},\n    \"d\":3}";
        let b = text.find("\"b\"").unwrap();
        assert_eq!(
            formatted(text, Some(b - 1..b + 5), SPACES),
            "{\n    \"a\": {\n        \"b\": 1,\n    \"c\":2},\n    \"d\":3}"
        );
    }
}
