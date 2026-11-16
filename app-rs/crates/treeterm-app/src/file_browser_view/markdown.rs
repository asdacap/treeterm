//! `MarkdownPreview`: GitHub-flavoured markdown styled as `.markdown-preview` in
//! `index.css` (16px text at 1.6 line height in 20px of padding; h1/h2 underlined; tertiary
//! code; accent links and quote bars; bordered tables).
//!
//! GPUI Kit's text view leaves a paragraph gap below each block and none above, where CSS
//! collapses `margin: 1em 0` between blocks: the 1em above the first block becomes extra
//! top padding, and a heading's `margin-top: 1.5em` becomes padding above it less the gap
//! the block before already left.
//!
//! react-markdown parses CommonMark and GFM, nothing more, and without `rehype-raw` it writes
//! raw HTML out as text: `<b>x</b>` shows as typed, and an HTML block as a run of text in its
//! place, which the browser runs together with the HTML blocks beside it. Nor does it know
//! math, so `$` is an ordinary character and a `$$` block an ordinary paragraph. And no image
//! loads: `urlTransform` empties `data:` URLs and the CSP (`img-src 'self' data: blob:`)
//! blocks the rest, so Chromium draws its broken-image icon and the `alt` text. GPUI Kit
//! renders HTML, parses math and loads images, so it is given the source with HTML inside
//! paragraphs and every `$` escaped ([`literal_source`]), and two plugins draw HTML blocks as
//! text ([`LiteralHtml`]) and every image as a broken one ([`BrokenImage`]).

use gpui_kit::assets::IconName;
use gpui_kit::base::{
    InlineElement, InlineRenderContext, MarkdownExtensions, MarkdownNode, MarkdownParseContext, MarkdownPlugin, TextView,
    TextViewStyle,
};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, FontWeight, HighlightStyle, Hsla, ScrollHandle, SharedString, StyleRefinement, Window, div, px,
    relative, rems, rgb,
};
use markdown::mdast::{Node, ReferenceKind};

use crate::theme::colors;

/// The body text size (`html` default) and the 1em margin around blocks.
const BODY: f32 = 16.;
/// `.markdown-preview { padding: 20px }`.
const PADDING: f32 = 20.;
/// `code { font-family: monospace; font-size: 0.9em }`: Chromium sizes the generic
/// monospace family from 13px, not 16px.
const CODE: f32 = 13. * 0.9;
const MONO: &str = "Menlo";

/// `h1`..`h6` at 2em, 1.5em, 1.25em, then the browser's 1em, 0.83em and 0.67em.
fn heading_size(level: u8) -> f32 {
    match level {
        1 => 32.,
        2 => 24.,
        3 => 20.,
        4 => 16.,
        5 => 13.28,
        _ => 10.72,
    }
}

/// A heading: semibold at its size; 1.5em above (less the gap the block before left),
/// 0.5em below (at least the next block's 1em); h1 and h2 underlined 0.3em below the text.
fn heading(level: u8, border: Hsla) -> StyleRefinement {
    let size = heading_size(level);
    let underlined = level <= 2;
    let style = StyleRefinement::default()
        .text_size(px(size))
        .line_height(relative(1.6))
        .font_weight(FontWeight::SEMIBOLD)
        .pt(px((1.5 * size - BODY).max(0.)))
        .pb(px(if underlined { 0.3 * size } else { 0. }))
        .mb(px((0.5 * size).max(BODY)));
    if underlined { style.border_b_1().border_color(border) } else { style }
}

