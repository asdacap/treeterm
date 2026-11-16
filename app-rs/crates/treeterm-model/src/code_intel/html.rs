//! The completions of Monaco's HTML worker (vscode-html-languageservice's `doComplete`) for
//! element and attribute names: after `<` the elements (and the end tag of the element the
//! cursor is in), after `</` that end tag, and in a start tag the element's attributes and
//! the global ones it does not have yet. Attribute values and character entities are not
//! completed.

use std::ops::Range;

use super::web_data::{HTML_GLOBAL_ATTRIBUTES, HTML_TAGS, HtmlAttribute};
use super::{Completion, Completions};
use crate::words::{self, Trigger};

/// `nextElementName`: `[_:\w][_:\w\-.\d]*`.
fn element_name_len(text: &str, at: usize) -> usize {
    let bytes = &text.as_bytes()[at.min(text.len())..];
    let first = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b':';
    match bytes.first() {
        Some(&byte) if first(byte) => {
            1 + bytes[1..].iter().take_while(|&&byte| first(byte) || byte == b'-' || byte == b'.').count()
        }
        Some(_) | None => 0,
    }
}

/// JavaScript's `\s`: Unicode whitespace (but U+0085) and the byte order mark.
fn is_js_space(c: char) -> bool {
    (c.is_whitespace() && c != '\u{85}') || c == '\u{feff}'
}

/// `nextAttributeName`: `[^\s"'></=\x00-\x0F\x7F\x80-\x9F]*`.
fn attribute_name_len(text: &str, at: usize) -> usize {
    let ends = |c: char| is_js_space(c) || "\"'></=".contains(c) || matches!(c, '\0'..='\x0F' | '\x7F' | '\u{80}'..='\u{9F}');
    text[at..].find(ends).unwrap_or(text.len() - at)
}

/// An unquoted attribute value: `[^\s"'`=<>]+`.
fn unquoted_value_len(text: &str, at: usize) -> usize {
    text[at..].find(|c: char| is_js_space(c) || "\"'`=<>".contains(c)).unwrap_or(text.len() - at)
}

/// Past the character at byte `at` of `text`: the scanner steps over a character it cannot
/// read in a tag (Unicode whitespace such as a no-break or ideographic space, a stray quote)
/// whole.
fn past_char(text: &str, at: usize) -> usize {
    at + text[at..].chars().next().map_or(1, char::len_utf8)
}

fn is_void(tag: &str) -> bool {
    HTML_TAGS.iter().any(|(name, void, _)| *void && name.eq_ignore_ascii_case(tag))
}

/// Where in the markup the cursor is, for completing.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Place {
    /// An element name after `<` (`replace` from after it); `bare` when nothing follows the
    /// `<` yet, where the end tag of the open element is offered too.
    StartTag { replace: Range<usize>, bare: bool },
    /// An end tag after `</` (`replace` from the `/`).
    EndTag { replace: Range<usize> },
    /// An attribute name in a start tag of `tag`, which has `others` besides the one typed.
    Attribute { tag: String, replace: Range<usize>, others: Vec<String> },
}

/// Where the HTML scanner is in a start tag: its `WithinTag`, `AfterAttributeName` and
/// `BeforeAttributeValue` states. `spaced` is its `hasSpaceAfterTag`: an attribute name
/// starts only after (ASCII) whitespace, not right after the tag name or a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TagState {
    Within { spaced: bool },
    /// `=` gives the attribute a value.
    AfterName { spaced: bool },
    BeforeValue,
}

/// What the cursor is at in a start tag: the first token it is in or just after, as
/// `doComplete` reads them.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TagCursor {
    /// Past the tag.
    Past,
    /// An attribute name being typed, or none yet after whitespace.
    AttributeName(Range<usize>),
    /// An attribute value, or a character the scanner cannot read in a tag ("Unexpected
    /// character in tag."): no names are completed there.
    Elsewhere,
}

/// What a start tag holds, read from its name to its end.
struct StartTag {
    attributes: Vec<(Range<usize>, String)>,
    /// Just past its `>` (or where it stops without one).
    end: usize,
    self_closing: bool,
    cursor: TagCursor,
}

