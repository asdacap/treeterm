//! Monaco's HTML worker (vscode-html-languageservice), ported over its scanner ([`scanner`]),
//! parser ([`document`]) and data ([`data`]):
//! - completions (`doComplete`): after `<` the elements, the end tag of the element open
//!   there and (on the first line) the doctype; after `</` that end tag (moved to its start
//!   tag's indentation on a line of its own); in a start tag the attributes it does not have
//!   yet (`data-` ones the document uses too), inserted with their quotes; at an attribute
//!   value the values it takes; after a start tag's `>` its end tag; and after `&` the
//!   character entities. They are filtered and ranked as Monaco's suggest widget does.
//! - hover documentation (`doHover`) on an element's start or end tag, an attribute, an
//!   attribute value and a character entity: the description and the MDN reference.
//! - the matching tag highlights (`findDocumentHighlights`): on an element's start or end tag
//!   name, both names.

mod data;
mod document;
mod scanner;

use std::ops::Range;

use self::document::{Document, NodeId};
use self::scanner::{Scanner, State, Token, is_js_space};
use super::{Completion, Hover, Insert, Then};
use crate::words::{self, Trigger};

/// The characters that ask the HTML worker for completions as they are typed.
pub(super) const TRIGGER_CHARACTERS: [char; 6] = ['.', ':', '<', '"', '=', '/'];

/// A completion before it is filtered: [`Completion`] and the text what was typed is matched
/// against (its `filterText`, else its label).
struct Proposal {
    completion: Completion,
    filter: String,
}

/// What `doComplete` collects.
struct Collector<'a> {
    text: &'a str,
    offset: usize,
    document: Document,
    /// The element before the cursor, which the scan starts at.
    node: NodeId,
    proposals: Vec<Proposal>,
}

impl<'a> Collector<'a> {
    /// `getReplaceRange`: from `start` (no later than the cursor) to `end`.
    fn range(&self, start: usize, end: usize) -> Range<usize> {
        start.min(self.offset)..end
    }

    fn add(&mut self, label: String, replace: Range<usize>, insert: Insert, filter: Option<String>, then: Then) {
        let filter = filter.unwrap_or_else(|| label.clone());
        self.proposals.push(Proposal { completion: Completion { label, replace, insert, then }, filter });
    }

    /// `collectOpenTagSuggestions`: the elements.
    fn open_tags(&mut self, after_open_bracket: usize, tag_name_end: usize) {
        let range = self.range(after_open_bracket, tag_name_end);
        for tag in data::tags() {
            self.add(tag.name.clone(), range.clone(), Insert::Text(tag.name.clone()), None, Then::Done);
        }
    }