fn style(cx: &App) -> TextViewStyle {
    let c = colors(cx);
    let (border, tertiary, secondary, accent) = (c.border, c.bg_tertiary, c.text_secondary, c.accent);
    TextViewStyle::default()
        .with_foreground(c.text_primary)
        .with_muted_foreground(c.text_secondary)
        .with_link(c.accent)
        .with_selection(rgb(0x264f78).into())
        .with_code_background(tertiary)
        .with_border(border)
        .with_paragraph_gap(rems(BODY / 16.))
        .with_heading(move |level| heading(level, border))
        // `pre`: 13px rows (the monospace default) at 1.6, the code in them at 0.9em of 13px.
        .with_code_block(
            StyleRefinement::default()
                .bg(tertiary)
                .border_0()
                .p(px(16.))
                .rounded(px(6.))
                .font_family(MONO)
                .text_size(px(CODE))
                .line_height(px(13. * 1.6)),
        )
        .with_inline_code(HighlightStyle { background_color: Some(tertiary), ..Default::default() })
        .with_inline_code_style(
            StyleRefinement::default()
                .font_family(MONO)
                .text_size(px(CODE))
                .bg(tertiary)
                .px(px(6.))
                .py(px(2.))
                .rounded(px(4.)),
        )
        // `ul { padding-left: 2em }`: an outside marker hangs in the first 32px.
        .with_list_style(StyleRefinement::default().min_w(px(2. * BODY)).pr(px(6.)).text_color(c.text_primary))
        .with_blockquote_style(StyleRefinement::default().border_l(px(4.)).border_color(accent).pl(px(16.)).text_color(secondary))
        .with_table(StyleRefinement::default().border_1().border_color(border))
        .with_table_head(StyleRefinement::default().bg(c.bg_secondary).font_weight(FontWeight::SEMIBOLD))
        .with_table_cell(StyleRefinement::default().px(px(12.)).py(px(8.)).border_color(border))
        .with_dark(true)
}

/// Where a link click goes instead of the system browser.
pub(crate) type LinkHandler = std::sync::Arc<dyn Fn(&str, &mut gpui_kit::Window, &mut App) + Send + Sync>;

/// The preview of `text`, scrolling in its own box (with Electron's classic scrollbar).
/// Links open in the system browser unless `on_link` takes them.
pub(crate) fn preview(
    id: SharedString,
    text: String,
    scroll: &ScrollHandle,
    on_link: Option<LinkHandler>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let source = literal(&id, text, window, cx);
    let c = colors(cx);
    let view = TextView::markdown(id, source)
        .style(style(cx))
        .markdown_extensions(MarkdownExtensions::default().plugin(LiteralHtml).plugin(BrokenImage))
        // react-markdown neither highlights code blocks nor labels their language.
        .code_block_highlighter(|_| Vec::new())
        .code_block_actions(|_, _, _| div());
    let view = match on_link {
        Some(on_link) => view.on_link_click(move |href, _, window, cx| on_link(href.as_ref(), window, cx)),
        None => view,
    };
    let content = div()
        .w_full()
        .px(px(PADDING))
        .py(px(PADDING + BODY))
        .text_color(c.text_primary)
        .text_size(px(BODY))
        .line_height(relative(1.6))
        .child(view);
    div().size_full().bg(c.bg_primary).child(crate::ui::scroll_area("markdown-preview", scroll, content, cx)).into_any_element()
}

/// The last text a preview was given and its [`literal_source`].
#[derive(Default)]
struct Literal {
    text: String,
    source: SharedString,
}

/// [`literal_source`] of `text`, worked out again only when the text changes: it parses the
/// whole document, and the preview draws every frame.
fn literal(id: &SharedString, text: String, window: &mut Window, cx: &mut App) -> SharedString {
    let memo = window.use_keyed_state(SharedString::from(format!("{id}/literal")), cx, |_, _| Literal::default());
    if memo.read(cx).text != text {
        let source = literal_source(&text).into();
        memo.update(cx, |memo, _| *memo = Literal { text, source });
    }
    memo.read(cx).source.clone()
}

/// `source` as GPUI Kit must read it to show what react-markdown shows: the HTML inside
/// paragraphs (and headings, table cells, link text) written out with each ASCII punctuation
/// character escaped and each run of whitespace one space, as a browser shows the text, and
/// each `$` of text escaped, so that the Kit (which turns math on) finds no math. `$` stays as
/// written in code, HTML blocks and autolinks, which take no escapes. HTML blocks stay too:
/// written out on one line, a block could join the paragraph after it, so [`LiteralHtml`]
/// draws them; but HTML blocks side by side become one, as the browser runs their text
/// together.
///
/// The document is parsed as react-markdown parses it (GFM, no math), so what counts as HTML
/// or text is what it counts.
fn literal_source(source: &str) -> String {
    if !source.contains(['<', '$']) {
        return source.to_owned();
    }
    let root = markdown::to_mdast(source, &markdown::ParseOptions::gfm()).expect("markdown-rs fails only on MDX, which is off");
    let mut literal = Rewrite { source, out: String::with_capacity(source.len() + source.len() / 8), copied: 0 };
    literal.node(&root, Place::Flow);
    literal.copy_to(source.len());
    literal.out
}