impl StartTag {
    /// The scanner read the token `range`: it holds the cursor (byte `offset`) unless an
    /// earlier one did.
    fn token(&mut self, range: Range<usize>, offset: usize, cursor: TagCursor) {
        if self.cursor == TagCursor::Past && range.start <= offset && offset <= range.end {
            self.cursor = cursor;
        }
    }
}

/// The start tag from `pos`, just past its name, as vscode-html-languageservice's scanner
/// reads it.
fn read_start_tag(text: &str, mut pos: usize, offset: usize) -> StartTag {
    let bytes = text.as_bytes();
    let mut tag = StartTag { attributes: Vec::new(), end: text.len(), self_closing: false, cursor: TagCursor::Past };
    let mut state = TagState::Within { spaced: false };
    loop {
        let start = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if start < pos {
            let cursor = match state {
                TagState::Within { .. } => {
                    state = TagState::Within { spaced: true };
                    TagCursor::AttributeName(offset..offset)
                }
                TagState::AfterName { .. } => {
                    state = TagState::AfterName { spaced: true };
                    TagCursor::AttributeName(offset..offset)
                }
                TagState::BeforeValue => TagCursor::Elsewhere,
            };
            tag.token(start..pos, offset, cursor);
            continue;
        }
        let Some(&byte) = bytes.get(pos) else {
            return tag;
        };
        match state {
            TagState::AfterName { spaced } => {
                if byte == b'=' {
                    pos += 1;
                    tag.token(start..pos, offset, TagCursor::Elsewhere);
                    state = TagState::BeforeValue;
                } else {
                    state = TagState::Within { spaced };
                }
            }
            TagState::BeforeValue => {
                let mut length = unquoted_value_len(text, pos);
                // `<a href=x/>`: the `/` closes the tag.
                if length > 0 && bytes.get(pos + length) == Some(&b'>') && bytes[pos + length - 1] == b'/' {
                    length -= 1;
                }
                if length > 0 {
                    pos += length;
                    tag.token(start..pos, offset, TagCursor::Elsewhere);
                } else if byte == b'"' || byte == b'\'' {
                    pos = text[pos + 1..].find(byte as char).map_or(text.len(), |end| pos + 1 + end + 1);
                    tag.token(start..pos, offset, TagCursor::Elsewhere);
                }
                state = TagState::Within { spaced: false };
            }
            TagState::Within { spaced } => {
                let length = if spaced { attribute_name_len(text, pos) } else { 0 };
                if length > 0 {
                    let name = pos..pos + length;
                    tag.token(name.clone(), offset, TagCursor::AttributeName(name.clone()));
                    tag.attributes.push((name.clone(), text[name.clone()].to_owned()));
                    pos = name.end;
                    state = TagState::AfterName { spaced: false };
                    continue;
                }
                match byte {
                    b'/' if bytes.get(pos + 1) == Some(&b'>') => {
                        tag.end = pos + 2;
                        tag.self_closing = true;
                        return tag;
                    }
                    b'>' => {
                        tag.end = pos + 1;
                        return tag;
                    }
                    // A new tag: this one was never closed.
                    b'<' => {
                        tag.end = pos;
                        return tag;
                    }
                    _ => {
                        pos = past_char(text, pos);
                        tag.token(start..pos, offset, TagCursor::Elsewhere);
                    }
                }
            }
        }
    }
}

