//! The problems Monaco's JSON worker marks in a `json` file: a port of
//! vscode-json-languageservice's parser (`parser/jsonParser.ts`'s `parse`, on jsonc-parser's
//! scanner) and of `doValidation` with Monaco 0.55.1's defaults (`comments: 'error'`,
//! `trailingCommas: 'error'`, no schemas, no schema requests).
//!
//! Without a schema the worker reports the syntax errors, the comments ("Comments are not
//! permitted in JSON."), duplicate keys (warnings), and a warning at a `$schema` it cannot
//! load: it has no way to fetch one.
//!
//! The scanner reads bytes: every character it tells apart is ASCII, so byte offsets stand
//! for JavaScript's UTF-16 offsets. Keys are compared as JavaScript strings (UTF-16 units).

use std::collections::HashMap;
use std::ops::Range;

use super::uri::{Uri, is_scheme};
use super::{Problem, Severity};

mod completion;
mod format;

pub use completion::completions;
pub(super) use completion::TRIGGER_CHARACTERS;
pub(super) use format::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token {
    OpenBrace,
    CloseBrace,
    OpenBracket,
    CloseBracket,
    Comma,
    Colon,
    Null,
    True,
    False,
    String,
    Number,
    LineComment,
    BlockComment,
    LineBreak,
    Trivia,
    Unknown,
    Eof,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScanError {
    None,
    UnexpectedEndOfComment,
    UnexpectedEndOfString,
    UnexpectedEndOfNumber,
    InvalidUnicode,
    InvalidEscapeCharacter,
    InvalidCharacter,
}

/// jsonc-parser's `createScanner(text, false)`.
struct Scanner<'a> {
    text: &'a [u8],
    pos: usize,
    token: Token,
    token_offset: usize,
    /// The token's value as JavaScript holds it: a string's content with its escapes read,
    /// or the text of an unknown token.
    value: Vec<u16>,
    error: ScanError,
}

fn is_white_space(ch: u8) -> bool {
    ch == b' ' || ch == b'\t'
}

fn is_line_break(ch: u8) -> bool {
    ch == b'\n' || ch == b'\r'
}

fn is_unknown_content(ch: u8) -> bool {
    !is_white_space(ch) && !is_line_break(ch) && !matches!(ch, b'}' | b']' | b'{' | b'[' | b'"' | b':' | b',' | b'/')
}