/// Where a node sits: HTML among blocks is an HTML block; anywhere else it is inline.
#[derive(Clone, Copy)]
enum Place {
    Flow,
    Phrasing,
}

/// The source being copied out, with the replacements [`literal_source`] makes.
struct Rewrite<'a> {
    source: &'a str,
    out: String,
    /// The end of what has been copied out (or replaced) so far.
    copied: usize,
}

impl Rewrite<'_> {
    fn node(&mut self, node: &Node, place: Place) {
        let Some(position) = node.position() else { return };
        let range = position.start.offset..position.end.offset;
        match (node, place) {
            (Node::Html(html), Place::Phrasing) => {
                self.copy_to(range.start);
                escape_html(&html.value, &mut self.out);
                self.copied = range.end;
            }
            // Backslash escapes count in an image's text, URL and title alike.
            (Node::Text(_) | Node::Image(_), _) => {
                let text = &self.source[range.clone()];
                if text.contains('$') {
                    self.copy_to(range.start);
                    escape_dollars(text, &mut self.out);
                    self.copied = range.end;
                }
            }
            // An autolink (`<https://…>`, or a bare URL) takes its text as written.
            (Node::Link(_), _) if !self.source[range].starts_with('[') => {}
            // `[text]` and `[text][]` find their definition by the text as written.
            (Node::LinkReference(link), _) if link.reference_kind != ReferenceKind::Full => {}
            _ => {
                let inner = match node {
                    Node::Root(_) | Node::Blockquote(_) | Node::List(_) | Node::ListItem(_) | Node::FootnoteDefinition(_) => {
                        Place::Flow
                    }
                    _ => Place::Phrasing,
                };
                let children = node.children().map_or(&[][..], Vec::as_slice);
                let mut index = 0;
                while index < children.len() {
                    let run = match inner {
                        Place::Flow => children[index..].iter().take_while(|child| matches!(child, Node::Html(_))).count(),
                        Place::Phrasing => 0,
                    };
                    if run > 1 {
                        self.html_blocks(&children[index..index + run]);
                        index += run;
                    } else {
                        self.node(&children[index], inner);
                        index += 1;
                    }
                }
            }
        }
    }

    /// HTML blocks side by side, which react-markdown writes out as text nodes that the
    /// browser runs together into one line of text: written as one HTML block on one line,
    /// [`JOINED_HTML`] then each block's text with its whitespace collapsed, joined by a
    /// space. A comment starts it, so it ends with its line whatever the first block was,
    /// and what follows parses as it did after the last one.
    fn html_blocks(&mut self, blocks: &[Node]) {
        let (Some(first), Some(last)) = (blocks.first().and_then(Node::position), blocks.last().and_then(Node::position))
        else {
            return;
        };
        let (start, end) = (first.start.offset, last.end.offset);
        self.copy_to(start);
        self.out.push_str(JOINED_HTML);
        let texts = blocks.iter().filter_map(|block| match block {
            Node::Html(html) => Some(html.value.as_str()),
            _ => None,
        });
        self.out.push_str(&texts.flat_map(str::split_ascii_whitespace).collect::<Vec<_>>().join(" "));
        self.copied = end;
    }

    fn copy_to(&mut self, end: usize) {
        self.out.push_str(&self.source[self.copied..end]);
        self.copied = end;
    }
}

/// Inline HTML as the text it shows: each ASCII punctuation character escaped, each run of
/// whitespace (line breaks too) one space.
fn escape_html(html: &str, out: &mut String) {
    for (index, word) in html.split_ascii_whitespace().enumerate() {
        if index > 0 {
            out.push(' ');
        }
        for c in word.chars() {
            if c.is_ascii_punctuation() {
                out.push('\\');
            }
            out.push(c);
        }
    }
}