    /// `getLineIndent`: the whitespace from the start of the line to `offset`, if there is only
    /// whitespace there.
    fn line_indent(&self, offset: usize) -> Option<&'a str> {
        let before = &self.text[..offset];
        let start = before.rfind(['\n', '\r']).map_or(0, |index| index + 1);
        let indent = &before[start..];
        indent.chars().all(is_js_space).then_some(indent)
    }

    /// `collectCloseTagSuggestions`: the end tag of the innermost element still open (not the
    /// one being started, `in_open_tag`), else every element's.
    fn close_tags(&mut self, after_open_bracket: usize, in_open_tag: bool, tag_name_end: usize) {
        let range = self.range(after_open_bracket, tag_name_end);
        let close = if followed_by(self.text, tag_name_end, State::WithinEndTag, Token::EndTagClose) { "" } else { ">" };
        let mut current = Some(self.node);
        if in_open_tag {
            current = current.and_then(|node| self.document.nodes[node].parent);
        }
        while let Some(id) = current {
            let node = &self.document.nodes[id];
            let open = !node.closed || node.end_tag_start.is_some_and(|end_tag| end_tag > self.offset);
            if let (Some(tag), true) = (&node.tag, open) {
                let label = format!("/{tag}");
                let start_indent = self.line_indent(node.start);
                let end_indent = self.line_indent(after_open_bracket - 1);
                match (start_indent, end_indent) {
                    (Some(start_indent), Some(end_indent)) if start_indent != end_indent => {
                        let range = self.range(after_open_bracket - 1 - end_indent.len(), self.offset);
                        let insert = format!("{start_indent}</{tag}{close}");
                        let filter = format!("{end_indent}</{tag}");
                        self.add(label, range, Insert::Text(insert), Some(filter), Then::Done);
                    }
                    _ => {
                        let insert = format!("/{tag}{close}");
                        self.add(label, range, Insert::Text(insert), None, Then::Done);
                    }
                }
                return;
            }
            current = node.parent;
        }
        if in_open_tag {
            return;
        }
        for tag in data::tags() {
            let label = format!("/{}", tag.name);
            let insert = format!("/{}{close}", tag.name);
            self.add(label, range.clone(), Insert::Text(insert.clone()), Some(insert), Then::Done);
        }
    }

    /// `collectAutoCloseTagSuggestion`: after the `>` of a start tag of `tag`, its end tag.
    fn auto_close_tag(&mut self, tag_close_end: usize, tag: &str) {
        if data::is_void(tag) {
            return;
        }
        let label = format!("</{tag}>");
        let insert = Insert::Snippet(format!("$0</{tag}>"));
        self.add(label.clone(), tag_close_end..tag_close_end, insert, Some(label), Then::Done);
    }

    /// `collectAttributeNameSuggestions`: the attributes of `tag` not written yet.
    fn attribute_names(&mut self, tag: &str, name_start: usize, name_end: usize) {
        let mut replace_end = self.offset;
        while replace_end < name_end && self.text.as_bytes()[replace_end] != b'<' {
            replace_end += 1;
        }
        // From whitespace past the cursor (`name_start` after `name_end`), the character between
        // them, as JavaScript's `substring` takes it.
        let current = self.text[name_start.min(name_end)..name_start.max(name_end)].to_owned();
        let range = self.range(name_start, replace_end);
        let value = if followed_by(self.text, name_end, State::AfterAttributeName, Token::DelimiterAssign) {
            ""
        } else {
            "=\"$1\""
        };
        // Those written already, but the one being typed.
        let mut seen: Vec<String> = self.document.nodes[self.node]
            .attributes
            .iter()
            .map(|(name, _)| name.clone())
            .filter(|name| *name != current)
            .collect();
        for attribute in data::attributes(tag) {
            if seen.contains(&attribute.name) {
                continue;
            }
            seen.push(attribute.name.clone());
            let (insert, then) = if !attribute.is_valueless() && !value.is_empty() {
                let has_values = attribute.value_set.is_some() || attribute.name == "style";
                let then = if has_values { Then::Suggest } else { Then::Done };
                (format!("{}{value}", attribute.name), then)
            } else {
                (attribute.name.clone(), Then::Done)
            };
            self.add(attribute.name.clone(), range.clone(), Insert::Snippet(insert), None, then);
        }
        // `collectDataAttributesSuggestions`: `data-`, and the document's `data-` attributes.
        let mut data_attributes = vec![("data-".to_owned(), "data-$1=\"$2\"".to_owned())];
        for node in &self.document.nodes {
            for (name, _) in &node.attributes {
                let known = data_attributes.iter().any(|(seen_name, _)| seen_name == name);
                if name.starts_with("data-") && !known && !seen.contains(name) {
                    data_attributes.push((name.clone(), format!("{name}=\"$1\"")));
                }
            }
        }
        for (name, snippet) in data_attributes {
            self.add(name, range.clone(), Insert::Snippet(snippet), None, Then::Done);
        }
    }

    /// `collectAttributeValueSuggestions`: the values `attribute` of `tag` takes, from
    /// `value_start` (the value token, or the cursor right after `=`) to `value_end`; and the
    /// character entities.
    fn attribute_values(&mut self, tag: &str, attribute: &str, value_start: usize, value_end: usize) {
        let text = self.text;
        let quoted = text[value_start..].starts_with(['"', '\'']);
        let (range, add_quotes) = if self.offset > value_start && self.offset <= value_end && quoted {
            // Inside the quotes: the word at the cursor.
            let content_start = value_start + 1;
            let mut content_end = value_end;
            if value_end > value_start && text[..value_end].ends_with(&text[value_start..content_start]) {
                content_end -= 1;
            }
            let word_start = text[content_start..self.offset]
                .char_indices()
                .rev()
                .take_while(|(_, c)| !is_js_space(*c))
                .last()
                .map_or(self.offset, |(at, _)| content_start + at);
            let word_end = text[self.offset..content_end.max(self.offset)]
                .find(is_js_space)
                .map_or(content_end.max(self.offset), |at| self.offset + at);
            (self.range(word_start, word_end), false)
        } else {
            (self.range(value_start, value_end), true)
        };
        for value in data::values(tag, attribute) {
            let insert = if add_quotes { format!("\"{}\"", value.name) } else { value.name.clone() };
            self.add(value.name.clone(), range.clone(), Insert::Text(insert.clone()), Some(insert), Then::Done);
        }
        self.entities();
    }

    /// `collectCharacterEntityProposals`: after `&` and letters or digits, the entities.
    fn entities(&mut self) {
        let before = &self.text.as_bytes()[..self.offset];
        let letters = before.iter().rev().take_while(|byte| byte.is_ascii_alphanumeric()).count();
        let Some(ampersand) = (self.offset - letters).checked_sub(1) else {
            return;
        };
        if before[ampersand] != b'&' {
            return;
        }
        for (name, _) in data::entities().iter().filter(|(name, _)| name.ends_with(';')) {
            let label = format!("&{name}");
            self.add(label.clone(), ampersand..self.offset, Insert::Text(label), None, Then::Done);
        }
    }

    /// `suggestDoctype`.
    fn doctype(&mut self, start: usize, end: usize) {
        let range = self.range(start, end);
        self.add("!DOCTYPE".to_owned(), range, Insert::Text("!DOCTYPE html>".to_owned()), None, Then::Done);
    }
}

