//! `JsonViewer.tsx`: a JSON value as a foldable tree. The root starts open and everything
//! below it folded; a toggle (or a folded value's "N items") flips it.

use std::collections::HashSet;

use gpui_kit::component::v_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, FontStyle, HighlightStyle, Hsla, SharedString, StyledText, Window, div, px, rgb,
    rgba,
};
use serde_json::Value;

use crate::ui::MONO;

#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    Name(String),
    Index(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Null,
    Str(String),
    Num(String),
    Bool(bool),
    /// `{}` or `[]`.
    Empty(&'static str),
    /// `▸{3 items}`; `path` flips it open.
    Folded { path: String, brackets: (char, char), count: usize },
    /// `▾{`, its rows, `}`.
    Open { path: String, brackets: (char, char), children: Vec<(Key, Node)> },
}

/// `toggled` holds the paths flipped from their default (only the root starts open).
pub fn tree(value: &Value, toggled: &HashSet<String>) -> Node {
    node(value, String::new(), 0, toggled)
}

fn child_path(parent: &str, key: &str) -> String {
    format!("{parent}/{}", key.replace('~', "~0").replace('/', "~1"))
}

fn node(value: &Value, path: String, depth: usize, toggled: &HashSet<String>) -> Node {
    let children: Vec<(Key, String, &Value)> = match value {
        Value::Null => return Node::Null,
        Value::String(text) => return Node::Str(text.clone()),
        Value::Number(number) => return Node::Num(number.to_string()),
        Value::Bool(flag) => return Node::Bool(*flag),
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
        return Node::Empty(if brackets.0 == '[' { "[]" } else { "{}" });
    }
    if (depth == 0) == toggled.contains(&path) {
        return Node::Folded { path, brackets, count: children.len() };
    }
    let children = children
        .into_iter()
        .map(|(key, child_path, child)| (key, node(child, child_path, depth + 1, toggled)))
        .collect();
    Node::Open { path, brackets, children }
}

const KEY: u32 = 0x9cdcfe;
const STRING: u32 = 0xce9178;
const NUMBER: u32 = 0xb5cea8;
const LITERAL: u32 = 0x569cd6;

/// Colours and the toggle callback, shared by every row.
struct Palette<F> {
    punct: Hsla,
    primary: Hsla,
    on_toggle: F,
}

/// `.json-viewer`: monospace 12px, line-height 1.6, padded 8px 12px.
pub fn render(
    node: Node,
    secondary: Hsla,
    primary: Hsla,
    on_toggle: impl Fn(String, &mut Window, &mut App) + Clone + 'static,
) -> AnyElement {
    let palette = Palette { punct: secondary, primary, on_toggle };
    let mut counter = 0;
    let body = value(&palette, None, node, false, &mut counter);
    div()
        .px(px(12.))
        .py(px(8.))
        .font_family(MONO)
        .text_size(px(12.))
        .line_height(px(19.2))
        .child(body)
        .into_any_element()
}

/// One `.json-viewer-row`'s text (`"key": value,`) as inline runs, so a long string wraps
/// under the key like the DOM's inline spans.
fn scalar_text(key: Option<&Key>, body: &str, body_color: Hsla, italic: bool, comma: bool, punct: Hsla) -> StyledText {
    let mut text = String::new();
    let mut runs: Vec<(std::ops::Range<usize>, HighlightStyle)> = Vec::new();
    let mut push = |text: &mut String, part: &str, color: Hsla, italic: bool| {
        let start = text.len();
        text.push_str(part);
        let style = HighlightStyle {
            color: Some(color),
            font_style: italic.then_some(FontStyle::Italic),
            ..Default::default()
        };
        runs.push((start..text.len(), style));
    };
    match key {
        Some(Key::Name(name)) => push(&mut text, &format!("\"{name}\""), rgb(KEY).into(), false),
        Some(Key::Index(index)) => push(&mut text, &index.to_string(), punct, false),
        None => {}
    }
    if key.is_some() {
        push(&mut text, ": ", punct, false);
    }
    push(&mut text, body, body_color, italic);
    if comma {
        push(&mut text, ",", punct, false);
    }
    StyledText::new(text).with_highlights(runs)
}

/// The key and colon in front of a collection's opening line.
fn key_prefix(key: Option<&Key>, punct: Hsla) -> Option<AnyElement> {
    let key = key?;
    Some(scalar_text(Some(key), "", punct, false, false, punct).into_any_element())
}

fn value<F: Fn(String, &mut Window, &mut App) + Clone + 'static>(
    palette: &Palette<F>,
    key: Option<Key>,
    node: Node,
    comma: bool,
    counter: &mut usize,
) -> AnyElement {
    *counter += 1;
    let id = *counter;
    let punct = palette.punct;
    // Rows (not the root) are padded 1px and tint on hover.
    let row = |content: gpui_kit::Stateful<gpui_kit::Div>| {
        content.when(key.is_some(), |d| d.py(px(1.)).rounded(px(2.)).hover(|d| d.bg(rgba(0xffffff0a))))
    };
    let base = div().id(("json-row", id));
    let toggle = |path: String, glyph: &'static str, on: &F| {
        let on = on.clone();
        div()
            .id(SharedString::from(format!("json-toggle-{id}")))
            .pr(px(4.))
            .text_size(px(10.))
            .text_color(punct)
            .cursor_pointer()
            .hover({
                let primary = palette.primary;
                move |d| d.text_color(primary)
            })
            .child(glyph)
            .on_click(move |_: &ClickEvent, window, cx| on(path.clone(), window, cx))
    };
    let (body, color, italic) = match node {
        Node::Null => ("null".to_owned(), rgb(LITERAL).into(), true),
        Node::Bool(flag) => (flag.to_string(), rgb(LITERAL).into(), false),
        Node::Num(number) => (number, rgb(NUMBER).into(), false),
        Node::Str(text) => (format!("\"{text}\""), rgb(STRING).into(), false),
        Node::Empty(brackets) => (brackets.to_owned(), punct, false),
        Node::Folded { path, brackets, count } => {
            let noun = if count == 1 { "item" } else { "items" };
            let on = palette.on_toggle.clone();
            let summary_path = path.clone();
            let primary = palette.primary;
            let line = div()
                .flex()
                .items_center()
                .flex_wrap()
                .children(key_prefix(key.as_ref(), punct))
                .child(toggle(path, "▸", &palette.on_toggle))
                .child(div().text_color(punct).child(brackets.0.to_string()))
                .child(
                    div()
                        .id(SharedString::from(format!("json-summary-{id}")))
                        .px(px(4.))
                        .text_size(px(11.))
                        .italic()
                        .text_color(punct)
                        .cursor_pointer()
                        .hover(move |d| d.text_color(primary))
                        .child(format!("{count} {noun}"))
                        .on_click(move |_: &ClickEvent, window, cx| on(summary_path.clone(), window, cx)),
                )
                .child(div().text_color(punct).child(format!("{}{}", brackets.1, if comma { "," } else { "" })));
            return row(base.child(line)).into_any_element();
        }
        Node::Open { path, brackets, children } => {
            let last = children.len().saturating_sub(1);
            let mut rows = v_flex().pl(px(16.));
            for (i, (child_key, child)) in children.into_iter().enumerate() {
                rows = rows.child(value(palette, Some(child_key), child, i < last, counter));
            }
            let opening = div()
                .flex()
                .items_center()
                .children(key_prefix(key.as_ref(), punct))
                .child(toggle(path, "▾", &palette.on_toggle))
                .child(div().text_color(punct).child(brackets.0.to_string()));
            let closing = div().text_color(punct).child(format!("{}{}", brackets.1, if comma { "," } else { "" }));
            return row(base.child(opening).child(rows).child(closing)).into_any_element();
        }
    };
    row(base.child(scalar_text(key.as_ref(), &body, color, italic, comma, punct))).into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The tree as text: one line per row, folded ones as `folded(n)`.
    fn lines(node: &Node) -> Vec<String> {
        fn walk(node: &Node, prefix: String, depth: usize, comma: bool, out: &mut Vec<String>) {
            let pad = "  ".repeat(depth);
            let comma = if comma { "," } else { "" };
            let body = match node {
                Node::Null => "null".to_owned(),
                Node::Str(text) => format!("\"{text}\""),
                Node::Num(number) => number.clone(),
                Node::Bool(flag) => flag.to_string(),
                Node::Empty(brackets) => (*brackets).to_owned(),
                Node::Folded { count, .. } => format!("folded({count})"),
                Node::Open { brackets, children, .. } => {
                    out.push(format!("{pad}{prefix}{}", brackets.0));
                    let last = children.len() - 1;
                    for (i, (key, child)) in children.iter().enumerate() {
                        let key = match key {
                            Key::Name(name) => format!("{name}: "),
                            Key::Index(i) => format!("{i}: "),
                        };
                        walk(child, key, depth + 1, i < last, out);
                    }
                    out.push(format!("{pad}{}{comma}", brackets.1));
                    return;
                }
            };
            out.push(format!("{pad}{prefix}{body}{comma}"));
        }
        let mut out = Vec::new();
        walk(node, String::new(), 0, false, &mut out);
        out
    }

    #[test]
    fn the_root_is_open_and_children_folded() {
        let value = json!({ "a": 1, "b": { "c": true }, "d": [], "e": null });
        assert_eq!(lines(&tree(&value, &HashSet::new())), ["{", "  a: 1,", "  b: folded(1),", "  d: [],", "  e: null", "}"]);
    }

    #[test]
    fn toggling_opens_a_child_and_folds_the_root() {
        let value = json!({ "b": { "c": "x", "d": [1, 2] } });
        let open: HashSet<String> = ["/b".to_owned()].into();
        assert_eq!(lines(&tree(&value, &open)), ["{", "  b: {", "    c: \"x\",", "    d: folded(2)", "  }", "}"]);
        let folded: HashSet<String> = [String::new()].into();
        assert_eq!(lines(&tree(&value, &folded)), ["folded(1)"]);
    }

    #[test]
    fn keys_with_slashes_get_their_own_paths() {
        let value = json!({ "a/b": { "x": 1 }, "a": { "b": { "y": 2 } } });
        let open: HashSet<String> = ["/a~1b".to_owned()].into();
        let shown = lines(&tree(&value, &open));
        assert!(shown.contains(&"    x: 1".to_owned()));
        // Insertion order: "a" is last, so it has no comma.
        assert!(shown.contains(&"  a: folded(1)".to_owned()));
    }

    #[test]
    fn a_scalar_root_is_one_line() {
        assert_eq!(lines(&tree(&json!("hi"), &HashSet::new())), ["\"hi\""]);
    }
}