/// `text` (source) with each `$` escaped; those already escaped stay as they are.
fn escape_dollars(text: &str, out: &mut String) {
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                if let Some(escaped) = chars.next_if(char::is_ascii_punctuation) {
                    out.push(escaped);
                }
            }
            '$' => out.push_str("\\$"),
            c => out.push(c),
        }
    }
}

const LITERAL_HTML: &str = "literal-html";

/// What starts the one HTML block [`literal_source`] writes for HTML blocks side by side.
const JOINED_HTML: &str = "<!--treeterm:joined-html-->";

/// An HTML block, which react-markdown writes out as text in its place: each run of
/// whitespace one space (none at the ends), as a browser lays it out, in the text style
/// around it, with no margins of its own. HTML blocks side by side are one line of text
/// ([`Rewrite::html_blocks`]).
struct LiteralHtml;

impl MarkdownPlugin for LiteralHtml {
    fn is_block(&self) -> bool {
        true
    }

    fn name(&self) -> &str {
        LITERAL_HTML
    }

    fn parse(&self, node: &Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        literal_html(node)
    }

    fn render(&self, node: &MarkdownNode, _: &mut Window, _: &mut App) -> impl IntoElement {
        div().debug_selector(|| LITERAL_HTML.into()).child(SharedString::from(node.as_text().to_owned()))
    }
}

fn literal_html(node: &Node) -> Option<MarkdownNode> {
    let Node::Html(html) = node else { return None };
    let value = html.value.strip_prefix(JOINED_HTML).unwrap_or(&html.value);
    let text = value.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    Some(MarkdownNode::new(LITERAL_HTML, ()).text(text))
}

const BROKEN_IMAGE: &str = "broken-image";

/// An image, which never loads in Electron's preview: Chromium draws its broken-image icon
/// and the `alt` text after it, on one line, or nothing when the `alt` is empty.
struct BrokenImage;

/// What [`BrokenImage`] draws.
#[derive(Clone, Debug, PartialEq)]
enum Broken {
    Alt(SharedString),
    NoAlt,
}

impl MarkdownPlugin for BrokenImage {
    fn name(&self) -> &str {
        BROKEN_IMAGE
    }

    fn parse(&self, node: &Node, _: &MarkdownParseContext<'_>) -> Option<MarkdownNode> {
        broken_image(node)
    }

    fn render_inline(
        &self,
        node: &MarkdownNode,
        context: &InlineRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<InlineElement> {
        // `None` (a node not of this plugin) draws the node's text.
        Some(match node.data::<Broken>()? {
            Broken::Alt(alt) => broken_image_element(alt, context, window, cx),
            // The Kit draws the source in place of a box with no width.
            Broken::NoAlt => InlineElement::new(div().w(px(1.)).h(px(1.))),
        })
    }
}

/// The `alt` is laid out as a browser lays out text: each run of whitespace one space (none at
/// the ends). A label over several lines keeps its line endings (`\r\n` too) in the `alt`,
/// which must not reach the one-line box the text is drawn in.
fn broken_image(node: &Node) -> Option<MarkdownNode> {
    let alt = match node {
        Node::Image(image) => &image.alt,
        Node::ImageReference(image) => &image.alt,
        _ => return None,
    };
    let text = alt.split_ascii_whitespace().collect::<Vec<_>>().join(" ");
    let broken = if alt.is_empty() { Broken::NoAlt } else { Broken::Alt(text.clone().into()) };
    Some(MarkdownNode::new(BROKEN_IMAGE, broken).text(text))
}

/// The 16px icon (`ImageOff`, as for the Files viewer's broken image) at the top of the line,
/// as Chromium draws its icon, then the `alt` text in the text style around it, on its
/// baseline.
fn broken_image_element(alt: &SharedString, context: &InlineRenderContext, window: &mut Window, cx: &App) -> InlineElement {
    let line_height = context.line_height();
    let run = context.text_style().to_run(alt.len());
    let line = window.text_system().shape_line(alt.clone(), context.font_size(), &[run], None);
    let baseline = (line_height - line.ascent - line.descent) / 2. + line.ascent;
    let icon = crate::ui::icon(IconName::ImageOff, 16., colors(cx).text_secondary).flex_shrink_0();
    let element = div().debug_selector(|| BROKEN_IMAGE.into()).flex().items_start().h(line_height).child(icon).child(alt.clone());
    InlineElement::new(element).with_baseline(baseline)
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Context, Render, TestAppContext, VisualTestContext};
    use markdown::ParseOptions;

