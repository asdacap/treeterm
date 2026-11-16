//! vscode-html-languageservice's scanner (`parser/htmlScanner.js`), ported: the tokens of an
//! HTML document from a byte offset and a state. Offsets are bytes; the scanner reads the
//! characters JavaScript's regular expressions do (`\w` and `\d` are ASCII, `\s` is
//! JavaScript's whitespace), and where it steps over one character it steps over it whole.

use std::sync::LazyLock;

use regex::Regex;

/// The scanner's tokens (`TokenType`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Token {
    StartCommentTag,
    Comment,
    EndCommentTag,
    StartTagOpen,
    StartTagClose,
    StartTagSelfClose,
    StartTag,
    EndTagOpen,
    EndTagClose,
    EndTag,
    DelimiterAssign,
    AttributeName,
    AttributeValue,
    StartDoctypeTag,
    Doctype,
    EndDoctypeTag,
    Content,
    Whitespace,
    Unknown,
    Script,
    Styles,
    Eos,
}

/// Where the scanner is (`ScannerState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum State {
    WithinContent,
    AfterOpeningStartTag,
    AfterOpeningEndTag,
    WithinDoctype,
    WithinTag,
    WithinEndTag,
    WithinComment,
    WithinScriptContent,
    WithinStyleContent,
    AfterAttributeName,
    BeforeAttributeValue,
}

/// JavaScript's `\s` as a character class: Unicode whitespace but U+0085, and the byte order
/// mark.
const JS_SPACE: &str = r"\t\n\x0B\x0C\r \u{a0}\u{1680}\u{2000}-\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";

/// Whether `c` is JavaScript's `\s`.
pub(crate) fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

fn regex(source: &str) -> Regex {
    Regex::new(source).unwrap_or_else(|error| panic!("scanner pattern {source}: {error}"))
}

/// `/<!--|-->|<\/?script\s*\/?>?/i`, its letters matched in ASCII case only, as JavaScript's
/// `i` flag matches them.
static SCRIPT_TOKEN: LazyLock<Regex> =
    LazyLock::new(|| regex(&format!(r"<!--|-->|</?(?-u:(?i)script)[{JS_SPACE}]*/?>?")));
