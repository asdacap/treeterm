//! The links Monaco's base editor worker finds in every file (`LinkComputer`, ported from
//! Monaco 0.55.1): `http://`, `https://` and `file://` URLs, up to a space, a quote or
//! bracket that does not belong to them, or one of the CJK punctuation marks; never ending
//! in `.`, `,`, `;` or `:`, nor in the `)`, `]` or `}` closing a bracket opened before
//! them.

use std::ops::Range;

/// A link: its bytes in the text, and the URL they spell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub range: Range<usize>,
    pub url: String,
}

/// The link computer's states (`State`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Invalid,
    Start,
    H,
    Ht,
    Htt,
    Http,
    F,
    Fi,
    Fil,
    BeforeColon,
    AfterColon,
    AlmostThere,
    End,
    Accept,
}

/// The scheme state machine: `http://`, `https://` or `file://`, in any case.
fn next_state(state: State, c: char) -> State {
    match (state, c.to_ascii_lowercase()) {
        (State::Start, 'h') => State::H,
        (State::Start, 'f') => State::F,
        (State::H, 't') => State::Ht,
        (State::Ht, 't') => State::Htt,
        (State::Htt, 'p') => State::Http,
        (State::Http, 's') => State::BeforeColon,
        (State::Http, ':') => State::AfterColon,
        (State::F, 'i') => State::Fi,
        (State::Fi, 'l') => State::Fil,
        (State::Fil, 'e') => State::BeforeColon,
        (State::BeforeColon, ':') => State::AfterColon,
        (State::AfterColon, '/') => State::AlmostThere,
        (State::AlmostThere, '/') => State::End,
        _ => State::Invalid,
    }
}

/// How a character ends a link (`CharacterClass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Class {
    None,
    ForceTermination,
    CannotEndIn,
}

fn classify(c: char) -> Class {
    const FORCE_TERMINATION: &str = " \t<>'\"、。｡､，．：；‘〈「『〔（［｛｢｣｝］）〕』」〉’｀～…|";
    if FORCE_TERMINATION.contains(c) {
        Class::ForceTermination
    } else if ".,;:".contains(c) {
        Class::CannotEndIn
    } else {
        Class::None
    }
}

/// `_createLink`: the link from character `begin` to before character `end` of `line`
/// (`chars`, each with its byte), dropping the characters it cannot end in.
fn create_link(line: &str, chars: &[(usize, char)], line_start: usize, begin: usize, end: usize) -> Link {
    let mut last = end - 1;
    loop {
        if classify(chars[last].1) != Class::CannotEndIn {
            break;
        }
        last -= 1;
        if last <= begin {
            break;
        }
    }
    // A link in brackets does not end with the closing one.
    if begin > 0 {
        let before = chars[begin - 1].1;
        let last_char = chars[last].1;
        if matches!((before, last_char), ('(', ')') | ('[', ']') | ('{', '}')) {
            last -= 1;
        }
    }
    let start = chars[begin].0;
    let end = chars.get(last + 1).map_or(line.len(), |(byte, _)| *byte);
    Link { range: line_start + start..line_start + end, url: line[start..end].to_owned() }
}

/// The links in `line`, which starts at byte `line_start` of the text.
fn line_links(line: &str, line_start: usize, links: &mut Vec<Link>) {
    let chars: Vec<(usize, char)> = line.char_indices().collect();
    let mut state = State::Start;
    let mut begin = 0;
    let mut begin_char = '\0';
    let (mut open_paren, mut open_square, mut in_square, mut open_curly) = (false, false, false, false);
    for (index, &(_, c)) in chars.iter().enumerate() {
        let mut reset = false;
        match state {
            State::Accept => {
                let class = match c {
                    '(' => {
                        open_paren = true;
                        Class::None
                    }
                    ')' if open_paren => Class::None,
                    '[' => {
                        in_square = true;
                        open_square = true;
                        Class::None
                    }
                    ']' => {
                        in_square = false;
                        if open_square { Class::None } else { Class::ForceTermination }
                    }
                    '{' => {
                        open_curly = true;
                        Class::None
                    }
                    '}' if open_curly => Class::None,
                    ')' | '}' => Class::ForceTermination,
                    // A quote is part of a link only when another kind of quote began it.
                    '\'' | '"' | '`' => {
                        if begin_char == c {
                            Class::ForceTermination
                        } else if matches!(begin_char, '\'' | '"' | '`') {
                            Class::None
                        } else {
                            Class::ForceTermination
                        }
                    }
                    '*' if begin_char == '*' => Class::ForceTermination,
                    '*' => Class::None,
                    ' ' if in_square => Class::None,
                    ' ' => Class::ForceTermination,
                    c => classify(c),
                };
                if class == Class::ForceTermination {
                    links.push(create_link(line, &chars, line_start, begin, index));
                    reset = true;
                }
            }
            State::End => {
                // The authority may be an IPv6 address in brackets.
                let class = if c == '[' {
                    open_square = true;
                    Class::None
                } else {
                    classify(c)
                };
                if class == Class::ForceTermination {
                    reset = true;
                } else {
                    state = State::Accept;
                }
            }
            _ => {
                state = next_state(state, c);
                if state == State::Invalid {
                    reset = true;
                }
            }
        }
        if reset {
            state = State::Start;
            open_paren = false;
            open_square = false;
            open_curly = false;
            begin = index + 1;
            begin_char = c;
        }
    }
    if state == State::Accept {
        links.push(create_link(line, &chars, line_start, begin, chars.len()));
    }
}

