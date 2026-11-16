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

/// `nextAttributeName`: `[^\s"'`=<>\/]+`.
fn attribute_name_len(text: &str, at: usize) -> usize {
    text[at..].find(|c: char| c.is_whitespace() || "\"'`=<>/".contains(c)).unwrap_or(text.len() - at)
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

/// What a start tag holds, read from its name to its end.
struct StartTag {
    attributes: Vec<(Range<usize>, String)>,
    /// Just past its `>` (or where it stops without one).
    end: usize,
    self_closing: bool,
    /// An attribute name at the cursor: being typed, or none yet after a space.
    attribute_at: Option<Range<usize>>,
    /// The cursor is in an attribute value.
    in_value: bool,
}

fn read_start_tag(text: &str, mut pos: usize, offset: usize) -> StartTag {
    let bytes = text.as_bytes();
    let mut tag = StartTag { attributes: Vec::new(), end: text.len(), self_closing: false, attribute_at: None, in_value: false };
    loop {
        let space = pos;
        while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
            pos += 1;
        }
        if space < offset && offset <= pos && space < pos {
            tag.attribute_at = Some(offset..offset);
        }
        match bytes.get(pos) {
            None => return tag,
            Some(b'>') => {
                tag.end = pos + 1;
                return tag;
            }
            Some(b'/') if bytes.get(pos + 1) == Some(&b'>') => {
                tag.end = pos + 2;
                tag.self_closing = true;
                return tag;
            }
            // A new tag: this one was never closed.
            Some(b'<') => {
                tag.end = pos;
                return tag;
            }
            Some(b'=') => {
                let value_start = pos;
                pos += 1;
                while pos < bytes.len() && bytes[pos].is_ascii_whitespace() {
                    pos += 1;
                }
                let value_end = match bytes.get(pos) {
                    Some(&quote @ (b'"' | b'\'')) => text[pos + 1..].find(quote as char).map_or(text.len(), |end| pos + 1 + end + 1),
                    Some(_) | None => {
                        text[pos..].find(|c: char| c.is_whitespace() || c == '>').map_or(text.len(), |end| pos + end)
                    }
                };
                if value_start < offset && offset <= value_end {
                    tag.in_value = true;
                }
                pos = value_end.max(pos + 1).min(text.len());
            }
            Some(_) => {
                let length = attribute_name_len(text, pos);
                if length == 0 {
                    pos += 1;
                    continue;
                }
                let name = pos..pos + length;
                if name.start <= offset && offset <= name.end {
                    tag.attribute_at = Some(name.clone());
                }
                tag.attributes.push((name.clone(), text[name.clone()].to_owned()));
                pos = name.end;
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
            // Script and style hold text up to their end tag.
            if name == "script" || name == "style" {
                let end_tag = format!("</{name}");
                let content_end = text[pos..].to_lowercase().find(&end_tag).map_or(text.len(), |end| pos + end);
                if content_end >= offset {
                    return None;
                }
                pos = content_end;
            }
            continue;
        }
        if tag.in_value {
            return None;
        }
        let replace = tag.attribute_at?;
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