fn utf16(bytes: &[u8]) -> Vec<u16> {
    String::from_utf8_lossy(bytes).encode_utf16().collect()
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self { text: text.as_bytes(), pos: 0, token: Token::Unknown, token_offset: 0, value: Vec::new(), error: ScanError::None }
    }

    fn at(&self, pos: usize) -> Option<u8> {
        self.text.get(pos).copied()
    }

    fn token_len(&self) -> usize {
        self.pos - self.token_offset
    }

    /// Up to `count` hex digits as a number; `None` when fewer were there.
    fn scan_hex_digits(&mut self, count: usize) -> Option<u16> {
        let mut digits = 0;
        let mut value: u32 = 0;
        while digits < count {
            let Some(digit) = self.at(self.pos).and_then(|ch| (ch as char).to_digit(16)) else {
                break;
            };
            value = value * 16 + digit;
            self.pos += 1;
            digits += 1;
        }
        (digits == count).then_some(value as u16)
    }

    fn scan_number(&mut self) {
        let is_digit = |ch: Option<u8>| ch.is_some_and(|ch| ch.is_ascii_digit());
        if self.at(self.pos) == Some(b'0') {
            self.pos += 1;
        } else {
            self.pos += 1;
            while is_digit(self.at(self.pos)) {
                self.pos += 1;
            }
        }
        if self.at(self.pos) == Some(b'.') {
            self.pos += 1;
            if is_digit(self.at(self.pos)) {
                while is_digit(self.at(self.pos)) {
                    self.pos += 1;
                }
            } else {
                self.error = ScanError::UnexpectedEndOfNumber;
                return;
            }
        }
        if matches!(self.at(self.pos), Some(b'E' | b'e')) {
            self.pos += 1;
            if matches!(self.at(self.pos), Some(b'+' | b'-')) {
                self.pos += 1;
            }
            if is_digit(self.at(self.pos)) {
                while is_digit(self.at(self.pos)) {
                    self.pos += 1;
                }
            } else {
                self.error = ScanError::UnexpectedEndOfNumber;
            }
        }
    }

    fn scan_string(&mut self) {
        let mut start = self.pos;
        loop {
            let Some(ch) = self.at(self.pos) else {
                self.value.extend(utf16(&self.text[start..self.pos]));
                self.error = ScanError::UnexpectedEndOfString;
                break;
            };
            if ch == b'"' {
                self.value.extend(utf16(&self.text[start..self.pos]));
                self.pos += 1;
                break;
            }
            if ch == b'\\' {
                self.value.extend(utf16(&self.text[start..self.pos]));
                self.pos += 1;
                let Some(escaped) = self.at(self.pos) else {
                    self.error = ScanError::UnexpectedEndOfString;
                    break;
                };
                self.pos += 1;
                let unit = match escaped {
                    b'"' => Some(u16::from(b'"')),
                    b'\\' => Some(u16::from(b'\\')),
                    b'/' => Some(u16::from(b'/')),
                    b'b' => Some(0x08),
                    b'f' => Some(0x0c),
                    b'n' => Some(u16::from(b'\n')),
                    b'r' => Some(u16::from(b'\r')),
                    b't' => Some(u16::from(b'\t')),
                    b'u' => match self.scan_hex_digits(4) {
                        Some(unit) => Some(unit),
                        None => {
                            self.error = ScanError::InvalidUnicode;
                            None
                        }
                    },
                    _ => {
                        self.error = ScanError::InvalidEscapeCharacter;
                        None
                    }
                };
                self.value.extend(unit);
                start = self.pos;
                continue;
            }
            if ch <= 0x1f {
                if is_line_break(ch) {
                    self.value.extend(utf16(&self.text[start..self.pos]));
                    self.error = ScanError::UnexpectedEndOfString;
                    break;
                }
                // Marked, but the string goes on.
                self.error = ScanError::InvalidCharacter;
            }
            self.pos += 1;
        }
    }

    fn scan(&mut self) -> Token {
        self.value.clear();
        self.error = ScanError::None;
        self.token_offset = self.pos;
        self.token = self.scan_token();
        self.token
    }

    fn scan_token(&mut self) -> Token {
        let len = self.text.len();
        let Some(code) = self.at(self.pos) else {
            self.token_offset = len;
            return Token::Eof;
        };
        if is_white_space(code) {
            while self.at(self.pos).is_some_and(is_white_space) {
                self.pos += 1;
            }
            return Token::Trivia;
        }
        if is_line_break(code) {
            self.pos += 1;
            if code == b'\r' && self.at(self.pos) == Some(b'\n') {
                self.pos += 1;
            }
            return Token::LineBreak;
        }
        let single = |scanner: &mut Self, token: Token| {
            scanner.pos += 1;
            token
        };
        match code {
            b'{' => single(self, Token::OpenBrace),
            b'}' => single(self, Token::CloseBrace),
            b'[' => single(self, Token::OpenBracket),
            b']' => single(self, Token::CloseBracket),
            b':' => single(self, Token::Colon),
            b',' => single(self, Token::Comma),
            b'"' => {
                self.pos += 1;
                self.scan_string();
                Token::String
            }
            b'/' => {
                if self.at(self.pos + 1) == Some(b'/') {
                    self.pos += 2;
                    while self.at(self.pos).is_some_and(|ch| !is_line_break(ch)) {
                        self.pos += 1;
                    }
                    return Token::LineComment;
                }
                if self.at(self.pos + 1) == Some(b'*') {
                    self.pos += 2;
                    let mut closed = false;
                    while self.pos + 1 < len {
                        let ch = self.text[self.pos];
                        if ch == b'*' && self.text[self.pos + 1] == b'/' {
                            self.pos += 2;
                            closed = true;
                            break;
                        }
                        self.pos += 1;
                        // A CRLF is stepped over whole, which can take the scan to the end.
                        if ch == b'\r' && self.at(self.pos) == Some(b'\n') {
                            self.pos += 1;
                        }
                    }
                    if !closed {
                        // One past the end when the text ends at `/*`, as in jsonc-parser:
                        // the end of the text then reads as one character long.
                        self.pos += 1;
                        self.error = ScanError::UnexpectedEndOfComment;
                    }
                    return Token::BlockComment;
                }
                self.value = utf16(b"/");
                self.pos += 1;
                Token::Unknown
            }
            b'-' => {
                self.pos += 1;
                if !self.at(self.pos).is_some_and(|ch| ch.is_ascii_digit()) {
                    self.value = utf16(b"-");
                    return Token::Unknown;
                }
                self.scan_number();
                Token::Number
            }
            b'0'..=b'9' => {
                self.scan_number();
                Token::Number
            }
            _ => {
                while self.at(self.pos).is_some_and(is_unknown_content) {
                    self.pos += 1;
                }
                // `code` is not one of the characters above, so it is unknown content: the
                // word holds at least it.
                let word = &self.text[self.token_offset..self.pos];
                match word {
                    b"true" => Token::True,
                    b"false" => Token::False,
                    b"null" => Token::Null,
                    _ => {
                        self.value = utf16(word);
                        Token::Unknown
                    }
                }
            }
        }
    }
}