/// `/<\/style/i`.
static STYLE_END: LazyLock<Regex> = LazyLock::new(|| regex(r"</(?-u:(?i)style)"));
/// `/^[^\s"'`=<>]+/`: an unquoted attribute value.
static UNQUOTED_VALUE: LazyLock<Regex> = LazyLock::new(|| regex(&format!(r#"^[^{JS_SPACE}"'`=<>]+"#)));

/// The scanner (`createScanner`).
pub(crate) struct Scanner<'a> {
    text: &'a str,
    pos: usize,
    state: State,
    token_offset: usize,
    token: Token,
    has_space_after_tag: bool,
    last_tag: String,
    last_attribute_name: String,
    last_type_value: Option<String>,
    /// The parser's: a start or end tag cut short by a `<` closes with an empty token.
    emit_pseudo_close_tags: bool,
}

impl<'a> Scanner<'a> {
    pub(crate) fn new(text: &'a str, offset: usize, state: State, emit_pseudo_close_tags: bool) -> Self {
        Self {
            text,
            pos: offset,
            state,
            token_offset: 0,
            token: Token::Unknown,
            has_space_after_tag: false,
            last_tag: String::new(),
            last_attribute_name: String::new(),
            last_type_value: None,
            emit_pseudo_close_tags,
        }
    }

    pub(crate) fn token(&self) -> Token {
        self.token
    }

    pub(crate) fn token_offset(&self) -> usize {
        self.token_offset
    }

    pub(crate) fn token_end(&self) -> usize {
        self.pos
    }

    pub(crate) fn token_text(&self) -> &'a str {
        &self.text[self.token_offset..self.pos]
    }

    pub(crate) fn state(&self) -> State {
        self.state
    }

    /// The next token.
    pub(crate) fn scan(&mut self) -> Token {
        let offset = self.pos;
        let token = self.internal_scan();
        let pseudo_close =
            self.emit_pseudo_close_tags && matches!(token, Token::StartTagClose | Token::EndTagClose);
        if token != Token::Eos && offset == self.pos && !pseudo_close {
            self.advance_char();
            return self.finish(offset, Token::Unknown);
        }
        token
    }

    fn finish(&mut self, offset: usize, token: Token) -> Token {
        self.token = token;
        self.token_offset = offset;
        token
    }

    fn rest(&self) -> &'a str {
        &self.text[self.pos..]
    }

    fn eos(&self) -> bool {
        self.pos >= self.text.len()
    }

    /// The byte at `pos` (`peekChar`), 0 past the end.
    fn peek(&self) -> u8 {
        self.text.as_bytes().get(self.pos).copied().unwrap_or(0)
    }

    fn advance_char(&mut self) {
        self.pos += self.rest().chars().next().map_or(1, char::len_utf8);
    }

    fn advance_if(&mut self, prefix: &str) -> bool {
        let found = self.rest().starts_with(prefix);
        if found {
            self.pos += prefix.len();
        }
        found
    }

    /// Up to `needle`, or the end (`advanceUntilChar`, `advanceUntilChars`).
    fn advance_until(&mut self, needle: &str) -> bool {
        match self.rest().find(needle) {
            Some(at) => {
                self.pos += at;
                true
            }
            None => {
                self.pos = self.text.len();
                false
            }
        }
    }

    /// Spaces, tabs, line breaks and form feeds (`skipWhitespace`).
    fn skip_whitespace(&mut self) -> bool {
        let skipped = self.rest().bytes().take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\x0C' | b'\r')).count();
        self.pos += skipped;
        skipped > 0
    }

    /// `/^[_:\w][_:\w-.\d]*/`, lower-cased.
    fn next_element_name(&mut self) -> String {
        let bytes = self.rest().as_bytes();
        let name_byte = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':';
        let length = match bytes.first() {
            Some(&byte) if name_byte(byte) => {
                1 + bytes[1..].iter().take_while(|&&byte| name_byte(byte) || byte == b'-' || byte == b'.').count()
            }
            Some(_) | None => 0,
        };
        let name = self.rest()[..length].to_ascii_lowercase();
        self.pos += length;
        name
    }

    /// `/^[^\s"'></=\x00-\x0F\x7F\x80-\x9F]*/`, lower-cased.
    fn next_attribute_name(&mut self) -> String {
        let ends = |c: char| {
            is_js_space(c) || "\"'></=".contains(c) || matches!(c, '\0'..='\x0F' | '\x7F' | '\u{80}'..='\u{9F}')
        };
        let length = self.rest().find(ends).unwrap_or(self.rest().len());
        let name = self.rest()[..length].to_lowercase();
        self.pos += length;
        name
    }

    fn internal_scan(&mut self) -> Token {
        let offset = self.pos;
        if self.eos() {
            return self.finish(offset, Token::Eos);
        }
        match self.state {
            State::WithinComment => {
                if self.advance_if("-->") {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::EndCommentTag);
                }
                self.advance_until("-->");
                self.finish(offset, Token::Comment)
            }
            State::WithinDoctype => {
                if self.advance_if(">") {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::EndDoctypeTag);
                }
                self.advance_until(">");
                self.finish(offset, Token::Doctype)
            }
            State::WithinContent => {
                if self.advance_if("<") {
                    if !self.eos() && self.peek() == b'!' {
                        if self.advance_if("!--") {
                            self.state = State::WithinComment;
                            return self.finish(offset, Token::StartCommentTag);
                        }
                        if self.rest().get(..8).is_some_and(|word| word.eq_ignore_ascii_case("!doctype")) {
                            self.pos += 8;
                            self.state = State::WithinDoctype;
                            return self.finish(offset, Token::StartDoctypeTag);
                        }
                    }
                    if self.advance_if("/") {
                        self.state = State::AfterOpeningEndTag;
                        return self.finish(offset, Token::EndTagOpen);
                    }
                    self.state = State::AfterOpeningStartTag;
                    return self.finish(offset, Token::StartTagOpen);
                }
                self.advance_until("<");
                self.finish(offset, Token::Content)
            }
            State::AfterOpeningEndTag => {
                if !self.next_element_name().is_empty() {
                    self.state = State::WithinEndTag;
                    return self.finish(offset, Token::EndTag);
                }
                if self.skip_whitespace() {
                    return self.finish(offset, Token::Whitespace);
                }
                self.state = State::WithinEndTag;
                self.advance_until(">");
                if offset < self.pos {
                    return self.finish(offset, Token::Unknown);
                }
                self.internal_scan()
            }
            State::WithinEndTag => {
                if self.skip_whitespace() {
                    return self.finish(offset, Token::Whitespace);
                }
                if self.advance_if(">") {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::EndTagClose);
                }
                if self.emit_pseudo_close_tags && self.peek() == b'<' {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::EndTagClose);
                }
                self.advance_char();
                self.state = State::WithinContent;
                self.finish(offset, Token::Unknown)
            }
            State::AfterOpeningStartTag => {
                self.last_tag = self.next_element_name();
                self.last_type_value = None;
                self.last_attribute_name = String::new();
                if !self.last_tag.is_empty() {
                    self.has_space_after_tag = false;
                    self.state = State::WithinTag;
                    return self.finish(offset, Token::StartTag);
                }
                if self.skip_whitespace() {
                    return self.finish(offset, Token::Whitespace);
                }
                self.state = State::WithinTag;
                self.advance_until(">");
                if offset < self.pos {
                    return self.finish(offset, Token::Unknown);
                }
                self.internal_scan()
            }
            State::WithinTag => {
                if self.skip_whitespace() {
                    self.has_space_after_tag = true;
                    return self.finish(offset, Token::Whitespace);
                }
                if self.has_space_after_tag {
                    self.last_attribute_name = self.next_attribute_name();
                    if !self.last_attribute_name.is_empty() {
                        self.state = State::AfterAttributeName;
                        self.has_space_after_tag = false;
                        return self.finish(offset, Token::AttributeName);
                    }
                }
                if self.advance_if("/>") {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::StartTagSelfClose);
                }
                if self.advance_if(">") {
                    self.state = match self.last_tag.as_str() {
                        "script" => match self.last_type_value.as_deref() {
                            Some("text/x-handlebars-template" | "text/html") => State::WithinContent,
                            Some(_) | None => State::WithinScriptContent,
                        },
                        "style" => State::WithinStyleContent,
                        _ => State::WithinContent,
                    };
                    return self.finish(offset, Token::StartTagClose);
                }
                if self.emit_pseudo_close_tags && self.peek() == b'<' {
                    self.state = State::WithinContent;
                    return self.finish(offset, Token::StartTagClose);
                }
                self.advance_char();
                self.finish(offset, Token::Unknown)
            }
            State::AfterAttributeName => {
                if self.skip_whitespace() {
                    self.has_space_after_tag = true;
                    return self.finish(offset, Token::Whitespace);
                }
                if self.advance_if("=") {
                    self.state = State::BeforeAttributeValue;
                    return self.finish(offset, Token::DelimiterAssign);
                }
                self.state = State::WithinTag;
                self.internal_scan()
            }
            State::BeforeAttributeValue => {
                if self.skip_whitespace() {
                    return self.finish(offset, Token::Whitespace);
                }
                if let Some(found) = UNQUOTED_VALUE.find(self.rest()) {
                    let mut value = found.as_str();
                    self.pos += value.len();
                    // `<foo bar=http://foo/>`: the `/` closes the tag.
                    if self.peek() == b'>' && value.ends_with('/') {
                        self.pos -= 1;
                        value = &value[..value.len() - 1];
                    }
                    if self.last_attribute_name == "type" {
                        self.last_type_value = Some(value.to_owned());
                    }
                    if !value.is_empty() {
                        self.state = State::WithinTag;
                        self.has_space_after_tag = false;
                        return self.finish(offset, Token::AttributeValue);
                    }
                }
                let quote = self.peek();
                if quote == b'\'' || quote == b'"' {
                    self.pos += 1;
                    let closed = self.advance_until(if quote == b'"' { "\"" } else { "'" });
                    if closed {
                        self.pos += 1;
                    }
                    if self.last_attribute_name == "type" {
                        // Without its closing quote, the value loses its last character.
                        let value = &self.text[offset + 1..self.pos];
                        let mut chars = value.chars();
                        chars.next_back();
                        self.last_type_value = Some(chars.as_str().to_owned());
                    }
                    self.state = State::WithinTag;
                    self.has_space_after_tag = false;
                    return self.finish(offset, Token::AttributeValue);
                }
                self.state = State::WithinTag;
                self.has_space_after_tag = false;
                self.internal_scan()
            }
            State::WithinScriptContent => {
                // See http://stackoverflow.com/questions/14574471/how-do-browsers-parse-a-script-tag-exactly
                let mut script_state = 1;
                while !self.eos() {
                    let Some(found) = SCRIPT_TOKEN.find(self.rest()) else {
                        self.pos = self.text.len();
                        return self.finish(offset, Token::Script);
                    };
                    let matched = found.as_str();
                    self.pos += found.end();
                    if matched == "<!--" {
                        if script_state == 1 {
                            script_state = 2;
                        }
                    } else if matched == "-->" {
                        script_state = 1;
                    } else if matched.as_bytes()[1] != b'/' {
                        if script_state == 2 {
                            script_state = 3;
                        }
                    } else if script_state == 3 {
                        script_state = 2;
                    } else {
                        // Back to the start of the end tag.
                        self.pos -= matched.len();
                        break;
                    }
                }
                self.state = State::WithinContent;
                if offset < self.pos {
                    return self.finish(offset, Token::Script);
                }
                self.internal_scan()
            }
            State::WithinStyleContent => {
                self.pos = STYLE_END.find(self.rest()).map_or(self.text.len(), |found| self.pos + found.start());
                self.state = State::WithinContent;
                if offset < self.pos {
                    return self.finish(offset, Token::Styles);
                }
                self.internal_scan()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every token of `text` as `(token, its text)`, as the language service's scanner reads them.
    fn tokens(text: &str) -> Vec<(Token, &str)> {
        let mut scanner = Scanner::new(text, 0, State::WithinContent, false);
        let mut found = Vec::new();
        while scanner.scan() != Token::Eos {
            found.push((scanner.token(), scanner.token_text()));
        }
        found
    }

    #[test]
    fn tags_attributes_and_values_are_read() {
        use Token::*;
        assert_eq!(tokens("<A href=x title='y' hidden/>"), [
            (StartTagOpen, "<"),
            (StartTag, "A"),
            (Whitespace, " "),
            (AttributeName, "href"),
            (DelimiterAssign, "="),
            (AttributeValue, "x"),
            (Whitespace, " "),
            (AttributeName, "title"),
            (DelimiterAssign, "="),
            (AttributeValue, "'y'"),
            (Whitespace, " "),
            (AttributeName, "hidden"),
            (StartTagSelfClose, "/>"),
        ]);
        assert_eq!(tokens("<a href=x/>"), [
            (StartTagOpen, "<"),
            (StartTag, "a"),
            (Whitespace, " "),
            (AttributeName, "href"),
            (DelimiterAssign, "="),
            (AttributeValue, "x"),
            (StartTagSelfClose, "/>"),
        ]);
        // No attribute right after the tag name or a value.
        assert_eq!(tokens("<a\u{a0}b>")[2], (Unknown, "\u{a0}"));
        assert_eq!(tokens("<a b=\"c\"d>")[6], (Unknown, "d"));
    }

    #[test]
    fn comments_doctypes_and_end_tags_are_read() {
        use Token::*;
        assert_eq!(tokens("<!DOCTYPE html><!-- a --></p >"), [
            (StartDoctypeTag, "<!DOCTYPE"),
            (Doctype, " html"),
            (EndDoctypeTag, ">"),
            (StartCommentTag, "<!--"),
            (Comment, " a "),
            (EndCommentTag, "-->"),
            (EndTagOpen, "</"),
            (EndTag, "p"),
            (Whitespace, " "),
            (EndTagClose, ">"),
        ]);
    }

    #[test]
    fn script_and_style_hold_text_up_to_their_end_tag() {
        use Token::*;
        let script = "<script>a < b; '</scrip'</SCRIPT >";
        assert_eq!(tokens(script)[3..], [
            (Script, "a < b; '</scrip'"),
            (EndTagOpen, "</"),
            (EndTag, "SCRIPT"),
            (Whitespace, " "),
            (EndTagClose, ">"),
        ]);
        // `<!-- <script>` hides an end tag.
        let hidden = "<script><!-- <script></script> --></script>";
        assert_eq!(tokens(hidden)[3], (Script, "<!-- <script></script> -->"));
        assert_eq!(tokens("<style>a { }</style>")[3], (Styles, "a { }"));
        // A handlebars template is HTML.
        assert_eq!(tokens("<script type=\"text/html\"><b>")[7], (StartTagOpen, "<"));
        // `ſ` is not `s` in ASCII case.
        assert_eq!(tokens("<script>x</ſcript>").len(), 4);
    }

    #[test]
    fn pseudo_close_tags_close_a_tag_cut_short_by_a_bracket() {
        let mut scanner = Scanner::new("<a <b>", 0, State::WithinContent, true);
        let found: Vec<Token> = std::iter::from_fn(|| Some(scanner.scan()).filter(|token| *token != Token::Eos)).collect();
        assert_eq!(found, [
            Token::StartTagOpen,
            Token::StartTag,
            Token::Whitespace,
            Token::StartTagClose,
            Token::StartTagOpen,
            Token::StartTag,
            Token::StartTagClose,
        ]);
    }
}