/// The links in `text`, by start.
pub fn links(text: &str) -> Vec<Link> {
    let mut found = Vec::new();
    let mut line_start = 0;
    for line in text.split('\n') {
        line_links(line.strip_suffix('\r').unwrap_or(line), line_start, &mut found);
        line_start += line.len() + 1;
    }
    found
}

/// The link at byte `offset` of `text`, touching it on either side.
pub fn link_at(text: &str, offset: usize) -> Option<Link> {
    let line_start = text[..offset.min(text.len())].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[line_start..].find('\n').map_or(text.len(), |index| line_start + index);
    let line = &text[line_start..line_end];
    let mut found = Vec::new();
    line_links(line.strip_suffix('\r').unwrap_or(line), line_start, &mut found);
    found.into_iter().find(|link| link.range.start <= offset && offset <= link.range.end)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn urls(text: &str) -> Vec<&str> {
        links(text).into_iter().map(|link| &text[link.range]).collect()
    }

    /// Monaco's own `linkComputer.test.ts` cases, and its results.
    #[test]
    fn links_end_where_monacos_do() {
        assert_eq!(urls("x = http://foo.bar"), ["http://foo.bar"]);
        assert_eq!(urls("x = (http://foo.bar)"), ["http://foo.bar"]);
        assert_eq!(urls("x = [http://foo.bar]"), ["http://foo.bar"]);
        assert_eq!(urls("x = 'http://foo.bar'"), ["http://foo.bar"]);
        assert_eq!(urls("x =  http://foo.bar "), ["http://foo.bar"]);
        assert_eq!(urls("x = <http://foo.bar>"), ["http://foo.bar"]);
        assert_eq!(urls("x = {http://foo.bar}"), ["http://foo.bar"]);
        assert_eq!(urls("(see http://foo.bar)"), ["http://foo.bar"]);
        assert_eq!(urls("[see http://foo.bar]"), ["http://foo.bar"]);
        assert_eq!(urls("{see http://foo.bar}"), ["http://foo.bar"]);
        assert_eq!(urls("<see http://foo.bar>"), ["http://foo.bar"]);
        assert_eq!(urls("<url>http://mylink.com</url>"), ["http://mylink.com"]);
        assert_eq!(urls("// Click here to learn more. https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409"), [
            "https://go.microsoft.com/fwlink/?LinkID=513275&clcid=0x409"
        ]);
        assert_eq!(urls("// Click here to learn more. https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx"), [
            "https://msdn.microsoft.com/en-us/library/windows/desktop/aa365247(v=vs.85).aspx"
        ]);
        assert_eq!(urls("// https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js"), [
            "https://github.com/projectkudu/kudu/blob/master/Kudu.Core/Scripts/selectNodeVersion.js"
        ]);
        assert_eq!(urls("<!-- !!! Do not remove !!!   WebContentRef(link:https://go.microsoft.com/fwlink/?LinkId=166007, area:Admin, updated:2015, nextUpdate:2016, tags:SqlServer)   !!! Do not remove !!! -->"), [
            "https://go.microsoft.com/fwlink/?LinkId=166007"
        ]);
        assert_eq!(urls("For instructions, see https://go.microsoft.com/fwlink/?LinkId=166007.</value>"), [
            "https://go.microsoft.com/fwlink/?LinkId=166007"
        ]);
        assert_eq!(urls("x = \"file:///foo.bar\";"), ["file:///foo.bar"]);
        assert_eq!(urls("x = \"https://en.wikipedia.org/wiki/Zürich\";"), ["https://en.wikipedia.org/wiki/Zürich"]);
        assert_eq!(urls("請參閱 http://go.microsoft.com/fwlink/?LinkId=761051。"), ["http://go.microsoft.com/fwlink/?LinkId=761051"]);
        assert_eq!(urls("x = \"file://shares/foo.bar\";"), ["file://shares/foo.bar"]);
        assert_eq!(urls("Some text, then http://www.bing.com."), ["http://www.bing.com"]);
        assert_eq!(urls("let url = `http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items`;"), [
            "http://***/_api/web/lists/GetByTitle('Teambuildingaanvragen')/items"
        ]);
        assert_eq!(urls("x = \"https://[2001:db8::1]:8080/path\""), ["https://[2001:db8::1]:8080/path"]);
        assert_eq!(urls("x = **http://foo.bar**"), ["http://foo.bar"]);
        assert_eq!(urls("[label](http://example.com/a(b))"), ["http://example.com/a(b)"]);
        assert_eq!(urls("no scheme: www.example.com, ftp://x.y"), Vec::<&str>::new());
    }

    #[test]
    fn a_link_is_found_at_a_byte_on_its_line() {
        let text = "one\n  see https://a.b/c.\r\nthree";
        let link = link_at(text, 12).expect("a link");
        assert_eq!((link.url.as_str(), &text[link.range.clone()]), ("https://a.b/c", "https://a.b/c"));
        // Touching either end.
        assert!(link_at(text, link.range.start).is_some());
        assert!(link_at(text, link.range.end).is_some());
        assert_eq!(link_at(text, 2), None);
        assert_eq!(links(text).len(), 1);
    }
}