/// What a value is, as jsonParser's AST holds it.
#[derive(Debug, Clone)]
enum NodeKind {
    Object(Vec<PropertyNode>),
    Array(Vec<Node>),
    String(Vec<u16>),
    Number,
    Boolean(bool),
    Null,
}

/// A value, and the bytes it spans as jsonParser measures them (an unclosed object or array
/// ends with the token its error was reported at).
#[derive(Debug, Clone)]
struct Node {
    range: Range<usize>,
    kind: NodeKind,
}

/// A string, or an unquoted word in a key's place, with its value as JavaScript holds it.
#[derive(Debug, Clone)]
struct Key {
    range: Range<usize>,
    value: Vec<u16>,
}

#[derive(Debug, Clone)]
struct PropertyNode {
    range: Range<usize>,
    key: Key,
    /// Where its `:` is, when it has one.
    colon: Option<usize>,
    value: Option<Node>,
}

/// `keysSeen`: a key's first property, until it is seen again.
enum Seen {
    First(Range<usize>),
    Again,
}

/// How deep arrays and objects may nest. jsonParser descends a level of JavaScript's stack
/// for each: deeper than a few thousand levels it runs out, the worker's validation fails
/// and Monaco marks nothing. The parser here gives up deeper than this and marks nothing
/// either, inside a background thread's stack (2 MiB) even unoptimized, where a level of
/// objects takes some 2.7 KiB.
const MAX_NESTING: usize = 512;

/// How far the parser got.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reading {
    /// Reading a value inside this many arrays and objects.
    Nested(usize),
    /// Values were nested deeper than [`MAX_NESTING`]: the parser stopped there.
    TooDeep,
}

/// jsonParser's `parse`, collecting comments.
struct Parser<'a> {
    text: &'a str,
    scanner: Scanner<'a>,
    problems: Vec<Problem>,
    last_problem_offset: Option<usize>,
    comments: Vec<Range<usize>>,
    reading: Reading,
}

/// JavaScript's `\s`.
fn is_js_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

impl<'a> Parser<'a> {
    fn scan_next(&mut self) -> Token {
        loop {
            let token = self.scanner.scan();
            self.check_scan_error();
            match token {
                Token::LineComment | Token::BlockComment => {
                    self.comments.push(self.scanner.token_offset..self.scanner.pos.min(self.text.len()));
                }
                Token::Trivia | Token::LineBreak => {}
                _ => return token,
            }
        }
    }

    fn token(&self) -> Token {
        self.scanner.token
    }

    /// Where the current token ends (`_finalize`), within the text.
    fn token_end(&self) -> usize {
        self.scanner.pos.min(self.text.len())
    }

    /// `_errorAtRange`: one problem per start. The range may reach one past the end of the
    /// text, which Monaco's `positionAt` clamps.
    fn error_at_range(&mut self, message: &str, range: Range<usize>, severity: Severity) {
        let range = range.start.min(self.text.len())..range.end.min(self.text.len());
        if self.problems.is_empty() || self.last_problem_offset != Some(range.start) {
            self.last_problem_offset = Some(range.start);
            self.problems.push(Problem { range, severity, message: message.to_owned() });
        }
    }

    /// `_error`: at the current token, or at the last character before an empty one; then
    /// skip tokens up to (and past) `skip_until_after`, or up to `skip_until`.
    fn error(&mut self, message: &str, skip_until_after: &[Token], skip_until: &[Token]) {
        let mut start = self.scanner.token_offset;
        let mut end = start + self.scanner.token_len();
        if start == end && start > 0 {
            let before = &self.text[..start];
            let mut chars = before.char_indices().rev();
            let (mut index, mut c) = chars.next().unwrap_or((0, ' '));
            while index > 0 && is_js_space(c) {
                (index, c) = chars.next().unwrap_or((0, ' '));
            }
            start = index;
            end = start + self.text[start..].chars().next().map_or(1, char::len_utf8);
        }
        self.error_at_range(message, start..end, Severity::Error);
        if skip_until_after.is_empty() && skip_until.is_empty() {
            return;
        }
        let mut token = self.token();
        while token != Token::Eof {
            if skip_until_after.contains(&token) {
                self.scan_next();
                break;
            } else if skip_until.contains(&token) {
                break;
            }
            token = self.scan_next();
        }
    }