/// `isFollowedBy`: whether, scanned from `offset` in `state`, the first token past whitespace
/// is `expected`.
fn followed_by(text: &str, offset: usize, state: State, expected: Token) -> bool {
    let mut scanner = Scanner::new(text, offset, state, false);
    let mut token = scanner.scan();
    while token == Token::Whitespace {
        token = scanner.scan();
    }
    token == expected
}

/// `scanNextForEndPos`: where the token after the one just scanned ends, if it is `next` and
/// starts at the cursor; the cursor otherwise.
fn next_end(scanner: &mut Scanner, offset: usize, next: Token) -> usize {
    if offset == scanner.token_end() && scanner.scan() == next && scanner.token_offset() == offset {
        return scanner.token_end();
    }
    offset
}

/// `_doComplete`: what the HTML worker proposes at byte `offset` of `text`.
fn collect(text: &str, offset: usize) -> Vec<Proposal> {
    let document = Document::parse(text);
    let node = document.node_before(offset);
    let mut scanner = Scanner::new(text, document.nodes[node].start, State::WithinContent, false);
    let mut collector = Collector { text, offset, document, node, proposals: Vec::new() };
    let mut current_tag = String::new();
    let mut current_attribute = String::new();
    let mut token = scanner.scan();
    while token != Token::Eos && scanner.token_offset() <= offset {
        let (start, end) = (scanner.token_offset(), scanner.token_end());
        match token {
            Token::StartTagOpen if end == offset => {
                let end_pos = next_end(&mut scanner, offset, Token::StartTag);
                if !text[..offset].contains(['\n', '\r']) {
                    collector.doctype(offset, end_pos);
                }
                collector.open_tags(offset, end_pos);
                collector.close_tags(offset, true, end_pos);
                break;
            }
            Token::StartTag => {
                if start <= offset && offset <= end {
                    collector.open_tags(start, end);
                    break;
                }
                current_tag = scanner.token_text().to_owned();
            }
            Token::AttributeName => {
                if start <= offset && offset <= end {
                    collector.attribute_names(&current_tag, start, end);
                    break;
                }
                current_attribute = scanner.token_text().to_owned();
            }
            Token::DelimiterAssign if end == offset => {
                let end_pos = next_end(&mut scanner, offset, Token::AttributeValue);
                collector.attribute_values(&current_tag, &current_attribute, offset, end_pos);
                break;
            }
            Token::AttributeValue if start <= offset && offset <= end => {
                collector.attribute_values(&current_tag, &current_attribute, start, end);
                break;
            }
            Token::Whitespace if offset <= end => match scanner.state() {
                State::AfterOpeningStartTag => {
                    let end_pos = next_end(&mut scanner, offset, Token::StartTag);
                    collector.open_tags(start, end_pos);
                    collector.close_tags(start, true, end_pos);
                    break;
                }
                State::WithinTag | State::AfterAttributeName => {
                    collector.attribute_names(&current_tag, end, offset);
                    break;
                }
                State::BeforeAttributeValue => {
                    collector.attribute_values(&current_tag, &current_attribute, end, offset);
                    break;
                }
                State::AfterOpeningEndTag => {
                    collector.close_tags(start - 1, false, offset);
                    break;
                }
                State::WithinContent => {
                    collector.entities();
                    break;
                }
                // Whitespace in an end tag or a doctype: scanned past.
                _ => {}
            },
            Token::EndTagOpen if offset <= end => {
                let end_pos = next_end(&mut scanner, offset, Token::EndTag);
                collector.close_tags(start + 1, false, end_pos);
                break;
            }
            Token::EndTag if offset <= end => {
                // `</ name`: the slash before the spaces.
                let before = text[..start].trim_end_matches(is_js_space);
                if before.ends_with('/') {
                    collector.close_tags(before.len() - 1, false, end);
                    break;
                }
            }
            Token::StartTagClose if offset <= end && !current_tag.is_empty() => {
                collector.auto_close_tag(end, &current_tag);
                break;
            }
            Token::Content if offset <= end => {
                collector.entities();
                break;
            }
            Token::StartTagOpen
            | Token::DelimiterAssign
            | Token::AttributeValue
            | Token::Whitespace
            | Token::EndTagOpen
            | Token::EndTag
            | Token::StartTagClose
            | Token::Content => {}
            _ if offset <= end => break,
            _ => {}
        }
        token = scanner.scan();
    }
    collector.proposals
}

