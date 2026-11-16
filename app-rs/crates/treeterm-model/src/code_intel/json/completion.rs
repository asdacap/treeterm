//! What Monaco's JSON worker completes without a schema: a port of vscode-json-languageservice's
//! `doComplete` (`services/jsonCompletion.ts`) with no schema associated, as Monaco 0.55.1
//! sets the worker up (`getSchemaLessPropertyCompletions`, `getSchemaLessValueCompletions`),
//! then filtered and ranked by what was typed, as Monaco's suggest widget does.
//!
//! In an object it proposes the keys that objects at the same place use (under a property of
//! the same name, or in the same array), less those the object has, and `$schema` in the
//! root object; at a value, the values the same key has elsewhere (other items of the array
//! at the same key, in an array), and the other boolean; in an empty file, an empty object
//! and an empty array. A document whose root object names a `$schema` is read with that
//! schema: the worker cannot load one, and the drafts it has built in are not ported.

use std::collections::HashSet;
use std::ops::Range;

use super::{Node, NodeKind, PropertyNode, Scanner, Token, parse, schema_of};
use crate::code_intel::{Completion, Insert, Then};
use crate::words::{self, Trigger, fuzzy};

/// The characters that ask the JSON worker for its completions as they are typed.
pub(in crate::code_intel) const TRIGGER_CHARACTERS: [char; 3] = [' ', ':', '"'];

/// A node of the document, as jsonParser's AST has them.
#[derive(Debug, Clone, Copy)]
enum At<'a> {
    Value(&'a Node),
    Property(&'a PropertyNode),
    /// A property's key, a string node of its own.
    Key(&'a PropertyNode),
}

impl At<'_> {
    fn range(self) -> Range<usize> {
        match self {
            At::Value(node) => node.range.clone(),
            At::Property(property) => property.range.clone(),
            At::Key(property) => property.key.range.clone(),
        }
    }
}

/// `contains(node, offset, true)`: whether `range`, its end included, holds `offset`.
fn touches(range: &Range<usize>, offset: usize) -> bool {
    range.start <= offset && offset <= range.end
}

/// The nodes from `root` down to the innermost one at `offset`, its end included
/// (`findNodeAtOffset(root, offset, true)`): the first child that holds it, at each level.
fn path_at(root: &Node, offset: usize) -> Vec<At<'_>> {
    let mut path = Vec::new();
    if !touches(&root.range, offset) {
        return path;
    }
    let mut at = At::Value(root);
    loop {
        path.push(at);
        let children: Vec<At<'_>> = match at {
            At::Value(Node { kind: NodeKind::Object(properties), .. }) => properties.iter().map(At::Property).collect(),
            At::Value(Node { kind: NodeKind::Array(items), .. }) => items.iter().map(At::Value).collect(),
            At::Property(property) => std::iter::once(At::Key(property)).chain(property.value.as_ref().map(At::Value)).collect(),
            At::Value(_) | At::Key(_) => Vec::new(),
        };
        let inner = children.into_iter().take_while(|child| child.range().start <= offset).find(|child| touches(&child.range(), offset));
        match inner {
            Some(inner) => at = inner,
            None => return path,
        }
    }
}

/// Every property of the document, in the order `doc.visit` meets them.
fn properties(root: Option<&Node>) -> Vec<&PropertyNode> {
    let mut found = Vec::new();
    let mut stack: Vec<At<'_>> = root.map(At::Value).into_iter().collect();
    while let Some(at) = stack.pop() {
        match at {
            At::Property(property) => {
                found.push(property);
                stack.extend(property.value.as_ref().map(At::Value));
            }
            At::Value(Node { kind: NodeKind::Object(properties), .. }) => stack.extend(properties.iter().rev().map(At::Property)),
            At::Value(Node { kind: NodeKind::Array(items), .. }) => stack.extend(items.iter().rev().map(At::Value)),
            At::Value(_) | At::Key(_) => {}
        }
    }
    found
}

/// `isInComment`: whether the token at `offset`, scanning from `start`, is a comment.
fn in_comment(text: &str, start: usize, offset: usize) -> bool {
    let mut scanner = Scanner::new(text);
    scanner.pos = start;
    let mut token = scanner.scan();
    while token != Token::Eof && scanner.token_offset + scanner.token_len() < offset {
        token = scanner.scan();
    }
    matches!(token, Token::LineComment | Token::BlockComment) && scanner.token_offset <= offset
}

