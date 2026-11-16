//! vscode-html-languageservice's parser (`parser/htmlParser.js`), ported: the elements of an
//! HTML document as a tree, each with where it starts and ends, whether its end tag closed it,
//! and its attributes.

use super::data;
use super::scanner::{Scanner, State, Token};

/// An index into [`Document::nodes`].
pub(crate) type NodeId = usize;

/// An element, or the document itself (the root, with no tag).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Node {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) children: Vec<NodeId>,
    pub(crate) parent: Option<NodeId>,
    /// Its name as written; `None` for the document, and for a `<` with no name after it.
    pub(crate) tag: Option<String>,
    /// Its end tag closed it, or it needs none.
    pub(crate) closed: bool,
    pub(crate) end_tag_start: Option<usize>,
    /// Its attributes, names as written, in the order first written (a later one of the same
    /// name takes its place); `None` for an attribute with no value.
    pub(crate) attributes: Vec<(String, Option<String>)>,
}

/// A parsed document.
pub(crate) struct Document {
    pub(crate) nodes: Vec<Node>,
}

/// The document's root.
pub(crate) const ROOT: NodeId = 0;

impl Document {
    /// `text` parsed (`HTMLParser.parse`).
    pub(crate) fn parse(text: &str) -> Self {
        let mut scanner = Scanner::new(text, 0, State::WithinContent, true);
        let mut nodes = vec![Node {
            start: 0,
            end: text.len(),
            children: Vec::new(),
            parent: None,
            tag: None,
            closed: false,
            end_tag_start: None,
            attributes: Vec::new(),
        }];
        let mut current = ROOT;
        let mut end_tag_start = 0;
        let mut end_tag_name: Option<String> = None;
        let mut pending_attribute: Option<String> = None;
        let mut token = scanner.scan();
        while token != Token::Eos {
            match token {
                Token::StartTagOpen => {
                    let child = nodes.len();
                    nodes.push(Node {
                        start: scanner.token_offset(),
                        end: text.len(),
                        children: Vec::new(),
                        parent: Some(current),
                        tag: None,
                        closed: false,
                        end_tag_start: None,
                        attributes: Vec::new(),
                    });
                    nodes[current].children.push(child);
                    current = child;
                }
                Token::StartTag => nodes[current].tag = Some(scanner.token_text().to_owned()),
                Token::StartTagClose => {
                    if let Some(parent) = nodes[current].parent {
                        nodes[current].end = scanner.token_end();
                        if scanner.token_end() > scanner.token_offset() {
                            if nodes[current].tag.as_deref().is_some_and(data::is_void) {
                                nodes[current].closed = true;
                                current = parent;
                            }
                        } else {
                            // The pseudo close of a start tag cut short.
                            current = parent;
                        }
                    }
                }
                Token::StartTagSelfClose => {
                    if let Some(parent) = nodes[current].parent {
                        nodes[current].closed = true;
                        nodes[current].end = scanner.token_end();
                        current = parent;
                    }
                }
                Token::EndTagOpen => {
                    end_tag_start = scanner.token_offset();
                    end_tag_name = None;
                }
                Token::EndTag => end_tag_name = Some(scanner.token_text().to_lowercase()),
                Token::EndTagClose => {
                    // The element it ends, if one is open.
                    let mut node = current;
                    while !is_same_tag(&nodes[node], end_tag_name.as_deref()) {
                        match nodes[node].parent {
                            Some(parent) => node = parent,
                            None => break,
                        }
                    }
                    if let Some(parent) = nodes[node].parent {
                        while current != node {
                            nodes[current].end = end_tag_start;
                            nodes[current].closed = false;
                            current = nodes[current].parent.unwrap_or(ROOT);
                        }
                        nodes[current].closed = true;
                        nodes[current].end_tag_start = Some(end_tag_start);
                        nodes[current].end = scanner.token_end();
                        current = parent;
                    }
                }
                Token::AttributeName => {
                    let name = scanner.token_text().to_owned();
                    let attributes = &mut nodes[current].attributes;
                    match attributes.iter_mut().find(|(seen, _)| *seen == name) {
                        Some((_, value)) => *value = None,
                        None => attributes.push((name.clone(), None)),
                    }
                    pending_attribute = Some(name);
                }
                Token::AttributeValue => {
                    if let Some(name) = pending_attribute.take() {
                        let value = scanner.token_text().to_owned();
                        if let Some((_, slot)) = nodes[current].attributes.iter_mut().find(|(seen, _)| *seen == name) {
                            *slot = Some(value);
                        }
                    }
                }
                _ => {}
            }
            token = scanner.scan();
        }
        while let Some(parent) = nodes[current].parent {
            nodes[current].end = text.len();
            nodes[current].closed = false;
            current = parent;
        }
        Self { nodes }
    }