/// Whether typing the character before byte `offset` of `text` asks the HTML worker, or leaves
/// something to show of what it proposed: one of its trigger characters does, and so does a
/// word being typed (Monaco's quick suggestions, and its filtering of the suggestions
/// showing). Another character (a space, `>`, `&`) shows nothing.
fn asks(text: &str, offset: usize) -> bool {
    let typed = text.get(..offset).and_then(|before| before.chars().next_back());
    typed.is_some_and(|typed| TRIGGER_CHARACTERS.contains(&typed)) || words::typing_word(text, offset, "html")
}

/// The completions Monaco's HTML worker offers at byte `offset` of `text`, an `html` file,
/// filtered by what was typed of each (from its replaced range to the cursor, past leading
/// spaces) and ranked as the suggest widget does: best match, then by label. `None` where it
/// proposes nothing, and Monaco's word suggestions take over; an empty list where what was
/// typed matches none of its proposals.
pub fn completions(text: &str, offset: usize, trigger: Trigger) -> Option<Vec<Completion>> {
    if trigger == Trigger::Typing && !asks(text, offset) {
        return None;
    }
    let proposals = collect(text, offset);
    if proposals.is_empty() {
        return None;
    }
    let mut ranked: Vec<(i32, Completion)> = proposals
        .into_iter()
        .filter_map(|Proposal { completion, filter }| {
            let typed = &text[completion.replace.start.min(offset)..offset];
            let typed = typed.trim_start_matches([' ', '\t']);
            let score = if typed.is_empty() { 0 } else { words::fuzzy::score(typed, &filter)?.score };
            Some((score, completion))
        })
        .collect();
    ranked.sort_by(|(score_a, a), (score_b, b)| {
        score_b
            .cmp(score_a)
            .then_with(|| a.label.to_lowercase().cmp(&b.label.to_lowercase()))
            .then_with(|| a.label.cmp(&b.label))
    });
    Some(ranked.into_iter().map(|(_, completion)| completion).collect())
}

/// `getTagNameRange` of `doHover`: the bytes of the token `kind` that holds `offset`, the
/// first one scanned from `start` that ends there or later.
fn hovered_token(text: &str, start: usize, offset: usize, kind: Token) -> Option<Range<usize>> {
    let mut scanner = Scanner::new(text, start, State::WithinContent, false);
    let mut token = scanner.scan();
    while token != Token::Eos && (scanner.token_end() < offset || scanner.token_end() == offset && token != kind) {
        token = scanner.scan();
    }
    (token == kind && offset <= scanner.token_end()).then(|| scanner.token_offset()..scanner.token_end())
}

/// The hover of an entry with documentation; none for one without.
fn documented(contents: Option<String>, range: Range<usize>) -> Option<Hover> {
    contents.map(|contents| Hover { contents, range })
}