    fn check_scan_error(&mut self) {
        let message = match self.scanner.error {
            ScanError::None => return,
            ScanError::InvalidUnicode => "Invalid unicode sequence in string.",
            ScanError::InvalidEscapeCharacter => "Invalid escape character in string.",
            ScanError::UnexpectedEndOfNumber => "Unexpected end of number.",
            ScanError::UnexpectedEndOfComment => "Unexpected end of comment.",
            ScanError::UnexpectedEndOfString => "Unexpected end of string.",
            ScanError::InvalidCharacter => "Invalid characters in string. Control characters must be escaped.",
        };
        self.error(message, &[], &[]);
    }

    fn parse_array(&mut self) -> Option<Node> {
        if self.token() != Token::OpenBracket {
            return None;
        }
        let start = self.scanner.token_offset;
        self.scan_next();
        let mut items = Vec::new();
        let mut needs_comma = false;
        while self.token() != Token::CloseBracket && self.token() != Token::Eof {
            if self.token() == Token::Comma {
                if !needs_comma {
                    self.error("Value expected", &[], &[]);
                }
                let comma = self.scanner.token_offset;
                self.scan_next();
                if self.token() == Token::CloseBracket {
                    if needs_comma {
                        self.error_at_range("Trailing comma", comma..comma + 1, Severity::Error);
                    }
                    continue;
                }
            } else if needs_comma {
                self.error("Expected comma", &[], &[]);
            }
            match self.parse_value() {
                Some(item) => items.push(item),
                None => self.error("Value expected", &[], &[Token::CloseBracket, Token::Comma]),
            }
            needs_comma = true;
        }
        if self.token() != Token::CloseBracket {
            self.error("Expected comma or closing bracket", &[], &[]);
            return Some(Node { range: start..self.token_end(), kind: NodeKind::Array(items) });
        }
        let end = self.token_end();
        self.scan_next();
        Some(Node { range: start..end, kind: NodeKind::Array(items) })
    }

    fn parse_property(&mut self, keys_seen: &mut HashMap<Vec<u16>, Seen>) -> Option<PropertyNode> {
        let start = self.scanner.token_offset;
        let key = match self.parse_string() {
            Some(key) => key,
            None if self.token() == Token::Unknown => {
                // A more helpful message.
                self.error("Property keys must be doublequoted", &[], &[]);
                let key = Key { range: self.scanner.token_offset..self.token_end(), value: self.scanner.value.clone() };
                self.scan_next();
                key
            }
            None => return None,
        };
        // JSON files that forbid comments use "//" keys for them, as many as they like.
        if key.value != utf16(b"//") {
            match keys_seen.get(&key.value) {
                Some(seen) => {
                    self.error_at_range("Duplicate object key", key.range.clone(), Severity::Warning);
                    if let Seen::First(first) = seen {
                        let first = first.clone();
                        self.error_at_range("Duplicate object key", first, Severity::Warning);
                    }
                    keys_seen.insert(key.value.clone(), Seen::Again);
                }
                None => {
                    keys_seen.insert(key.value.clone(), Seen::First(key.range.clone()));
                }
            }
        }
        let mut colon = None;
        if self.token() == Token::Colon {
            colon = Some(self.scanner.token_offset);
            self.scan_next();
        } else {
            self.error("Colon expected", &[], &[]);
            let next = self.scanner.token_offset;
            if self.token() == Token::String && self.text[key.range.end..next].contains(['\n', '\r']) {
                return Some(PropertyNode { range: start..key.range.end, key, colon, value: None });
            }
        }
        match self.parse_value() {
            Some(value) => Some(PropertyNode { range: start..value.range.end, key, colon, value: Some(value) }),
            None => {
                // The property ends with the token the error is reported at.
                let end = self.token_end();
                self.error("Value expected", &[], &[Token::CloseBrace, Token::Comma]);
                Some(PropertyNode { range: start..end, key, colon, value: None })
            }
        }
    }