/// `evaluateSeparatorAfter`: a `,` unless the next token (past whitespace and comments) ends
/// the list, or the text.
fn separator_after(text: &str, offset: usize) -> &'static str {
    let mut scanner = Scanner::new(text);
    scanner.pos = offset;
    let token = loop {
        match scanner.scan() {
            Token::LineComment | Token::BlockComment | Token::LineBreak | Token::Trivia => {}
            token => break token,
        }
    };
    match token {
        Token::Comma | Token::CloseBrace | Token::CloseBracket | Token::Eof => "",
        _ => ",",
    }
}

/// `getCurrentWord`: the text before `offset` back to whitespace or a JSON delimiter.
fn current_word(text: &str, offset: usize) -> &str {
    let start = text[..offset].rfind([' ', '\t', '\n', '\r', '\u{b}', '"', ':', '{', '[', ',', ']', '}']).map_or(0, |at| at + 1);
    &text[start..offset]
}

/// `JSON.stringify` of a string.
fn stringify(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `getInsertTextForPlainText`: `text` as a snippet that inserts it.
fn plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '$' | '}') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// A key as JavaScript holds it.
fn key_text(property: &PropertyNode) -> String {
    String::from_utf16_lossy(&property.key.value)
}

/// A proposal, before the suggest widget filters it.
struct Proposal {
    label: String,
    /// What is typed is matched with: the item's `filterText`, else its label.
    filter: String,
    snippet: String,
}

/// The worker's `collector`: one proposal per label.
#[derive(Default)]
struct Collector {
    proposed: HashSet<String>,
    items: Vec<Proposal>,
}

impl Collector {
    /// Propose nothing labelled `label` (a key the object already has).
    fn block(&mut self, label: String) {
        self.proposed.insert(label);
    }

    /// A label over 60 characters is cut to 57 and `...`, unless that is proposed already.
    fn add(&mut self, label: String, filter: Option<String>, snippet: String) {
        if self.proposed.contains(&label) {
            return;
        }
        let mut label = label.replace('\n', "\u{21b5}");
        if label.chars().count() > 60 {
            let short = format!("{}...", label.chars().take(57).collect::<String>().trim());
            if !self.proposed.contains(&short) {
                label = short;
            }
        }
        self.proposed.insert(label.clone());
        let filter = filter.unwrap_or_else(|| label.clone());
        self.items.push(Proposal { label, filter, snippet });
    }

    /// The keys of `object`, a similar object (`collectCompletionsForSimilarObject`).
    fn add_keys_of(&mut self, object: &Node) {
        let NodeKind::Object(properties) = &object.kind else {
            return;
        };
        for property in properties {
            let key = key_text(property);
            let quoted = stringify(&key);
            self.add(key, Some(quoted.clone()), plain(&quoted));
        }
    }

    /// `value`, the value of a node whose bytes are `parent` (unless they hold the cursor at
    /// `offset`), and the other boolean (`collectSuggestionsForValues`).
    fn add_value(&mut self, text: &str, value: &Node, parent: &Range<usize>, offset: usize, separator: &str) {
        if !touches(parent, offset) {
            let (label, snippet) = match &value.kind {
                NodeKind::Array(_) => ("[]".to_owned(), format!("[$1]{separator}")),
                NodeKind::Object(_) => ("{}".to_owned(), format!("{{$1}}{separator}")),
                NodeKind::String(_) | NodeKind::Number | NodeKind::Boolean(_) | NodeKind::Null => {
                    let content = &text[value.range.clone()];
                    (content.to_owned(), plain(&format!("{content}{separator}")))
                }
            };
            self.add(label, None, snippet);
        }
        if let NodeKind::Boolean(value) = value.kind {
            let other = (!value).to_string();
            self.add(other.clone(), None, plain(&format!("{other}{separator}")));
        }
    }
}

/// Whether typing the character before `offset` asks the JSON worker: a trigger character
/// does, and so does ending a word that is not a number (Monaco's quick suggestions).
fn asks(text: &str, offset: usize) -> bool {
    let typed = text.get(..offset).and_then(|before| before.chars().next_back());
    typed.is_some_and(|typed| TRIGGER_CHARACTERS.contains(&typed)) || words::ends_word(text, offset, "json")
}