/// The hover over an element named `tag`.
fn tag_hover(tag: &str, range: Range<usize>) -> Option<Hover> {
    let entry = data::tag(tag)?;
    documented(data::documentation(entry.description.as_deref(), &entry.references), range)
}

/// The UTF-16 code unit JavaScript's `charCodeAt(0)` reads of `text`, as `U+` and four or
/// more hex digits.
fn first_code_unit(text: &str) -> String {
    let unit = text.encode_utf16().next().unwrap_or(0);
    format!("U+{unit:04X}")
}

/// What Monaco's HTML worker shows on hover at byte `offset` of `text`, an `html` file
/// (`doHover`): on an element's start or end tag name, the element's description and MDN
/// reference; on an attribute name, the attribute's; on an attribute value, the value's; on a
/// character entity, the character it stands for. `None` elsewhere, and for an entry without
/// documentation.
pub fn hover(text: &str, offset: usize) -> Option<Hover> {
    let document = Document::parse(text);
    let node = &document.nodes[document.node_at(offset)];
    let tag = node.tag.as_deref()?;
    if let Some(end_tag_start) = node.end_tag_start.filter(|&end_tag_start| offset >= end_tag_start) {
        return tag_hover(tag, hovered_token(text, end_tag_start, offset, Token::EndTag)?);
    }
    if let Some(range) = hovered_token(text, node.start, offset, Token::StartTag) {
        return tag_hover(tag, range);
    }
    if let Some(range) = hovered_token(text, node.start, offset, Token::AttributeName) {
        let name = &text[range.clone()];
        let entry = data::attributes(tag).filter(|entry| entry.name == name && entry.description.is_some()).last()?;
        return documented(data::documentation(entry.description.as_deref(), &entry.references), range);
    }
    let bytes = text.as_bytes();
    let start = offset - bytes[..offset].iter().rev().take_while(|byte| byte.is_ascii_alphanumeric()).count();
    if start > 0 && bytes[start - 1] == b'&' {
        let end = start + bytes[start..].iter().take_while(|byte| byte.is_ascii_alphanumeric()).count();
        let range = if bytes.get(end) == Some(&b';') { start..end + 1 } else { start..end };
        let name = format!("{};", &text[start..end]);
        let (_, character) = data::entities().iter().find(|(entity, _)| *entity == name)?;
        let unit = first_code_unit(character);
        let contents = format!("Character entity representing '{character}', unicode equivalent '{unit}'");
        return Some(Hover { contents, range });
    }
    let range = hovered_token(text, node.start, offset, Token::AttributeValue)?;
    // The attribute the value is of: the last name scanned before it.
    let mut scanner = Scanner::new(text, node.start, State::WithinContent, false);
    let mut attribute = None;
    while scanner.scan() != Token::Eos && scanner.token_end() <= range.start {
        if scanner.token() == Token::AttributeName {
            attribute = Some(scanner.token_text());
        }
    }
    let value = trim_quotes(&text[range.clone()]);
    // The last one of that name with a description.
    let entry = data::values(tag, attribute?)
        .into_iter()
        .rev()
        .find(|entry| entry.name == value && entry.description.is_some())?;
    documented(data::documentation(entry.description.as_deref(), &entry.references), range)
}

/// `trimQuotes`: a value without its opening and closing quotes.
fn trim_quotes(value: &str) -> &str {
    let value = value.strip_prefix(['"', '\'']).unwrap_or(value);
    value.strip_suffix(['"', '\'']).unwrap_or(value)
}

/// The bytes of the first token `kind` scanned from `start` (`getTagNameRange` of
/// `findDocumentHighlights`).
fn first_token(text: &str, start: usize, kind: Token) -> Option<Range<usize>> {
    let mut scanner = Scanner::new(text, start, State::WithinContent, false);
    loop {
        match scanner.scan() {
            Token::Eos => return None,
            token if token == kind => return Some(scanner.token_offset()..scanner.token_end()),
            _ => {}
        }
    }
}