    fn parse_object(&mut self) -> Option<Node> {
        if self.token() != Token::OpenBrace {
            return None;
        }
        let start = self.scanner.token_offset;
        let mut keys_seen = HashMap::new();
        let mut properties = Vec::new();
        self.scan_next();
        let mut needs_comma = false;
        while self.token() != Token::CloseBrace && self.token() != Token::Eof {
            if self.token() == Token::Comma {
                if !needs_comma {
                    self.error("Property expected", &[], &[]);
                }
                let comma = self.scanner.token_offset;
                self.scan_next();
                if self.token() == Token::CloseBrace {
                    if needs_comma {
                        self.error_at_range("Trailing comma", comma..comma + 1, Severity::Error);
                    }
                    continue;
                }
            } else if needs_comma {
                self.error("Expected comma", &[], &[]);
            }
            match self.parse_property(&mut keys_seen) {
                Some(property) => properties.push(property),
                None => self.error("Property expected", &[], &[Token::CloseBrace, Token::Comma]),
            }
            needs_comma = true;
        }
        if self.token() != Token::CloseBrace {
            self.error("Expected comma or closing brace", &[], &[]);
            return Some(Node { range: start..self.token_end(), kind: NodeKind::Object(properties) });
        }
        let end = self.token_end();
        self.scan_next();
        Some(Node { range: start..end, kind: NodeKind::Object(properties) })
    }

    fn parse_string(&mut self) -> Option<Key> {
        if self.token() != Token::String {
            return None;
        }
        let string = Key { range: self.scanner.token_offset..self.token_end(), value: self.scanner.value.clone() };
        self.scan_next();
        Some(string)
    }

    /// An array or object, one level deeper; past [`MAX_NESTING`] the parser gives up: it
    /// skips to the end of the text, where every value being read ends.
    fn parse_nested(&mut self, parse: fn(&mut Self) -> Option<Node>) -> Option<Node> {
        let Reading::Nested(depth) = self.reading else {
            return None;
        };
        if depth == MAX_NESTING {
            self.reading = Reading::TooDeep;
            self.scanner.pos = self.text.len();
            self.scanner.scan();
            return None;
        }
        self.reading = Reading::Nested(depth + 1);
        let node = parse(self);
        if let Reading::Nested(depth) = &mut self.reading {
            *depth -= 1;
        }
        node
    }

    fn parse_value(&mut self) -> Option<Node> {
        match self.token() {
            Token::OpenBracket => self.parse_nested(Self::parse_array),
            Token::OpenBrace => self.parse_nested(Self::parse_object),
            Token::String => {
                self.parse_string().map(|Key { range, value }| Node { range, kind: NodeKind::String(value) })
            }
            // A number token always reads as a JSON number, or its scan error was reported.
            Token::Number | Token::Null | Token::True | Token::False => {
                let range = self.scanner.token_offset..self.token_end();
                let kind = match self.token() {
                    Token::True => NodeKind::Boolean(true),
                    Token::False => NodeKind::Boolean(false),
                    Token::Null => NodeKind::Null,
                    _ => NodeKind::Number,
                };
                self.scan_next();
                Some(Node { range, kind })
            }
            Token::CloseBrace
            | Token::CloseBracket
            | Token::Comma
            | Token::Colon
            | Token::LineComment
            | Token::BlockComment
            | Token::LineBreak
            | Token::Trivia
            | Token::Unknown
            | Token::Eof => None,
        }
    }
}

/// The JSON schema drafts the language service has built in: it loads nothing for them.
const BUILT_IN_SCHEMAS: [&str; 2] = ["http://json-schema.org/draft-04/schema#", "http://json-schema.org/draft-07/schema#"];

/// jsonWorker's `normalizePath`.
fn normalize_path(parts: &[&str]) -> String {
    let mut normalized: Vec<&str> = Vec::new();
    for part in parts {
        match *part {
            "" | "." => {}
            ".." => {
                normalized.pop();
            }
            part => normalized.push(part),
        }
    }
    if parts.len() > 1 && parts.last() == Some(&"") {
        normalized.push("");
    }
    let joined = normalized.join("/");
    if parts.first() == Some(&"") { format!("/{joined}") } else { joined }
}

/// The schema service's `normalizeId`: the id as vscode-uri writes it back, or as it is
/// where vscode-uri cannot read it.
fn normalize_id(id: &str) -> String {
    Uri::parse(id).map_or_else(|| id.to_owned(), |uri| uri.to_display_string())
}