    /// The first of `children` that `starts_after` holds for, or their count (`findFirst`: it
    /// holds for none before it).
    fn first(&self, children: &[NodeId], starts_after: impl Fn(&Node) -> bool) -> usize {
        children.partition_point(|&child| !starts_after(&self.nodes[child]))
    }

    /// The innermost element whose start is before `offset`, or that ends before it
    /// (`findNodeBefore`).
    pub(crate) fn node_before(&self, offset: usize) -> NodeId {
        let mut node = ROOT;
        loop {
            let children = &self.nodes[node].children;
            let index = self.first(children, |child| offset <= child.start);
            let Some(&child) = index.checked_sub(1).and_then(|index| children.get(index)) else {
                return node;
            };
            let found = &self.nodes[child];
            if offset <= found.start {
                return node;
            }
            let last_ends_with_it = found.children.last().is_some_and(|&last| self.nodes[last].end == found.end);
            if offset < found.end || last_ends_with_it {
                node = child;
                continue;
            }
            return child;
        }
    }

    /// The innermost element holding `offset`, its end included (`findNodeAt`).
    pub(crate) fn node_at(&self, offset: usize) -> NodeId {
        let mut node = ROOT;
        loop {
            let children = &self.nodes[node].children;
            let index = self.first(children, |child| offset <= child.start);
            match index.checked_sub(1).and_then(|index| children.get(index)) {
                Some(&child) if offset > self.nodes[child].start && offset <= self.nodes[child].end => node = child,
                Some(_) | None => return node,
            }
        }
    }
}

/// `Node.isSameTag`: `tag`, lower-cased, is the node's (none for the root).
fn is_same_tag(node: &Node, tag: Option<&str>) -> bool {
    match (&node.tag, tag) {
        (None, None) => true,
        (Some(own), Some(tag)) => own.len() == tag.len() && own.to_lowercase() == tag,
        (Some(_), None) | (None, Some(_)) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each element as `(tag, start..end, closed)`, in document order.
    fn elements(text: &str) -> Vec<(String, std::ops::Range<usize>, bool)> {
        let document = Document::parse(text);
        document.nodes[1..]
            .iter()
            .map(|node| (node.tag.clone().unwrap_or_default(), node.start..node.end, node.closed))
            .collect()
    }

    fn element(tag: &str, range: std::ops::Range<usize>, closed: bool) -> (String, std::ops::Range<usize>, bool) {
        (tag.to_owned(), range, closed)
    }

    #[test]
    fn elements_end_at_their_end_tag_or_where_an_outer_one_ends() {
        assert_eq!(elements("<div><p>a</div>"), [element("div", 0..15, true), element("p", 5..9, false)]);
        assert_eq!(elements("<br><img/><P></p>"), [
            element("br", 0..4, true),
            element("img", 4..10, true),
            element("P", 10..17, true),
        ]);
        // Unclosed to the end; a stray end tag closes nothing.
        assert_eq!(elements("<ul><li></b>"), [element("ul", 0..12, false), element("li", 4..12, false)]);
        // A start tag cut short by another.
        assert_eq!(elements("<a <b>"), [element("a", 0..3, false), element("b", 3..6, false)]);
    }

    #[test]
    fn attributes_keep_their_first_place_and_last_value() {
        let document = Document::parse("<a href=x title b='1' href>");
        let attribute = |name: &str, value: Option<&str>| (name.to_owned(), value.map(str::to_owned));
        assert_eq!(document.nodes[1].attributes, [
            attribute("href", None),
            attribute("title", None),
            attribute("b", Some("'1'")),
        ]);
    }

    #[test]
    fn the_node_before_or_at_an_offset_is_found() {
        let text = "<div><p>a</p> <br> </div>";
        let document = Document::parse(text);
        let tag = |node: NodeId| document.nodes[node].tag.clone().unwrap_or_default();
        assert_eq!(tag(document.node_before(6)), "p");
        // Past an element: that element.
        assert_eq!(tag(document.node_before(14)), "p");
        assert_eq!(tag(document.node_before(19)), "br");
        assert_eq!(tag(document.node_before(0)), "");
        assert_eq!(tag(document.node_at(9)), "p");
        assert_eq!(tag(document.node_at(13)), "p");
        assert_eq!(tag(document.node_at(14)), "div");
        assert_eq!(tag(document.node_at(25)), "div");
        assert_eq!(tag(document.node_at(0)), "");
    }
}