    use super::*;

    /// How GPUI Kit parses (`MarkdownExtensions::parse_options`): GFM with math.
    fn kit_options() -> ParseOptions {
        let mut options = ParseOptions::gfm();
        options.constructs.math_text = true;
        options.constructs.math_flow = true;
        options
    }

    /// The blocks GPUI Kit parses from the preview's source for `markdown`.
    fn kit_blocks(markdown: &str) -> Vec<Node> {
        match markdown::to_mdast(&literal_source(markdown), &kit_options()) {
            Ok(Node::Root(root)) => root.children,
            other => panic!("a root, not {other:?}"),
        }
    }

    fn has_html_or_math(node: &Node) -> bool {
        matches!(node, Node::Html(_) | Node::InlineMath(_) | Node::Math(_))
            || node.children().into_iter().flatten().any(has_html_or_math)
    }

    /// Inline HTML shows as typed (react-markdown turns raw HTML into text), formatting tags
    /// the Kit would apply as much as any other.
    #[test]
    fn inline_html_is_its_text() {
        let markdown = r#"a <b>c</b> d <kbd>K</kbd> <span title="*x*">s</span>"#;
        assert_eq!(literal_source(markdown), r#"a \<b\>c\<\/b\> d \<kbd\>K\<\/kbd\> \<span title\=\"\*x\*\"\>s\<\/span\>"#);
        let blocks = kit_blocks(markdown);
        assert_eq!(blocks.len(), 1);
        assert!(!has_html_or_math(&blocks[0]));
        assert_eq!(blocks[0].to_string(), markdown);
    }

    /// A tag over several lines is one line of text, its whitespace collapsed as a browser
    /// shows it; the lines' blockquote markers are not part of it.
    #[test]
    fn inline_html_over_lines_is_one_line() {
        let markdown = "> a <span\n>    title=\"x\">b</span> c";
        assert_eq!(literal_source(markdown), r#"> a \<span title\=\"x\"\>b\<\/span\> c"#);
        assert_eq!(kit_blocks(markdown)[0].to_string(), r#"a <span title="x">b</span> c"#);
    }

    /// HTML in headings, table cells and link text is inline HTML too.
    #[test]
    fn inline_html_anywhere_in_text_is_its_text() {
        let markdown = "# T <sup>1</sup>\n\n| a <br> b |\n|---|\n\n[x <i>y</i>](https://e.com)";
        let blocks = kit_blocks(markdown);
        assert!(!blocks.iter().any(has_html_or_math));
        let texts: Vec<String> = blocks.iter().map(Node::to_string).collect();
        assert_eq!(texts, ["T <sup>1</sup>", "a <br> b", "x <i>y</i>"]);
    }

    /// An HTML block stays one: [`LiteralHtml`] shows its text, whitespace collapsed.
    #[test]
    fn an_html_block_is_its_text_with_whitespace_collapsed() {
        let markdown = "<div align=\"center\">\n  <b>x</b>   y\n</div>\n\n- <p>\n  z\n  </p>";
        assert_eq!(literal_source(markdown), markdown);
        let blocks = kit_blocks(markdown);
        let block = literal_html(&blocks[0]).expect("an HTML block");
        assert_eq!(block.name(), LITERAL_HTML);
        assert_eq!(block.as_text(), r#"<div align="center"> <b>x</b> y </div>"#);
        let item = &blocks[1].children().expect("a list")[0];
        let in_item = literal_html(&item.children().expect("a list item")[0]).expect("an HTML block");
        assert_eq!(in_item.as_text(), "<p> z </p>");
        assert!(literal_html(&kit_blocks("text")[0]).is_none());
    }

    /// The text of each HTML block, as [`LiteralHtml`] shows it, among the blocks GPUI Kit
    /// parses; the text of any other block.
    fn block_texts(blocks: &[Node]) -> Vec<String> {
        blocks.iter().map(|block| literal_html(block).map_or_else(|| block.to_string(), |html| html.as_text().to_owned())).collect()
    }

    /// HTML blocks side by side, on lines of their own or between blank lines, are one line of
    /// text joined by spaces, as Chromium runs react-markdown's text nodes together. What
    /// follows them is parsed as before: a paragraph straight after a comment stays one.
    #[test]
    fn html_blocks_side_by_side_are_one_line_of_text() {
        let markdown = "<p align=\"center\">\n  <img src=\"logo.png\">\n</p>\n\n<h1 align=\"center\">Tree</h1>\n\n\
                        <!-- note -->\n<!-- more -->\nText *after*.\n\n<div>x</div>";
        let blocks = kit_blocks(markdown);
        assert_eq!(block_texts(&blocks), [
            r#"<p align="center"> <img src="logo.png"> </p> <h1 align="center">Tree</h1> <!-- note --> <!-- more -->"#,
            "Text after.",
            "<div>x</div>",
        ]);
        // A run that starts with a tag alone on its line (which cannot interrupt a paragraph)
        // and ends the document.
        let blocks = kit_blocks("<a href=\"x\">\n\n<b>\nbold\n</b>");
        assert_eq!(block_texts(&blocks), ["<a href=\"x\"> <b> bold </b>"]);
    }

    /// In a quote or a list item, the run is joined within it; the item goes on after it.
    #[test]
    fn html_blocks_side_by_side_in_a_quote_or_list_item_are_one_line() {
        let blocks = kit_blocks("> <p>a</p>\n>\n> <p>\n> b\n> </p>\n\n- <p>c</p>\n\n  <p>d</p>\n\n  after\n- <p>e</p>");
        assert_eq!(block_texts(blocks[0].children().expect("a quote")), ["<p>a</p> <p> b </p>"]);
        let items = blocks[1].children().expect("a list");
        assert_eq!(block_texts(items[0].children().expect("an item")), ["<p>c</p> <p>d</p>", "after"]);
        assert_eq!(block_texts(items[1].children().expect("an item")), ["<p>e</p>"]);
    }

    /// react-markdown has no math: a `$$` block is a paragraph, Markdown inside it included.
    #[test]
    fn a_math_block_is_a_paragraph() {
        let markdown = "$$\nx^2 + \\{y\\} *z*\n$$";
        assert_eq!(literal_source(markdown), "\\$\\$\nx^2 + \\{y\\} *z*\n\\$\\$");
        let blocks = kit_blocks(markdown);
        assert_eq!(blocks.len(), 1);
        let Node::Paragraph(paragraph) = &blocks[0] else { panic!("a paragraph, not {:?}", blocks[0]) };
        assert!(paragraph.children.iter().any(|child| matches!(child, Node::Emphasis(_))));
        assert!(!has_html_or_math(&blocks[0]));
        assert_eq!(blocks[0].to_string(), "$$\nx^2 + {y} z\n$$");
    }

    /// Dollars in text are text; ones already escaped stay escaped (`\\$` is an escaped
    /// backslash, then a dollar).
    #[test]
    fn dollars_are_text() {
        let markdown = r"Inline $a$, $5 and $10, a\$b and \\$c.";
        assert_eq!(literal_source(markdown), r"Inline \$a\$, \$5 and \$10, a\$b and \\\$c.");
        let blocks = kit_blocks(markdown);
        assert!(!has_html_or_math(&blocks[0]));
        assert_eq!(blocks[0].to_string(), r"Inline $a$, $5 and $10, a$b and \$c.");
    }

    /// Code, code blocks, autolinks and `[text]` references take no escapes: what is in them
    /// stays as written.
    #[test]
    fn code_autolinks_and_reference_text_stay_as_written() {
        let markdown =
            "`$x$ <b>`\n\n```\n$$\n<b>\n```\n\n<https://e.com/$q$> https://e.com/$r\n\n[a $x$] [b $y$][]\n\n[a $x$]: u\n[b $y$]: v";
        assert_eq!(literal_source(markdown), markdown);
        assert_eq!(literal_source("[a $x$][r]\n\n[r]: u"), "[a \\$x\\$][r]\n\n[r]: u");
    }

    /// An image's text keeps its dollars; its URL and title are unchanged by the escapes.
    #[test]
    fn an_images_dollars_are_text() {
        let blocks = kit_blocks(r#"![a $x$](u/$y$ "t $z$")"#);
        let Node::Image(image) = &blocks[0].children().expect("a paragraph")[0] else { panic!("an image") };
        assert_eq!((image.alt.as_str(), image.url.as_str(), image.title.as_deref()), ("a $x$", "u/$y$", Some("t $z$")));
    }

    #[test]
    fn markdown_without_tags_or_dollars_is_as_written() {
        assert_eq!(literal_source("# Hi *there*\n\n- [x] done"), "# Hi *there*\n\n- [x] done");
    }

    /// No image loads (Electron's CSP; react-markdown empties `data:` URLs): each is a broken
    /// one with its `alt` text, which is nothing when the `alt` is empty.
    #[test]
    fn every_image_is_broken_with_its_alt() {
        let markdown = "![logo](https://e.com/x.png) ![rel](img/x.png) ![d](data:image/png;base64,AAAA) \
                        ![](https://e.com/y.png) ![ref][r]\n\n[r]: x.png";
        let blocks = kit_blocks(markdown);
        let broken: Vec<Broken> = blocks[0]
            .children()
            .expect("a paragraph")
            .iter()
            .filter_map(broken_image)
            .map(|node| {
                assert_eq!(node.name(), BROKEN_IMAGE);
                node.data::<Broken>().expect("what to draw").clone()
            })
            .collect();
        let alt = |text: &str| Broken::Alt(text.to_owned().into());
        assert_eq!(broken, [alt("logo"), alt("rel"), alt("d"), Broken::NoAlt, alt("ref")]);
        assert!(broken_image(&kit_blocks("text")[0]).is_none());
    }

    /// An `alt` over several lines (`\n` or `\r\n`), or with runs of spaces and tabs, is one
    /// line with each run of whitespace one space, as a browser lays out the text; one of only
    /// whitespace still draws the icon.
    #[test]
    fn an_images_alt_is_one_line() {
        let broken = |markdown: &str| {
            let blocks = kit_blocks(markdown);
            let image = &blocks[0].children().expect("a paragraph")[0];
            let node = broken_image(image).expect("an image");
            (node.data::<Broken>().expect("what to draw").clone(), node.as_text().to_owned())
        };
        let alt = |text: &str| (Broken::Alt(text.to_owned().into()), text.to_owned());
        assert_eq!(broken("![two\nlines](u)"), alt("two lines"));
        assert_eq!(broken("![two\r\nlines](u)"), alt("two lines"));
        assert_eq!(broken("![two\r\n  lines\rand\t three](u)"), alt("two lines and three"));
        assert_eq!(broken("![two\r\nlines][r]\n\n[r]: u"), alt("two lines"));
        assert_eq!(broken("![two\nlines]\n\n[two lines]: u"), alt("two lines"));
        assert_eq!(broken("![ ](u)"), alt(""));
        assert_eq!(broken("![](u)"), (Broken::NoAlt, String::new()));
    }

    struct Host {
        text: String,
    }

    impl Render for Host {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let scroll = ScrollHandle::new();
            div().w(px(600.)).h(px(600.)).child(preview("md-test".into(), self.text.clone(), &scroll, None, window, cx))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// The preview draws HTML blocks and images with the plugins, and follows a new text.
    #[gpui_kit::test]
    fn the_preview_draws_html_blocks_and_broken_images(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
        });
        let (host, cx) = cx.add_window_view(|_, _| Host { text: "plain".into() });
        draw(cx);
        assert!(cx.debug_bounds(LITERAL_HTML).is_none());
        assert!(cx.debug_bounds(BROKEN_IMAGE).is_none());
        host.update_in(cx, |host, _, cx| {
            // An `alt` over lines must not reach text shaping with its line ending.
            host.text = "<div>block</div>\n\nAn ![an\r\nimage](https://e.com/x.png) and ![](y.png) here.".into();
            cx.notify();
        });
        draw(cx);
        draw(cx);
        assert!(cx.debug_bounds(LITERAL_HTML).is_some());
        assert!(cx.debug_bounds(BROKEN_IMAGE).is_some());
    }
}