/// The schema `id` a `$schema` names, resolved and normalized as the schema service does:
/// an id without a scheme is resolved against the model, which Monaco names
/// `inmemory://model/<n>`.
fn schema_id(id: &str) -> String {
    let has_scheme = id.split_once(':').is_some_and(|(scheme, _)| is_scheme(scheme));
    if has_scheme {
        return normalize_id(id);
    }
    let mut parts = vec!["", ""];
    parts.extend(id.split('/'));
    let path = normalize_path(&parts).replace('#', "%23").replace('?', "%3F");
    format!("inmemory://model{path}")
}

/// `toDisplayString`: a `file:` URI as its path.
fn schema_display(url: &str) -> String {
    match Uri::parse(url) {
        Some(uri) if uri.scheme == "file" => uri.fs_path(),
        Some(_) | None => url.to_owned(),
    }
}

/// A JSON document as jsonParser reads it.
struct Parsed {
    /// Its value; `None` for an empty text, or one that starts with no value.
    root: Option<Node>,
    syntax_errors: Vec<Problem>,
    comments: Vec<Range<usize>>,
}

/// jsonParser's `parse` of `text`; `None` when its arrays and objects nest deeper than the
/// worker can read.
fn parse(text: &str) -> Option<Parsed> {
    let mut parser = Parser {
        text,
        scanner: Scanner::new(text),
        problems: Vec::new(),
        last_problem_offset: None,
        comments: Vec::new(),
        reading: Reading::Nested(0),
    };
    let mut root = None;
    if parser.scan_next() != Token::Eof {
        root = parser.parse_value();
        if root.is_none() {
            parser.error("Expected a JSON object, array or literal.", &[], &[]);
        } else if parser.token() != Token::Eof {
            parser.error("End of file expected.", &[], &[]);
        }
    }
    let Parser { problems: syntax_errors, comments, reading, .. } = parser;
    match reading {
        Reading::Nested(_) => Some(Parsed { root, syntax_errors, comments }),
        Reading::TooDeep => None,
    }
}

/// The `$schema` of a document, which Monaco's worker reads it with: the root object's first
/// `$schema` property whose value is a string (the schema service's `getSchemaFromProperty`).
fn schema_of(root: Option<&Node>) -> Option<&[u16]> {
    let Some(Node { kind: NodeKind::Object(properties), .. }) = root else {
        return None;
    };
    properties.iter().find_map(|property| match &property.value {
        Some(Node { kind: NodeKind::String(value), .. }) if property.key.value == utf16(b"$schema") => Some(value.as_slice()),
        Some(_) | None => None,
    })
}

/// The problems Monaco marks in `text`, a `json` file, by start; none when its arrays and
/// objects nest deeper than the worker can read.
pub fn problems(text: &str) -> Vec<Problem> {
    let Some(Parsed { root, syntax_errors, comments }) = parse(text) else {
        return Vec::new();
    };
    let mut problems: Vec<Problem> = Vec::new();
    let mut add = |problem: Problem| {
        if !problems.iter().any(|added| added.range.start == problem.range.start && added.message == problem.message) {
            problems.push(problem);
        }
    };
    // The `$schema` the schema service would load, and what it would fail with: at the first
    // property's value when that is `$schema`, else at the root's first character.
    if let Some(Node { range: root_range, kind: NodeKind::Object(properties) }) = &root {
        let is_schema = |key: &Key| key.value == utf16(b"$schema");
        if let Some(schema) = schema_of(root.as_ref()) {
            let id = schema_id(&String::from_utf16_lossy(schema));
            if !BUILT_IN_SCHEMAS.iter().any(|built_in| normalize_id(built_in) == id) {
                let range = match properties.first() {
                    Some(first) if is_schema(&first.key) => {
                        first.value.as_ref().map_or(first.range.clone(), |value| value.range.clone())
                    }
                    Some(_) | None => root_range.start..root_range.start + 1,
                };
                let message = format!(
                    "Unable to load schema from '{}'. No schema request service available",
                    schema_display(&id)
                );
                add(Problem { range, severity: Severity::Warning, message });
            }
        }
    }
    for problem in syntax_errors {
        add(problem);
    }
    for comment in comments {
        add(Problem { range: comment, severity: Severity::Error, message: "Comments are not permitted in JSON.".to_owned() });
    }
    problems.sort_by_key(|problem| problem.range.start);
    problems
}


#[cfg(test)]
mod tests {
    use super::*;

    /// The problems as `(text marked, severity, message)`, by start.
    fn marked(text: &str) -> Vec<(String, Severity, String)> {
        problems(text).into_iter().map(|problem| (text[problem.range].to_owned(), problem.severity, problem.message)).collect()
    }