/// What the JSON worker completes at byte `offset` of `text`, a `json` file, best matches of
/// what was typed first (what the proposals replace, up to the cursor), then by label. `None`
/// where it proposes nothing, and Monaco's word suggestions take over; an empty list where
/// what was typed matches none of its proposals, which leaves nothing to suggest.
pub fn completions(text: &str, offset: usize, trigger: Trigger) -> Option<Vec<Completion>> {
    proposals(text, offset, trigger).map(|(_, items)| items)
}

/// [`completions`], and the bytes they replace.
fn proposals(text: &str, offset: usize, trigger: Trigger) -> Option<(Range<usize>, Vec<Completion>)> {
    if trigger == Trigger::Typing && !asks(text, offset) {
        return None;
    }
    let parsed = parse(text)?;
    let root = parsed.root.as_ref();
    if schema_of(root).is_some() {
        return None;
    }
    let mut path = root.map_or_else(Vec::new, |root| path_at(root, offset));
    if in_comment(text, path.last().map_or(0, |at| at.range().start), offset) {
        return None;
    }
    // Right after a `}` or `]`: the object or array's parent.
    if let Some(At::Value(node)) = path.last()
        && offset == node.range.end
        && offset > 0
        && matches!((&node.kind, text.as_bytes()[offset - 1]), (NodeKind::Object(_), b'}') | (NodeKind::Array(_), b']'))
    {
        path.pop();
    }
    let word = current_word(text, offset);
    let replace = match path.last() {
        Some(At::Key(property)) => property.key.range.clone(),
        Some(At::Value(Node { range, kind: NodeKind::String(_) | NodeKind::Number | NodeKind::Boolean(_) | NodeKind::Null })) => {
            range.clone()
        }
        Some(At::Value(_) | At::Property(_)) | None => {
            let start = offset - word.len();
            let start = if start > 0 && text.as_bytes()[start - 1] == b'"' { start - 1 } else { start };
            start..offset
        }
    };
    // Where in `path` the node completions are for is, if anywhere.
    let mut index = path.len().checked_sub(1);
    let mut current = None;
    if let Some(At::Key(property)) = path.last() {
        current = Some(*property);
        index = path.len().checked_sub(3);
    }
    let parent_of = |index: Option<usize>| index.and_then(|index| index.checked_sub(1)).map(|index| path[index]);
    let all_properties = properties(root);
    let mut collector = Collector::default();

    // Keys.
    if let Some(At::Value(object @ Node { kind: NodeKind::Object(own), .. })) = index.map(|index| path[index]) {
        if object.range.start == offset {
            return None;
        }
        for property in own {
            if !current.is_some_and(|current| std::ptr::eq(current, property)) {
                collector.block(key_text(property));
            }
        }
        match parent_of(index) {
            Some(At::Property(parent)) => {
                for property in &all_properties {
                    if !std::ptr::eq(*property, parent) && property.key.value == parent.key.value {
                        property.value.iter().for_each(|value| collector.add_keys_of(value));
                    }
                }
            }
            Some(At::Value(Node { kind: NodeKind::Array(items), .. })) => {
                for item in items.iter().filter(|item| !std::ptr::eq(*item, object)) {
                    collector.add_keys_of(item);
                }
            }
            Some(At::Value(_) | At::Key(_)) => {}
            None => collector.add("$schema".to_owned(), Some(stringify("$schema")), "\"\\$schema\": $1".to_owned()),
        }
        let before_word = offset - word.len();
        if !word.is_empty() && (before_word == 0 || text.as_bytes()[before_word - 1] != b'"') {
            let quoted = stringify(word);
            collector.add(quoted.clone(), None, plain(&quoted));
        }
    }

    // Values.
    let mut separator_at = offset;
    if let Some(At::Value(Node { range, kind: NodeKind::String(_) | NodeKind::Number | NodeKind::Boolean(_) | NodeKind::Null })) =
        index.map(|index| path[index])
    {
        separator_at = range.end;
        index = index.and_then(|index| index.checked_sub(1));
    }
    match index.map(|index| path[index]) {
        None => {
            collector.add("Empty object".to_owned(), None, "{$1}".to_owned());
            collector.add("Empty array".to_owned(), None, "[$1]".to_owned());
        }
        Some(At::Property(property)) => {
            let separator = separator_after(text, separator_at);
            let past_value = property.value.as_ref().is_some_and(|value| {
                offset > value.range.end || matches!(value.kind, NodeKind::Object(_) | NodeKind::Array(_))
            });
            // jsonParser's `colonOffset` is -1 without a colon: past it anywhere.
            if property.colon.is_none_or(|colon| offset > colon) && !past_value {
                for same in all_properties.iter().filter(|same| same.key.value == property.key.value) {
                    if let Some(value) = &same.value {
                        collector.add_value(text, value, &same.range, offset, separator);
                    }
                }
                // `$schema` in the root object would offer the schemas registered with the
                // worker, and Monaco registers none.
            }
        }
        Some(At::Value(array @ Node { kind: NodeKind::Array(items), .. })) => {
            let separator = separator_after(text, separator_at);
            match parent_of(index) {
                Some(At::Property(parent)) => {
                    for same in all_properties.iter().filter(|same| same.key.value == parent.key.value) {
                        if let Some(value @ Node { kind: NodeKind::Array(items), .. }) = &same.value {
                            for item in items {
                                collector.add_value(text, item, &value.range, offset, separator);
                            }
                        }
                    }
                }
                Some(At::Value(_) | At::Key(_)) | None => {
                    for item in items {
                        collector.add_value(text, item, &array.range, offset, separator);
                    }
                }
            }
        }
        Some(At::Value(_) | At::Key(_)) => {}
    }

    if collector.items.is_empty() {
        return None;
    }
    let typed = &text[replace.start..offset.max(replace.start)];
    let mut ranked: Vec<(i32, Proposal)> = collector
        .items
        .into_iter()
        .filter_map(|proposal| {
            let score = if typed.is_empty() { 0 } else { fuzzy::score(typed, &proposal.filter)?.score };
            Some((score, proposal))
        })
        .collect();
    ranked.sort_by(|(score_a, a), (score_b, b)| score_b.cmp(score_a).then_with(|| a.label.cmp(&b.label)));
    let items = ranked
        .into_iter()
        .map(|(_, proposal)| Completion {
            label: proposal.label,
            replace: replace.clone(),
            insert: Insert::Snippet(proposal.snippet),
            then: Then::Done,
        })
        .collect();
    Some((replace, items))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is offered at `|` in `text` (asked for), as `(label, snippet)`, and the text a pick
    /// replaces; `None` where the worker proposes nothing.
    fn offered(text: &str) -> Option<(String, Vec<(String, String)>)> {
        offered_on(text, Trigger::Invoked)
    }

    fn offered_on(text: &str, trigger: Trigger) -> Option<(String, Vec<(String, String)>)> {
        let offset = text.find('|').expect("a cursor");
        let text = text.replace('|', "");
        let (replace, found) = proposals(&text, offset, trigger)?;
        let items = found
            .into_iter()
            .map(|item| match item.insert {
                Insert::Snippet(snippet) => (item.label, snippet),
                Insert::Text(text) => panic!("a snippet, not {text}"),
            })
            .collect();
        Some((text[replace].to_owned(), items))
    }

    fn item(label: &str, snippet: &str) -> (String, String) {
        (label.to_owned(), snippet.to_owned())
    }

    /// Checked against vscode-json-languageservice's `doComplete` as Monaco's worker sets it up.
    #[test]
    fn keys_of_similar_objects_are_offered_but_those_the_object_has() {
        let text = r#"{"a": {"x": 1, "y": 2}, "b": [{"p": 1, "q": 2}, {"p": 3, |}], "c": {"a": {"z": 0, "x": 5}}}"#;
        assert_eq!(offered(text), Some((String::new(), vec![item("q", "\"q\"")])));
        // Under a property of the same name, anywhere in the document.
        let text = r#"{"a": {"x": 1, "y": 2}, "c": {"a": {"z": 0, "|"}}}"#;
        assert_eq!(offered(text), Some(("\"\"".to_owned(), vec![item("x", "\"x\""), item("y", "\"y\"")])));
        // Typed: the best matches of the quoted key.
        let text = r#"{"a": {"xa": 1, "ya": 2}, "c": {"a": {"y|"}}}"#;
        assert_eq!(offered(text), Some(("\"y\"".to_owned(), vec![item("ya", "\"ya\"")])));
    }

    #[test]
    fn the_root_object_offers_schema_and_an_unquoted_word_quoted() {
        assert_eq!(offered("{|}"), Some((String::new(), vec![item("$schema", "\"\\$schema\": $1")])));
        assert_eq!(offered("{\"a\": 1, na|}"), Some(("na".to_owned(), vec![item("\"na\"", "\"na\"")])));
        // Not before the opening brace.
        assert_eq!(offered("|{}"), None);
    }

    #[test]
    fn values_the_key_has_elsewhere_are_offered_with_a_separator_when_more_follows() {
        let text = "[{\"k\": \"one\", \"b\": true}, {\"k\": 2, \"b\": null}, {\"k\": |\n\"n\": 0}]";
        assert_eq!(offered(text), Some((String::new(), vec![item("\"one\"", "\"one\","), item("2", "2,")])));
        let text = r#"[{"k": "one"}, {"k": |, "n": 0}]"#;
        assert_eq!(offered(text), Some((String::new(), vec![item("\"one\"", "\"one\"")])));
        // The other boolean, also where the value itself holds the cursor.
        let text = r#"{"x": {"b": false}, "b": t|}"#;
        assert_eq!(offered(text), Some(("t".to_owned(), vec![item("true", "true")])));
        let text = r#"[{"b": true}, {"b": "a|"}]"#;
        assert_eq!(offered(text), Some(("\"a\"".to_owned(), Vec::new())));
        // Objects and arrays as empty ones.
        let text = r#"[{"v": {"a": 1}}, {"v": [1]}, {"v": |}]"#;
        assert_eq!(offered(text), Some((String::new(), vec![item("[]", "[$1]"), item("{}", "{$1}")])));
    }

    #[test]
    fn items_of_arrays_at_the_same_key_are_offered() {
        let text = r#"{"a": {"tags": ["x", "y"]}, "tags": ["z", |]}"#;
        assert_eq!(offered(text), Some((String::new(), vec![item("\"x\"", "\"x\""), item("\"y\"", "\"y\"")])));
        // In an array of its own, only the other boolean: its items hold the cursor.
        assert_eq!(offered("[true, |]"), Some((String::new(), vec![item("false", "false")])));
        assert_eq!(offered("[1, |]"), None);
    }

    #[test]
    fn an_empty_file_offers_an_empty_object_and_array() {
        assert_eq!(offered("|"), Some((String::new(), vec![item("Empty array", "[$1]"), item("Empty object", "{$1}")])));
        assert_eq!(offered("{}|"), Some((String::new(), vec![item("Empty array", "[$1]"), item("Empty object", "{$1}")])));
    }

    #[test]
    fn nothing_in_comments_or_with_a_schema() {
        assert_eq!(offered("{\"a\": 1, // x|\n}"), None);
        assert_eq!(offered("{\"$schema\": \"x\", |}"), None);
        assert_eq!(offered(&format!("{}|", "[".repeat(1000))), None);
    }

    #[test]
    fn typing_asks_at_a_trigger_character_or_the_end_of_a_word_that_is_no_number() {
        let text = r#"{"a": {"x": 1}, "c": {"a": {"|"#;
        assert!(offered_on(text, Trigger::Typing).is_some());
        let text = r#"{"a": {"x": 1}, "b": {x|"#;
        assert!(offered_on(text, Trigger::Typing).is_some());
        assert_eq!(offered_on("{\"a\": 1, \"b\": {\"a\": 12|", Trigger::Typing), None);
        assert_eq!(offered_on("{\"a\": 1, \"b\": {\"a\": 1,|", Trigger::Typing), None);
    }

    #[test]
    fn long_labels_are_cut_and_keys_are_quoted_as_json() {
        let long = "x".repeat(70);
        let text = format!(r#"[{{"k": "{long}"}}, {{"k": |}}]"#);
        let (_, items) = offered(&text).expect("offered");
        assert_eq!(items[0].0, format!("\"{}...", "x".repeat(56)));
        let text = r#"[{"a\"b$": 1}, {|}]"#;
        assert_eq!(offered(text), Some((String::new(), vec![item("a\"b$", "\"a\\\\\"b\\$\"")])));
    }
}