/// The matching tag highlights at byte `offset` of `text`, an `html` file
/// (`findDocumentHighlights`): with the cursor on an element's start or end tag name (its ends
/// included), both names; none elsewhere.
pub fn matching_tags(text: &str, offset: usize) -> Vec<Range<usize>> {
    let document = Document::parse(text);
    let node = &document.nodes[document.node_at(offset)];
    if node.tag.is_none() {
        return Vec::new();
    }
    let start_tag = first_token(text, node.start, Token::StartTag);
    let end_tag = node.end_tag_start.and_then(|end_tag_start| first_token(text, end_tag_start, Token::EndTag));
    let covers =
        |range: &Option<Range<usize>>| range.as_ref().is_some_and(|range| range.start <= offset && offset <= range.end);
    if !covers(&start_tag) && !covers(&end_tag) {
        return Vec::new();
    }
    start_tag.into_iter().chain(end_tag).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The replaced text, and the first `count` labels and inserts offered at `|` in `text`.
    fn offered(text: &str, trigger: Trigger, count: usize) -> Option<(String, Vec<(String, String)>)> {
        let offset = text.find('|').expect("a cursor");
        let text = text.replace('|', "");
        let found = completions(&text, offset, trigger)?;
        let replace = found.first().map_or(String::new(), |item| text[item.replace.clone()].to_owned());
        let items = found.into_iter().take(count).map(|item| (item.label, item.insert.source().to_owned())).collect();
        Some((replace, items))
    }

    fn item(label: &str, insert: &str) -> (String, String) {
        (label.to_owned(), insert.to_owned())
    }

    #[test]
    fn after_a_bracket_the_elements_and_the_open_elements_end_tag_are_offered() {
        let (replace, items) = offered("<ul>\n<|", Trigger::Typing, 200).expect("tags");
        assert_eq!(replace, "");
        assert_eq!(items.len(), data::tags().len() + 1);
        assert_eq!(items[0], item("/ul", "/ul>"));
        assert!(items.contains(&item("div", "div")));
        // The doctype on the first line.
        let (_, items) = offered("<|", Trigger::Typing, 2).expect("tags");
        assert_eq!(items[0], item("!DOCTYPE", "!DOCTYPE html>"));
    }

    #[test]
    fn an_end_tag_on_a_line_of_its_own_moves_to_its_start_tags_indentation() {
        let text = "  <ul>\n    <|";
        let offset = text.find('|').unwrap();
        let found = completions(&text.replace('|', ""), offset, Trigger::Typing).expect("tags");
        // Replacing the line's indentation and the bracket; matched by the bracket alone, it
        // ranks after the elements, which match the nothing typed after it.
        let end_tag = found.last().expect("the end tag");
        assert_eq!((end_tag.label.as_str(), end_tag.replace.clone()), ("/ul", 7..offset));
        assert_eq!(end_tag.insert, Insert::Text("  </ul>".to_owned()));
        assert_eq!(found[0].label, "a");
        let (replace, items) = offered("<div>\n  </|", Trigger::Typing, 5).expect("end tag");
        assert_eq!((replace.as_str(), &items[..]), ("  </", &[item("/div", "</div>")][..]));
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
        assert_eq!(items.len(), data::tags().len());
    }

    #[test]
    fn in_a_start_tag_the_attributes_it_does_not_have_are_offered() {
        let (replace, items) = offered("<a hr|", Trigger::Typing, 1).expect("attributes");
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"$1\"")][..]));
        // Valueless ones, and those followed by `=`, take no value.
        let (_, items) = offered("<div hidd|", Trigger::Typing, 1).expect("attributes");
        assert_eq!(items, [item("hidden", "hidden")]);
        let (_, items) = offered("<a hr|=\"x\">", Trigger::Typing, 1).expect("attributes");
        assert_eq!(items, [item("href", "href")]);
        // Asked for after a space: the element's own first, none it has already.
        let (replace, items) = offered("<a href=\"x\" | title=\"y\">", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href" || label == "title"));
        assert!(items.contains(&item("download", "download=\"$1\"")));
        assert!(items.contains(&item("data-", "data-$1=\"$2\"")));
        assert_eq!(offered("<a href=\"x\" |", Trigger::Typing, 1), None);
    }

    #[test]
    fn an_attribute_with_values_asks_for_them_once_picked() {
        let found = completions("<input typ", 10, Trigger::Typing).expect("attributes");
        assert_eq!((found[0].label.as_str(), found[0].then), ("type", Then::Suggest));
        let found = completions("<a hre", 6, Trigger::Typing).expect("attributes");
        assert_eq!((found[0].label.as_str(), found[0].then), ("href", Then::Done));
        let found = completions("<a styl", 7, Trigger::Typing).expect("attributes");
        assert_eq!((found[0].label.as_str(), found[0].then), ("style", Then::Suggest));
    }

    #[test]
    fn the_documents_data_attributes_are_offered() {
        let (_, items) = offered("<b data-x=1 data-y></b><i |", Trigger::Invoked, 500).expect("attributes");
        let data: Vec<&(String, String)> = items.iter().filter(|(label, _)| label.starts_with("data-")).collect();
        assert_eq!(data, [&item("data-", "data-$1=\"$2\""), &item("data-x", "data-x=\"$1\""), &item("data-y", "data-y=\"$1\"")]);
    }

    #[test]
    fn an_attributes_values_are_offered_quoted_unless_inside_quotes() {
        let (replace, items) = offered("<input type=|", Trigger::Typing, 2).expect("values");
        assert_eq!((replace.as_str(), &items[..]), ("", &[item("button", "\"button\""), item("checkbox", "\"checkbox\"")][..]));
        let (replace, items) = offered("<input type=\"te|\">", Trigger::Typing, 2).expect("values");
        assert_eq!((replace.as_str(), &items[..]), ("te", &[item("tel", "tel"), item("text", "text")][..]));
        // Another word in the quotes.
        let (replace, items) = offered("<input type=\"x te|\">", Trigger::Typing, 1).expect("values");
        assert_eq!((replace.as_str(), &items[..]), ("te", &[item("tel", "tel")][..]));
        // No values: what was typed matches nothing, and no words are offered either.
        assert_eq!(offered("<a href=\"ab|\">", Trigger::Typing, 1), None);
    }

    #[test]
    fn after_an_ampersand_the_character_entities_are_offered() {
        let (replace, items) = offered("<p>a &am|", Trigger::Typing, 3).expect("entities");
        let entities = [item("&amacr;", "&amacr;"), item("&amalg;", "&amalg;"), item("&amp;", "&amp;")];
        assert_eq!((replace.as_str(), &items[..]), ("&am", &entities[..]));
        let (replace, items) = offered("<p title=\"&lt|\">", Trigger::Typing, 1).expect("entities");
        assert_eq!((replace.as_str(), &items[..]), ("&lt", &[item("&lt;", "&lt;")][..]));
        assert_eq!(offered("<p>a b|", Trigger::Typing, 1), None);
    }

    #[test]
    fn after_a_start_tag_its_end_tag_is_offered() {
        let (replace, items) = offered("<div>|", Trigger::Invoked, 5).expect("end tag");
        assert_eq!((replace.as_str(), &items[..]), ("", &[item("</div>", "$0</div>")][..]));
        assert_eq!(offered("<br>|", Trigger::Invoked, 5), None);
    }

    /// An attribute name starts only after ASCII whitespace; Unicode whitespace (a no-break or
    /// ideographic space) is a character the scanner cannot read in a tag. As Monaco's HTML
    /// worker completes (checked against vscode-html-languageservice).
    #[test]
    fn unicode_whitespace_in_a_start_tag_is_read_as_the_scanner_reads_it() {
        // Before the cursor.
        let (replace, items) = offered("<div\u{3000}class=\"x\">\n<a\u{a0}href=y>\n<a hr|", Trigger::Typing, 1).expect("attributes");
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"$1\"")][..]));
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
        assert_eq!((replace.as_str(), &items[..]), ("hr", &[item("href", "href=\"$1\"")][..]));
        let (replace, _) = offered("<a hr| \u{a0}", Trigger::Typing, 1).expect("attributes");
        assert_eq!(replace, "hr");
        // A value cannot start with one: what follows is not an attribute.
        let (replace, items) = offered("<a href=\u{3000}x |", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href"));
        assert!(items.contains(&item("target", "target=\"$1\"")));
    }

    #[test]
    fn a_start_tag_is_read_as_the_scanner_reads_it() {
        // No attribute right after a value; a name after whitespace is not the one typed.
        let (replace, items) = offered("<a href=\"x\"title=\"y\" |", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(items.contains(&item("title", "title=\"$1\"")) && !items.iter().any(|(label, _)| label == "href"));
        let (replace, items) = offered("<a |href=\"x\">", Trigger::Invoked, 500).expect("attributes");
        assert_eq!(replace, "");
        assert!(!items.iter().any(|(label, _)| label == "href"));
        // `=` without a name is a stray character; `/>` after an unquoted value closes the tag.
        let (_, items) = offered("<a =\"x\" |", Trigger::Invoked, 1).expect("attributes");
        assert_eq!(items, [item("accesskey", "accesskey=\"$1\"")]);
        let (_, items) = offered("<ul><a href=/>\n<|", Trigger::Typing, 1).expect("tags");
        assert_eq!(items, [item("/ul", "/ul>")]);
        // Script text whose lower case is longer.
        let script = format!("<script>{}</script><di|", "İ".repeat(12));
        let (replace, items) = offered(&script, Trigger::Typing, 1).expect("tags");
        assert_eq!((replace.as_str(), &items[..]), ("di", &[item("dialog", "dialog")][..]));
    }

    #[test]
    fn nothing_is_completed_in_text_comments_values_without_values_or_scripts() {
        assert_eq!(offered("<p>hel|lo</p>", Trigger::Invoked, 1), None);
        assert_eq!(offered("<!-- <di| -->", Trigger::Invoked, 1), None);
        assert_eq!(offered("<a href=\"ht|\">", Trigger::Invoked, 1), None);
        assert_eq!(offered("<script>if (a <b|) {}</script>", Trigger::Invoked, 1), None);
        // Past a script, tags again.
        assert!(offered("<script>a < b</script><|", Trigger::Typing, 1).is_some());
    }

    #[test]
    fn typing_asks_the_worker_with_its_trigger_characters_and_at_the_end_of_a_word() {
        assert!(completions("<input type=", 12, Trigger::Typing).is_some());
        assert_eq!(completions("<a ", 3, Trigger::Typing), None);
        assert!(completions("<a ", 3, Trigger::Invoked).is_some());
    }

    /// The hover at `|` in `text`: its markdown and the text it is over.
    fn hovered(text: &str) -> Option<(String, String)> {
        let offset = text.find('|').expect("a cursor");
        let text = text.replace('|', "");
        hover(&text, offset).map(|hover| (hover.contents, text[hover.range].to_owned()))
    }

    /// As Monaco's HTML worker shows them (checked against vscode-html-languageservice).
    #[test]
    fn tags_attributes_values_and_entities_are_documented_on_hover() {
        let (contents, over) = hovered("<di|v></div>").expect("tag");
        assert_eq!(over, "div");
        assert!(contents.starts_with("The div element has no special meaning at all."), "{contents}");
        assert!(contents.ends_with("[MDN Reference](https://developer.mozilla.org/docs/Web/HTML/Element/div)"), "{contents}");
        assert_eq!(hovered("<div></d|iv>").map(|(_, over)| over).as_deref(), Some("div"));
        let (contents, over) = hovered("<a hr|ef=\"x\">").expect("attribute");
        assert_eq!(over, "href");
        assert!(contents.starts_with("Contains a URL or a URL fragment"), "{contents}");
        let (contents, over) = hovered("<form method=\"po|st\">").expect("value");
        assert_eq!(over, "\"post\"");
        assert!(contents.starts_with("Corresponds to the HTTP [POST method]"), "{contents}");
        let (contents, over) = hovered("<p>&am|p; x</p>").expect("entity");
        assert_eq!((contents.as_str(), over.as_str()), ("Character entity representing '&', unicode equivalent 'U+0026'", "amp;"));
        // Nothing on text, an unknown element or a value without documentation.
        assert_eq!(hovered("<p>te|xt</p>"), None);
        assert_eq!(hovered("<fo|o></foo>"), None);
        assert_eq!(hovered("<a href=\"x|y\">"), None);
        assert_eq!(hovered("te|xt"), None);
    }

    #[test]
    fn on_a_tag_name_both_of_its_names_are_highlighted() {
        let text = "<div><p>a</p></div>";
        assert_eq!(matching_tags(text, 2), [1..4, 15..18]);
        assert_eq!(matching_tags(text, 18), [1..4, 15..18]);
        assert_eq!(matching_tags(text, 6), [6..7, 11..12]);
        // Not on a name; an element without an end tag.
        assert_eq!(matching_tags(text, 8), Vec::<Range<usize>>::new());
        assert_eq!(matching_tags("<br>", 2), std::iter::once(1..3).collect::<Vec<_>>());
        assert_eq!(matching_tags("text", 2), Vec::<Range<usize>>::new());
    }
}