    fn error(at: &str, message: &str) -> (String, Severity, String) {
        (at.to_owned(), Severity::Error, message.to_owned())
    }

    fn warning(at: &str, message: &str) -> (String, Severity, String) {
        (at.to_owned(), Severity::Warning, message.to_owned())
    }

    #[test]
    fn valid_json_has_no_problems() {
        let text = "{\n  \"a\": [1, -2.5e+3, true, false, null, \"\\u00e9\\n\"],\n  \"b\": {\"//\": 1, \"//\": 2}\n}\n";
        assert_eq!(marked(text), []);
        assert_eq!(marked(""), []);
        assert_eq!(marked("  \n"), []);
    }

    /// Monaco's own diagnostics for each of these (checked with vscode-json-languageservice
    /// as Monaco's worker sets it up).
    #[test]
    fn syntax_errors_are_marked_as_monaco_marks_them() {
        assert_eq!(marked("{\"a\": 1,, \"b\" 2 // c\n}"), [
            error(",", "Property expected"),
            error("2", "Colon expected"),
            error("// c", "Comments are not permitted in JSON."),
        ]);
        assert_eq!(marked("[1,2,]"), [error(",", "Trailing comma")]);
        assert_eq!(marked("[\"x\" // c\n,]"), [
            error("// c", "Comments are not permitted in JSON."),
            error(",", "Trailing comma"),
        ]);
        assert_eq!(marked("{\"a\":1,,}"), [error(",", "Property expected")]);
        assert_eq!(marked("{,}"), [error(",", "Property expected")]);
        assert_eq!(marked("[,1]"), [error(",", "Value expected")]);
        assert_eq!(marked("{\"a\": 1 \"b\": 2}"), [error("\"b\"", "Expected comma")]);
        assert_eq!(marked("[01]"), [error("1", "Expected comma")]);
        // At the end of the text, the last character before it is marked.
        assert_eq!(marked("{\n  \"a\": 1\n\n"), [error("1", "Expected comma or closing brace")]);
        // One problem per start: the missing bracket would be marked at the same comma.
        assert_eq!(marked("[1,  "), [error(",", "Value expected")]);
        assert_eq!(marked("{\"a\": }"), [error("}", "Value expected")]);
        assert_eq!(marked("[-]"), [error("-", "Value expected")]);
        // A key followed by a string on the next line is taken as a property of its own.
        assert_eq!(marked("{\"a\" \n \"b\": 1}"), [error("\"b\"", "Colon expected")]);
        assert_eq!(marked("{} {}"), [error("{", "End of file expected.")]);
        assert_eq!(marked("]"), [error("]", "Expected a JSON object, array or literal.")]);
        assert_eq!(marked("nul"), [error("nul", "Expected a JSON object, array or literal.")]);
        assert_eq!(marked("[1.]"), [error("1.", "Unexpected end of number.")]);
        assert_eq!(marked("[1e+]"), [error("1e+", "Unexpected end of number.")]);
        assert_eq!(marked("[\"a\\q\"]"), [error("\"a\\q\"", "Invalid escape character in string.")]);
        assert_eq!(marked("[\"\\u12\"]"), [error("\"\\u12\"", "Invalid unicode sequence in string.")]);
        assert_eq!(marked("[\"a\tb\"]"), [error("\"a\tb\"", "Invalid characters in string. Control characters must be escaped.")]);
        assert_eq!(marked("[\"ab\n]"), [error("\"ab", "Unexpected end of string.")]);
        assert_eq!(marked("[1] /* x"), [
            error("/* x", "Unexpected end of comment."),
            error("/* x", "Comments are not permitted in JSON."),
        ]);
        assert_eq!(marked("/*"), [
            error("/*", "Unexpected end of comment."),
            error("/*", "Comments are not permitted in JSON."),
        ]);
    }

    #[test]
    fn duplicate_keys_are_warnings_on_each_occurrence() {
        assert_eq!(marked("{\"a\":1,\"a\":2,\"a\":3}"), [
            warning("\"a\"", "Duplicate object key"),
            warning("\"a\"", "Duplicate object key"),
            warning("\"a\"", "Duplicate object key"),
        ]);
        let ranges: Vec<Range<usize>> = problems("{\"a\":1,\"a\":2,\"a\":3}").into_iter().map(|p| p.range).collect();
        assert_eq!(ranges, [1..4, 7..10, 13..16]);
        // Escapes are read before keys are compared.
        assert_eq!(marked("{\"\\u0061\":1,\"a\":2}"), [
            warning("\"\\u0061\"", "Duplicate object key"),
            warning("\"a\"", "Duplicate object key"),
        ]);
        // Unquoted keys too; one problem per start, so the second `a` is marked once.
        assert_eq!(marked("{a:1, a:2}"), [
            error("a", "Property keys must be doublequoted"),
            warning("a", "Duplicate object key"),
            error("a", "Property keys must be doublequoted"),
        ]);
    }