/// Where the cursor (byte `offset`) is, and the elements open there, innermost last; `None`
/// where nothing is completed (text, comments, attribute values, script and style).
fn place(text: &str, offset: usize) -> Option<(Place, Vec<String>)> {
    let bytes = text.as_bytes();
    let mut open: Vec<String> = Vec::new();
    let mut pos = 0;
    while pos < offset {
        if bytes[pos] != b'<' {
            pos += 1;
            continue;
        }
        let rest = &text[pos..];
        if let Some(comment) = rest.strip_prefix("<!--") {
            let end = pos + 4 + comment.find("-->").map_or(text.len() - pos - 4, |end| end + 3);
            if end >= offset {
                return None;
            }
            pos = end;
            continue;
        }
        if rest.starts_with("</") {
            let name_start = pos + 2;
            let name_end = name_start + element_name_len(text, name_start);
            if offset <= name_end {
                return Some((Place::EndTag { replace: pos + 1..name_end }, open));
            }
            let close = text[name_end..].find('>').map_or(text.len(), |end| name_end + end);
            if close >= offset {
                return None;
            }
            // The element it ends, and those in it left open.
            let name = text[name_start..name_end].to_lowercase();
            if let Some(index) = open.iter().rposition(|tag| *tag == name) {
                open.truncate(index);
            }
            pos = close + 1;
            continue;
        }
        if rest.starts_with("<!") || rest.starts_with("<?") {
            let close = text[pos..].find('>').map_or(text.len(), |end| pos + end);
            if close >= offset {
                return None;
            }
            pos = close + 1;
            continue;
        }
        let name_start = pos + 1;
        let name_end = name_start + element_name_len(text, name_start);
        if offset <= name_end {
            let bare = name_start == name_end;
            if bare && offset != name_start {
                pos += 1;
                continue;
            }
            return Some((Place::StartTag { replace: name_start..name_end, bare }, open));
        }
        if name_start == name_end {
            pos += 1;
            continue;
        }
        let name = text[name_start..name_end].to_lowercase();
        let tag = read_start_tag(text, name_end, offset);
        if tag.end < offset || (tag.end == offset && text[..offset].ends_with('>')) {
            if !tag.self_closing && !is_void(&name) {
                open.push(name.clone());
            }
            pos = tag.end;
            // Script and style hold text up to their end tag (in any ASCII case: lowering the
            // case of other characters may change their length).
            if name == "script" || name == "style" {
                let end_tag = format!("</{name}");
                let content_end = text[pos..].to_ascii_lowercase().find(&end_tag).map_or(text.len(), |end| pos + end);
                if content_end >= offset {
                    return None;
                }
                pos = content_end;
            }
            continue;
        }
        let TagCursor::AttributeName(replace) = tag.cursor else {
            return None;
        };
        let others = tag.attributes.into_iter().filter(|(range, _)| *range != replace).map(|(_, name)| name).collect();
        return Some((Place::Attribute { tag: name, replace, others }, open));
    }
    None
}

/// Whether `text` goes on (after spaces) with `next`.
fn followed_by(text: &str, at: usize, next: char) -> bool {
    text[at..].trim_start().starts_with(next)
}

