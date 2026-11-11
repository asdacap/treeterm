//! `JsonViewer.tsx`: a JSON value as a foldable tree. The root starts open and everything
//! below it folded; clicking a bracket line flips it.

use std::collections::HashSet;

use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, ClickEvent, SharedString, Window, div, px, rgb};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Name(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Null,
    Str(String),
    Num(String),
    Bool(bool),
    /// `{}` or `[]`.
    Empty(&'static str),
    /// `▸ { 3 items }`; `path` flips it open.
    Folded { path: String, brackets: (char, char), count: usize },
    /// `▾ {`; its children and a [`Body::Close`] follow.
    Open { path: String, bracket: char },
    Close(char),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub depth: usize,
    pub key: Option<Key>,
    pub body: Body,
    pub comma: bool,
}

/// `toggled` holds the paths flipped from their default (only the root starts open).
pub fn flatten(value: &Value, toggled: &HashSet<String>) -> Vec<Line> {
    let mut lines = Vec::new();
    push(value, String::new(), 0, None, false, toggled, &mut lines);
    lines
}

fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn push(
    value: &Value,
    path: String,
    depth: usize,
    key: Option<Key>,
    comma: bool,
    toggled: &HashSet<String>,
    lines: &mut Vec<Line>,
) {
    let line = |body| Line { depth, key: key.clone(), body, comma };
    let children: Vec<(Key, String, &Value)> = match value {
        Value::Null => return lines.push(line(Body::Null)),
        Value::String(text) => return lines.push(line(Body::Str(text.clone()))),
        Value::Number(number) => return lines.push(line(Body::Num(number.to_string()))),
        Value::Bool(flag) => return lines.push(line(Body::Bool(*flag))),
        Value::Array(items) => items
            .iter()
            .enumerate()
            .map(|(i, item)| (Key::Index(i), child_path(&path, &i.to_string()), item))
            .collect(),
        Value::Object(map) => map
            .iter()
            .map(|(name, item)| (Key::Name(name.clone()), child_path(&path, name), item))
            .collect(),
    };
    let brackets = match value {
        Value::Array(_) => ('[', ']'),
        Value::Null | Value::String(_) | Value::Number(_) | Value::Bool(_) | Value::Object(_) => ('{', '}'),
    };
    if children.is_empty() {
        return lines.push(line(Body::Empty(if brackets.0 == '[' { "[]" } else { "{}" })));
    }
    let expanded = (depth == 0) != toggled.contains(&path);
    if !expanded {
        return lines.push(line(Body::Folded { path, brackets, count: children.len() }));
    }
    lines.push(Line { depth, key: key.clone(), body: Body::Open { path, bracket: brackets.0 }, comma: false });
    let last = children.len() - 1;
    for (i, (child_key, child_path, child)) in children.into_iter().enumerate() {
        push(child, child_path, depth + 1, Some(child_key), i < last, toggled, lines);
    }
    lines.push(Line { depth, key: None, body: Body::Close(brackets.1), comma });
}

/// The tree's rows; clicking a foldable row calls `on_toggle` with its path.
pub fn render(
    lines: Vec<Line>,
    on_toggle: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
) -> Vec<AnyElement> {
    const KEY: u32 = 0x9cdcfe;
    const STRING: u32 = 0xce9178;
    const NUMBER: u32 = 0xb5cea8;
    const LITERAL: u32 = 0x569cd6;
    const PUNCT: u32 = 0x858585;
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            let mut row = h_flex().id(SharedString::from(format!("json-{i}"))).w_full().items_start().pl(px(16. * line.depth as f32));
            row = match &line.key {
                Some(Key::Name(name)) => row.child(div().text_color(rgb(KEY)).child(format!("\"{name}\""))).child(div().text_color(rgb(PUNCT)).child(": ")),
                Some(Key::Index(index)) => row.child(div().text_color(rgb(PUNCT)).child(format!("{index}: "))),
                None => row,
            };
            let toggle = |row: gpui_kit::Stateful<gpui_kit::Div>, path: &String| {
                let (path, on_toggle) = (path.clone(), on_toggle.clone());
                row.cursor_pointer().on_click(move |_: &ClickEvent, window, cx| on_toggle(path.clone(), window, cx))
            };
            row = match &line.body {
                Body::Null => row.child(div().text_color(rgb(LITERAL)).child("null")),
                Body::Bool(flag) => row.child(div().text_color(rgb(LITERAL)).child(flag.to_string())),
                Body::Num(number) => row.child(div().text_color(rgb(NUMBER)).child(number.clone())),
                // The comma rides with a string so it stays next to the text when it wraps.
                Body::Str(text) => {
                    let comma = if line.comma { "," } else { "" };
                    row.child(div().min_w_0().text_color(rgb(STRING)).child(format!("\"{text}\"{comma}")))
                }
                Body::Empty(brackets) => row.child(div().text_color(rgb(PUNCT)).child(*brackets)),
                Body::Folded { path, brackets, count } => {
                    let noun = if *count == 1 { "item" } else { "items" };
                    toggle(row, path).child(div().text_color(rgb(PUNCT)).child(format!("▸ {} {count} {noun} {}", brackets.0, brackets.1)))
                }
                Body::Open { path, bracket } => toggle(row, path).child(div().text_color(rgb(PUNCT)).child(format!("▾ {bracket}"))),
                Body::Close(bracket) => row.child(div().text_color(rgb(PUNCT)).child(bracket.to_string())),
            };
            let separate_comma = line.comma && !matches!(line.body, Body::Str(_));
            row.when(separate_comma, |row| row.child(div().text_color(rgb(PUNCT)).child(","))).into_any_element()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn bodies(lines: &[Line]) -> Vec<String> {
        lines
            .iter()
            .map(|line| {
                let key = match &line.key {
                    Some(Key::Name(name)) => format!("{name}: "),
                    Some(Key::Index(i)) => format!("{i}: "),
                    None => String::new(),
                };
                let body = match &line.body {
                    Body::Null => "null".to_owned(),
                    Body::Str(text) => format!("\"{text}\""),
                    Body::Num(number) => number.clone(),
                    Body::Bool(flag) => flag.to_string(),
                    Body::Empty(brackets) => (*brackets).to_owned(),
                    Body::Folded { count, .. } => format!("folded({count})"),
                    Body::Open { bracket, .. } => bracket.to_string(),
                    Body::Close(bracket) => bracket.to_string(),
                };
                format!("{}{key}{body}{}", "  ".repeat(line.depth), if line.comma { "," } else { "" })
            })
            .collect()
    }

    #[test]
    fn the_root_is_open_and_children_folded() {
        let value = json!({ "a": 1, "b": { "c": true }, "d": [], "e": null });
        assert_eq!(
            bodies(&flatten(&value, &HashSet::new())),
            ["{", "  a: 1,", "  b: folded(1),", "  d: [],", "  e: null", "}"]
        );
    }

    #[test]
    fn toggling_opens_a_child_and_folds_the_root() {
        let value = json!({ "b": { "c": "x", "d": [1, 2] } });
        let open: HashSet<String> = ["/b".to_owned()].into();
        assert_eq!(
            bodies(&flatten(&value, &open)),
            ["{", "  b: {", "    c: \"x\",", "    d: folded(2)", "  }", "}"]
        );
        let folded: HashSet<String> = [String::new()].into();
        assert_eq!(bodies(&flatten(&value, &folded)), ["folded(1)"]);
    }

    #[test]
    fn keys_with_slashes_get_their_own_paths() {
        let value = json!({ "a/b": { "x": 1 }, "a": { "b": { "y": 2 } } });
        let open: HashSet<String> = ["/a~1b".to_owned()].into();
        let lines = flatten(&value, &open);
        assert!(bodies(&lines).contains(&"    x: 1".to_owned()));
        // Insertion order: "a" is last, so it has no comma.
        assert!(bodies(&lines).contains(&"  a: folded(1)".to_owned()));
    }

    #[test]
    fn a_scalar_root_is_one_line() {
        assert_eq!(bodies(&flatten(&json!("hi"), &HashSet::new())), ["\"hi\""]);
    }
}