    #[test]
    fn a_schema_that_cannot_be_loaded_is_a_warning() {
        let text = "{\n  \"$schema\": \"https://json.schemastore.org/tsconfig\",\n  \"a\": 1\n}";
        assert_eq!(marked(text), [warning(
            "\"https://json.schemastore.org/tsconfig\"",
            "Unable to load schema from 'https://json.schemastore.org/tsconfig'. No schema request service available"
        )]);
        // Not the first property: the root's first character.
        assert_eq!(marked("{\"a\":1, \"$schema\": \"x\"}"), [warning(
            "{",
            "Unable to load schema from 'inmemory://model/x'. No schema request service available"
        )]);
        // The first property is `$schema` but not a string: its value, or itself without one.
        let schema_warning = |text: &str| {
            problems(text).into_iter().find(|problem| problem.message.starts_with("Unable")).map(|problem| problem.range)
        };
        assert_eq!(schema_warning("{\"$schema\": {\n}, \"$schema\": \"x\"}"), Some(12..15));
        assert_eq!(schema_warning("{\"$schema\": , \"$schema\": \"x\"}"), Some(1..13));
        assert_eq!(marked("{\"$schema\": \"http://json-schema.org/draft-07/schema#\"}"), []);
        assert_eq!(marked("{\"$schema\": 3}"), []);
        assert_eq!(marked("[{\"$schema\": \"x\"}]"), []);
    }

    #[test]
    fn values_nested_deeper_than_the_worker_reads_mark_nothing() {
        use crate::code_intel::on_background_stack;
        // JSONTestSuite's n_structure_100000_opening_arrays.json, and objects as deep.
        assert_eq!(on_background_stack(|| problems(&"[".repeat(100_000))), []);
        assert_eq!(on_background_stack(|| problems(&"{\"a\":".repeat(100_000))), []);
        // As deep as it reads, on the same stack: marked (as Monaco marks it).
        let nested = |open: &str, close: &str, depth: usize| format!("{}1,{}", open.repeat(depth), close.repeat(depth));
        let arrays = nested("[", "]", MAX_NESTING);
        assert_eq!(on_background_stack(move || marked(&arrays)), [error(",", "Trailing comma")]);
        let objects = nested("{\"a\":", "}", MAX_NESTING);
        assert_eq!(on_background_stack(move || marked(&objects)), [error(",", "Trailing comma")]);
        // One level deeper: nothing.
        let deeper = [nested("[", "]", MAX_NESTING + 1), nested("{\"a\":[", "]}", MAX_NESTING / 2 + 1)];
        assert_eq!(on_background_stack(move || deeper.map(|text| problems(&text))), [vec![], vec![]]);
    }

    #[test]
    fn a_schema_is_named_as_the_schema_service_names_it() {
        let named = |id: &str| schema_display(&schema_id(id));
        assert_eq!(named("./local.json"), "inmemory://model/local.json");
        assert_eq!(named("../up.json"), "inmemory://model/up.json");
        assert_eq!(named("/abs/x.json"), "inmemory://model/abs/x.json");
        assert_eq!(named(""), "inmemory://model/");
        assert_eq!(named("#/def"), "inmemory://model/%23/def");
        assert_eq!(named("a b"), "inmemory://model/a b");
        assert_eq!(named("HTTPS://Example.com/A B.json"), "HTTPS://example.com/A B.json");
        assert_eq!(named("https://x.org/a%20b.json#"), "https://x.org/a b.json");
        assert_eq!(named("file:///x/y%20z.json"), "/x/y z.json");
        assert_eq!(named("x:y"), "x:y");
        assert_eq!(named("http:so:schema.org/s#"), "http:/so:schema.org/s");
        // A drive letter in lower case, an escape that spells no text left as it is, a host
        // as a UNC path.
        assert_eq!(named("file:///C:/x.json"), "c:/x.json");
        assert_eq!(named("file:///a%FF%41.json"), "/a%FFA.json");
        assert_eq!(named("file://host/a/../b.json"), "//host/a/../b.json");
        assert_eq!(named("https://x.org/s?a?b"), "https://x.org/s?a%3Fb");
    }
}