/// The completions Monaco's HTML worker offers at byte `offset` of `text`, an `html` file;
/// `None` where it offers none (or nothing was typed yet while typing, but after `<` and
/// `</`), where Monaco's word suggestions take over.
pub fn completions(text: &str, offset: usize, trigger: Trigger) -> Option<Completions> {
    let (place, open) = place(text, offset)?;
    let end_tag = |name: &str, replace_end: usize| {
        let close = if followed_by(text, replace_end, '>') { "" } else { ">" };
        Completion { label: format!("/{name}"), insert: format!("/{name}{close}") }
    };
    let (replace, items): (Range<usize>, Vec<Completion>) = match place {
        Place::StartTag { replace, bare } => {
            let mut items: Vec<Completion> =
                HTML_TAGS.iter().map(|(name, _, _)| Completion { label: (*name).to_owned(), insert: (*name).to_owned() }).collect();
            if bare {
                items.extend(open.last().map(|tag| end_tag(tag, replace.end)));
                if !text[..replace.start].contains('\n') {
                    items.push(Completion { label: "!DOCTYPE".to_owned(), insert: "!DOCTYPE html>".to_owned() });
                }
            }
            (replace, items)
        }
        Place::EndTag { replace } => {
            let items = match open.last() {
                Some(tag) => vec![end_tag(tag, replace.end)],
                None => HTML_TAGS.iter().map(|(name, _, _)| end_tag(name, replace.end)).collect(),
            };
            (replace, items)
        }
        Place::Attribute { tag, replace, others } => {
            if replace.start == offset && trigger == Trigger::Typing {
                return None;
            }
            let assigned = followed_by(text, replace.end, '=');
            let own: &[HtmlAttribute] =
                HTML_TAGS.iter().find(|(name, _, _)| name.eq_ignore_ascii_case(&tag)).map_or(&[], |(_, _, attributes)| attributes);
            let mut seen: Vec<&str> = others.iter().map(String::as_str).collect();
            let mut items = Vec::new();
            for (name, valueless) in own.iter().chain(HTML_GLOBAL_ATTRIBUTES) {
                if seen.contains(name) {
                    continue;
                }
                seen.push(name);
                let value = if assigned || *valueless { "" } else { "=\"\"" };
                items.push(Completion { label: (*name).to_owned(), insert: format!("{name}{value}") });
            }
            items.push(Completion { label: "data-".to_owned(), insert: "data-".to_owned() });
            (replace, items)
        }
    };
    let typed = &text[replace.start..offset];
    let mut ranked: Vec<(i32, Completion)> = items
        .into_iter()
        .filter_map(|item| {
            let score = if typed.is_empty() { 0 } else { words::fuzzy::score(typed, &item.label)?.score };
            Some((score, item))
        })
        .collect();
    ranked.sort_by(|(score_a, a), (score_b, b)| {
        score_b.cmp(score_a).then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
    });
    Some(Completions { replace, items: ranked.into_iter().map(|(_, item)| item).collect() })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replaced text, and the first `count` labels and inserts offered at `|` in `text`.
    fn offered(text: &str, trigger: Trigger, count: usize) -> Option<(String, Vec<(String, String)>)> {
        let offset = text.find('|').expect("a cursor");
        let text = text.replace('|', "");
        let found = completions(&text, offset, trigger)?;
        let items = found.items.into_iter().take(count).map(|item| (item.label, item.insert)).collect();
        Some((text[found.replace].to_owned(), items))
    }

    fn item(label: &str, insert: &str) -> (String, String) {
        (label.to_owned(), insert.to_owned())
    }

    #[test]
    fn after_a_bracket_the_elements_and_the_open_elements_end_tag_are_offered() {
        let (replace, items) = offered("<ul>\n  <|", Trigger::Typing, 200).expect("tags");
        assert_eq!(replace, "");
        assert_eq!(items.len(), HTML_TAGS.len() + 1);
        assert_eq!(items[0], item("/ul", "/ul>"));
        assert!(items.contains(&item("div", "div")));
        // The doctype on the first line.
        let (_, items) = offered("<|", Trigger::Typing, 2).expect("tags");
        assert_eq!(items[0], item("!DOCTYPE", "!DOCTYPE html>"));
    }

    #[test]
    fn a_typed_element_name_is_completed() {
        let (replace, items) = offered("<p><spa|n>", Trigger::Typing, 1).expect("tags");
        assert_eq!((replace.as_str(), &items[..]), ("span", &[item("span", "span")][..]));
    }

    #[test]
    fn after_an_end_tag_bracket_the_innermost_open_element_is_closed() {
        let text = "<div><p>text</p><img src=x><br/><span>a</span>\n<!-- </b> --></|";
        let (replace, items) = offered(text, Trigger::Typing, 5).expect("end tag");
        assert_eq!((replace.as_str(), &items[..]), ("/", &[item("/div", "/div>")][..]));
        // A `>` already there is kept.
        let (_, items) = offered("<section></|>", Trigger::Typing, 5).expect("end tag");
        assert_eq!(items, [item("/section", "/section")]);
        // Nothing open: every element.
        let (_, items) = offered("</|", Trigger::Typing, 500).expect("end tags");
        assert_eq!(items.len(), HTML_TAGS.len());
    }

    #[test]
    fn in_a_start_tag_the_attributes_it_does_not_have_are_offered() {
        let (replace, items) = offered("<a hr|", Trigger::Typing, 1).expect("attributes");
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"\"")][..]));
        // Valueless ones, and those followed by `=`, take no value.
        let (_, items) = offered("<div hidd|", Trigger::Typing, 1).expect("attributes");
        assert_eq!(items, [item("hidden", "hidden")]);
        let (_, items) = offered("<a hr|=\"x\">", Trigger::Typing, 1).expect("attributes");
        assert_eq!(items, [item("href", "href")]);
        // Asked for after a space: the element's own first, none it has already.
        let (replace, items) = offered("<a href=\"x\" | title=\"y\">", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href" || label == "title"));
        assert!(items.contains(&item("download", "download=\"\"")));
        assert!(items.contains(&item("data-", "data-")));
        assert_eq!(offered("<a href=\"x\" |", Trigger::Typing, 1), None);
    }

    /// An attribute name starts only after ASCII whitespace; Unicode whitespace (a no-break or
    /// ideographic space) is a character the scanner cannot read in a tag. As Monaco's HTML
    /// worker completes (checked against vscode-html-languageservice).
    #[test]
    fn unicode_whitespace_in_a_start_tag_is_read_as_the_scanner_reads_it() {
        // Before the cursor.
        let (replace, items) = offered("<div\u{3000}class=\"x\">\n<a\u{a0}href=y>\n<a hr|", Trigger::Typing, 1).expect("attributes");
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"\"")][..]));
        let (replace, items) = offered("<ul\u{a0}id=x>\n</|", Trigger::Typing, 5).expect("end tag");
        assert_eq!((replace.as_str(), &items[..]), ("/", &[item("/ul", "/ul>")][..]));
        let (_, items) = offered("<ul\u{3000}id=x>\n<|", Trigger::Typing, 1).expect("tags");
        assert_eq!(items, [item("/ul", "/ul>")]);
        // At it: no name follows the tag's without whitespace between.
        assert_eq!(offered("<a\u{a0}hr|", Trigger::Typing, 1), None);
        assert_eq!(offered("<a\u{3000}hr|", Trigger::Invoked, 1), None);
        assert_eq!(offered("<a\u{3000}|", Trigger::Invoked, 1), None);
        assert_eq!(offered("<div\u{3000}|class=\"x\">", Trigger::Invoked, 1), None);
        let (replace, items) = offered("<a \u{a0}hr|", Trigger::Typing, 1).expect("attributes");
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"\"")][..]));
        let (replace, _) = offered("<a hr| \u{a0}", Trigger::Typing, 1).expect("attributes");
        assert_eq!(replace, "hr");
        // A value cannot start with one: what follows is not an attribute.
        let (replace, items) = offered("<a href=\u{3000}x |", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href"));
        assert!(items.contains(&item("target", "target=\"\"")));
    }

    #[test]
    fn a_start_tag_is_read_as_the_scanner_reads_it() {
        // No attribute right after a value; a name after whitespace is not the one typed.
        let (replace, items) = offered("<a href=\"x\"title=\"y\" |", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(items.contains(&item("title", "title=\"\"")) && !items.iter().any(|(label, _)| label == "href"));
        let (replace, items) = offered("<a |href=\"x\">", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href"));
        // `=` without a name is a stray character; `/>` after an unquoted value closes the tag.
        let (_, items) = offered("<a =\"x\" |", Trigger::Invoked, 1).expect("attributes");
        assert_eq!(items, [item("accesskey", "accesskey=\"\"")]);
        let (_, items) = offered("<ul><a href=/>\n<|", Trigger::Typing, 1).expect("tags");
        assert_eq!(items, [item("/ul", "/ul>")]);
        // Script text whose lower case is longer.
        let script = format!("<script>{}</script><di|", "İ".repeat(12));
        let (replace, items) = offered(&script, Trigger::Typing, 1).expect("tags");
        assert_eq!((replace.as_str(), &items[..]), ("di", &[item("dialog", "dialog")][..]));
    }

    #[test]
    fn nothing_is_completed_in_text_comments_values_or_scripts() {
        assert_eq!(offered("<p>hel|lo</p>", Trigger::Invoked, 1), None);
        assert_eq!(offered("<!-- <di| -->", Trigger::Invoked, 1), None);
        assert_eq!(offered("<a href=\"ht|\">", Trigger::Invoked, 1), None);
        assert_eq!(offered("<script>if (a <b|) {}</script>", Trigger::Invoked, 1), None);
        // Past a script, tags again.
        assert!(offered("<script>a < b</script><|", Trigger::Typing, 1).is_some());
    }
}
