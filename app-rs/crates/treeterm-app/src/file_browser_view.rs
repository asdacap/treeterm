//! The Files tab. Port of the view half of `FilesystemBrowser.tsx`, `FileTree.tsx`,
//! `FileViewer.tsx` and `SearchPanel.tsx`, sized and coloured as `index.css` draws them: a
//! collapsible, resizable directory tree with a file name search and favourites, a viewer
//! (a read-only, syntax-highlighted editor through GPUI Kit, markdown, images, PDFs and 3D
//! models) under a header with Download and "Open in Tab", and the search panel along the
//! bottom.
//!
//! In the viewer, Cmd/Ctrl+F opens the search panel with the selection, Cmd/Ctrl+click or
//! F12 goes to the definition of the word under the cursor, and the context menu offers
//! both.
//!
//! A click on a line number starts a review comment on that line. Commented lines are
//! tinted with a bar in the glyph margin (Monaco's `.comment-line` / `.comment-glyph`);
//! GPUI Kit's editor cannot open space between lines for Monaco's view zones, so the file's
//! comments and the comment box sit in a panel under the editor, each headed by its line.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::input::{
    Copy as CopyText, Editor, EditorState, GutterDecoration, Input, InputEvent, InputState, RangeDecoration,
    RangeDecorationCollection, RangeDecorationStyle, Search, SelectAll, TextareaState,
};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, Context, DragMoveEvent, Entity, Focusable as _, FontWeight, Hsla, KeyBinding, MouseButton,
    MouseUpEvent, Pixels, Point, SharedString, Subscription, Window, actions, div, px, relative, rgb, rgba,
};
use treeterm_core::review::{Freshness, MetadataError, ReviewComment};
use treeterm_core::files::{detect_monaco_language, favourite_file_name, max_read_bytes_for};
use treeterm_model::file_browser::{
    DefinitionLookup, Favourites, FileBrowser, FileView, LineDraft, Listing, NameResults, SearchPanel, Star,
};
use treeterm_model::review::Notice;

use crate::comment_ui::{CommentActions, Placement, comment_display, comment_input};

use crate::theme::colors;
use crate::ui;

mod markdown;
mod vs_dark;

actions!(file_browser, [GoToDefinition, SearchWord]);

/// The viewer and its editor.
const VIEWER_CONTEXT: &str = "FileViewer";

/// The viewer's keys, and Monaco's `vs-dark` colours for the code editors (the viewer and
/// the Editor tab), installed over the theme at start-up.
pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f12", GoToDefinition, Some(VIEWER_CONTEXT))]);
    gpui_kit::component::Theme::global_mut(cx).highlight_theme = vs_dark::theme();
}

/// The strip that lines GPUI Kit's gutter up with Monaco's, for a file of `lines` lines.
///
/// Monaco's gutter is the glyph margin (21px, when on), line numbers at least 5 digits wide
/// and 26px of folding and decorations; GPUI Kit's is a 6px inset, numbers at least 3 digits
/// wide, 6px and an 18px fold column. An empty gutter-decoration strip of the difference
/// (it brings its own 4px gap) before the numbers puts the text where Monaco's starts: 89px
/// in the Files tab's viewer, 68px in the Editor tab.
fn glyph_strip(lines: usize, glyph_margin: bool) -> f32 {
    /// Menlo's digit at 14px.
    const DIGIT: f32 = 8.43;
    let digits = lines.max(1).ilog10() as usize + 1;
    let monaco = if glyph_margin { 21. } else { 0. } + digits.max(5) as f32 * DIGIT + 26.;
    let gpui = 6. + 4. + digits.max(3) as f32 * DIGIT + 6. + 18.;
    (monaco - gpui).max(0.)
}

/// Where the code starts: GPUI Kit's gutter (34px plus the numbers) and the strip.
fn gutter_width(lines: usize, glyph_margin: bool) -> f32 {
    34. + (lines.max(1).ilog10() as usize + 1).max(3) as f32 * 8.43 + glyph_strip(lines, glyph_margin)
}

/// GPUI Kit's fold column and the 6px before it, at the gutter's right end.
const FOLD_COLUMN: f32 = 24.;

/// The 1-based line `offset` is on.
fn line_at(text: &str, offset: usize) -> u32 {
    let before = text.get(..offset.min(text.len())).unwrap_or(text);
    u32::try_from(before.matches('\n').count() + 1).unwrap_or(u32::MAX)
}

/// The bytes of 1-based `line`, its newline included (so an empty line has a width);
/// `None` past the end, and for the empty line after a final newline, which has no bytes
/// to mark (an empty range would mark the boundary above it).
fn line_bytes(text: &str, line: u32) -> Option<std::ops::Range<usize>> {
    let mut start = 0;
    for (index, row) in text.split('\n').enumerate() {
        let end = (start + row.len() + 1).min(text.len());
        if u32::try_from(index + 1).ok()? == line {
            return (start < end).then_some(start..end);
        }
        start += row.len() + 1;
    }
    None
}

/// `.comment-line` and `.comment-line-outdated`, and the glyph margin's mark (Monaco's
/// 💬 / ⚠️ `.comment-glyph`; GPUI Kit fills the whole strip, so it is toned down).
fn comment_line_colors(freshness: Freshness) -> (Hsla, Hsla) {
    match freshness {
        Freshness::Current => (rgba(0x0078d41a).into(), rgba(0x0078d499).into()),
        Freshness::Outdated => (rgba(0xf0a0001a).into(), rgba(0xf0a00099).into()),
    }
}

/// The comment being written, and the box it is typed in.
struct DraftInput {
    draft: LineDraft,
    input: Entity<TextareaState>,
    text: String,
    _events: Subscription,
}

/// Monaco's bracket pair colours (`editorBracketHighlight.foreground1..3`, cycling by depth,
/// and `unexpectedBracket`) for `text`, a file of Monaco `language`.
pub(crate) fn bracket_colors(text: &str, language: &str) -> Vec<gpui_kit::component::input::TextDecoration> {
    use treeterm_model::bracket_pairs::{Depth, bracket_pairs};
    const LEVELS: [u32; 3] = [0xffd700, 0xda70d6, 0x179fff];
    bracket_pairs(text, language)
        .into_iter()
        .map(|bracket| {
            let color: Hsla = match bracket.depth {
                Depth::Level(level) => rgb(LEVELS[level % LEVELS.len()]).into(),
                Depth::Unexpected => rgba(0xff1212cc).into(),
            };
            gpui_kit::component::input::TextDecoration::new(
                bracket.range,
                gpui_kit::HighlightStyle { color: Some(color), ..Default::default() },
            )
        })
        .collect()
}

/// Monaco's `revealLineInCenter` with the range selected: the line it starts on is scrolled
/// to the middle of the editor (as near as the ends of the text allow).
pub(crate) fn reveal_in_center(editor: &Entity<EditorState>, range: std::ops::Range<usize>, cx: &mut App) {
    editor.update(cx, |editor, cx| {
        editor.set_selected_range(range.clone(), cx);
        let Some(line_height) = editor.line_height() else {
            return;
        };
        let text = editor.text().to_string();
        let line = text.get(..range.start).map_or(0, |before| before.matches('\n').count());
        // Wrapped lines above take more than one row.
        let row = editor.display_map().buffer_line_to_display_row(line);
        let viewport = editor.input_bounds().size.height;
        let top = (line_height * row as f32 - (viewport - line_height) / 2.).max(px(0.));
        editor.set_scroll_offset(gpui_kit::point(editor.scroll_offset().x, -top), cx);
    });
}

/// A code editor laid out as Monaco's: its gutter width (keep the returned collection with
/// the editor) and its always-visible 14px scrollbar (`scrollbarSlider.*` of vs-dark, beside
/// the overview ruler's border).
///
/// `glyph_margin`: the Files tab's viewer has Monaco's glyph margin, the Editor tab not.
pub(crate) fn monaco_gutter(
    editor: &Entity<EditorState>,
    glyph_margin: bool,
    cx: &mut App,
) -> gpui_kit::component::input::GutterDecorationCollection {
    editor.update(cx, |editor, cx| {
        editor.set_scrollbar_renderer(Some(Rc::new(|context, _, _| {
            use gpui_kit::base::{Scrollbar, ScrollbarMode};
            Scrollbar::new(&context)
                .axis(context.axis())
                .mode(ScrollbarMode::Always)
                .styles(|styles| {
                    styles
                        .track(|track| track.width(px(14.)).bg(gpui_kit::transparent_black()).border_color(rgba(0x7f7f7f4d).into()))
                        .thumb(|thumb| thumb.width(px(14.)).inset(px(0.)).radius(px(0.)).bg(rgba(0x79797966)))
                        .thumb_hover(|thumb| thumb.bg(rgba(0x646464b3)))
                        .thumb_active(|thumb| thumb.bg(rgba(0xbfbfbf66)))
                })
                .into_any_element()
        })));
        let lines = gpui_kit::component::input::RopeExt::lines_len(editor.text());
        editor.set_gutter_decorations_width(px(glyph_strip(lines, glyph_margin)), cx);
        editor.create_gutter_decorations_collection(Vec::new(), cx)
    })
}

/// The tree's width (`treeWidth`): 250px by default, dragged between 150 and 500.
const TREE_WIDTH: f32 = 250.;
const TREE_MIN_WIDTH: f32 = 150.;
const TREE_MAX_WIDTH: f32 = 500.;

/// `#e3b341`: favourite stars and the favourites title.
const FAVOURITE: u32 = 0xe3b341;

/// Menlo, as Monaco and `.search-panel-hit` set it.
const MONO: &str = "Menlo";

/// What the view lays out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// The Files tab: the tree beside the viewer.
    Browser,
    /// The viewer alone, inside another tab (Review's favourites: `FileViewer` without
    /// `FilesystemBrowser`).
    Viewer,
}

/// Dragging the divider between the tree and the viewer.
#[derive(Clone)]
struct TreeResize;

impl Render for TreeResize {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

pub struct FileBrowserView {
    model: Entity<FileBrowser>,
    layout: Layout,
    /// The PDF or 3D model shown, and the webview showing it.
    page: Option<(String, Entity<crate::browser_view::AssetPageView>)>,
    /// The markdown file showing its preview (`viewMode`); a file opens on its source.
    markdown_preview: Option<String>,
    preview_scroll: gpui_kit::ScrollHandle,
    tree_scroll: gpui_kit::ScrollHandle,
    workspace: Entity<treeterm_model::workspace_store::WorkspaceStore>,
    /// The editor showing the current file, and which file it shows.
    editor: Option<(String, Entity<EditorState>)>,
    /// The editor's glyph margin ([`monaco_gutter`]).
    gutter: Option<gpui_kit::component::input::GutterDecorationCollection>,
    /// The editor's bracket pair colours ([`bracket_colors`]).
    brackets: Option<gpui_kit::component::input::TextDecorationCollection>,
    /// The model's last [`treeterm_model::file_browser::Reveal`] applied to the editor.
    revealed: u64,
    /// The search panel's query box, made when the panel first opens (it needs the window).
    search_input: Option<(Entity<InputState>, Subscription)>,
    /// The search panel's results, and the rows the list was last told about.
    search_results: crate::search_panel::ResultsList,
    /// The tree's "Search files..." box.
    name_input: Option<(Entity<InputState>, Subscription)>,
    tree_width: f32,
    tree_collapsed: bool,
    /// The tree row under the pointer, whose star shows.
    hovered_row: Option<String>,
    /// The divider is being dragged (it stays accent-coloured).
    resizing: bool,
    /// The editor's commented lines (`.comment-line`).
    comment_lines: Option<RangeDecorationCollection>,
    /// The file and lines the gutter and `comment_lines` mark now.
    marked: (String, Vec<(u32, Freshness)>),
    /// Where the editor was laid out, to tell a click on a line number.
    editor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    draft_input: Option<DraftInput>,
    comments_scroll: gpui_kit::ScrollHandle,
    _observe: Subscription,
    /// Review comments live in the workspace's metadata.
    _workspace: Subscription,
}

/// GPUI Kit language name for a path, by extension or file name.
pub fn language_for(path: &str) -> &'static str {
    let name = path.rsplit('/').next().unwrap_or(path);
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match (name, ext.as_str()) {
        ("Makefile" | "makefile" | "GNUmakefile", _) => "make",
        ("CMakeLists.txt", _) => "cmake",
        ("Dockerfile", _) => "bash",
        (_, "rs") => "rust",
        (_, "ts" | "mts" | "cts") => "typescript",
        (_, "tsx") => "tsx",
        (_, "js" | "mjs" | "cjs" | "jsx") => "javascript",
        (_, "json" | "jsonc") => "json",
        (_, "toml") => "toml",
        (_, "yml" | "yaml") => "yaml",
        (_, "md" | "markdown") => "markdown",
        (_, "py") => "python",
        (_, "go") => "go",
        (_, "sh" | "bash" | "zsh") => "bash",
        (_, "css" | "scss") => "css",
        (_, "html" | "htm") => "html",
        (_, "c" | "h") => "c",
        (_, "cc" | "cpp" | "cxx" | "hpp" | "hh") => "cpp",
        (_, "cs") => "csharp",
        (_, "java") => "java",
        (_, "kt" | "kts") => "kotlin",
        (_, "swift") => "swift",
        (_, "rb") => "ruby",
        (_, "php") => "php",
        (_, "sql") => "sql",
        (_, "zig") => "zig",
        (_, "lua") => "lua",
        (_, "proto") => "proto",
        (_, "ex" | "exs") => "elixir",
        (_, "scala") => "scala",
        (_, "graphql" | "gql") => "graphql",
        (_, "svelte") => "svelte",
        (_, "diff" | "patch") => "diff",
        _ => "text",
    }
}

/// `formatMegabytes`: `1.4MB`.
fn format_megabytes(bytes: u64) -> String {
    format!("{:.1}MB", bytes as f64 / (1024. * 1024.))
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(path)
}

/// `highlightMatch`: the name split around the first case-insensitive match of the query
/// (before, match, after), or `None` without one.
fn split_match<'a>(name: &'a str, query: &str) -> Option<(&'a str, &'a str, &'a str)> {
    if query.trim().is_empty() {
        return None;
    }
    let lower_name = name.to_lowercase();
    let lower_query = query.to_lowercase();
    // Lowercasing can change byte lengths; only split where it does not.
    if lower_name.len() != name.len() || lower_query.len() != query.len() {
        return None;
    }
    let start = lower_name.find(&lower_query)?;
    let end = start + query.len();
    (name.is_char_boundary(start) && name.is_char_boundary(end)).then(|| (&name[..start], &name[start..end], &name[end..]))
}

/// The length of the common prefix of two row lists.
impl FileBrowserView {
    pub fn new(
        model: Entity<FileBrowser>,
        workspace: Entity<treeterm_model::workspace_store::WorkspaceStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        let comments = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            model,
            layout: Layout::Browser,
            page: None,
            markdown_preview: None,
            preview_scroll: gpui_kit::ScrollHandle::new(),
            tree_scroll: gpui_kit::ScrollHandle::new(),
            workspace,
            editor: None,
            gutter: None,
            brackets: None,
            revealed: 0,
            search_input: None,
            search_results: crate::search_panel::ResultsList::new(),
            name_input: None,
            tree_width: TREE_WIDTH,
            tree_collapsed: false,
            hovered_row: None,
            resizing: false,
            comment_lines: None,
            marked: (String::new(), Vec::new()),
            editor_bounds: Rc::new(Cell::new(None)),
            draft_input: None,
            comments_scroll: gpui_kit::ScrollHandle::new(),
            _observe: observe,
            _workspace: comments,
        }
    }

    /// The viewer alone, for a file shown inside another tab.
    pub fn embedded(
        model: Entity<FileBrowser>,
        workspace: Entity<treeterm_model::workspace_store::WorkspaceStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        Self { layout: Layout::Viewer, ..Self::new(model, workspace, cx) }
    }

    /// Show the PDF / model webview only while this tab is shown, such a file is the current
    /// one, and no dialog covers it.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        let in_webview = matches!(self.model.read(cx).view(), FileView::Pdf { .. } | FileView::Model { .. });
        if let Some((_, page)) = &self.page {
            page.update(cx, |p, cx| p.set_shown(shown && in_webview, cx));
        }
    }

    /// The webview page for `path`, built when the shown file changes.
    fn page_for(
        &mut self,
        path: String,
        build: impl FnOnce() -> crate::browser_view::AssetPageView,
        cx: &mut Context<Self>,
    ) -> Entity<crate::browser_view::AssetPageView> {
        match &self.page {
            Some((shown, view)) if *shown == path => view.clone(),
            Some(_) | None => {
                let view = cx.new(|_| build());
                self.page = Some((path, view.clone()));
                view
            }
        }
    }

    /// The editor for `path`, created when the shown file changes.
    fn editor_for(&mut self, path: &str, text: &str, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorState> {
        if let Some((shown, editor)) = &self.editor
            && shown == path
        {
            return editor.clone();
        }
        let language = language_for(path);
        let brackets = bracket_colors(text, detect_monaco_language(path));
        let text = text.to_owned();
        // Not searchable: Cmd/Ctrl+F reaches this view's search panel instead.
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language).searchable(false).default_value(text));
        self.gutter = Some(monaco_gutter(&editor, true, cx));
        self.brackets = Some(editor.update(cx, |editor, cx| editor.create_decorations_collection(brackets, cx)));
        self.comment_lines = Some(editor.update(cx, |editor, cx| editor.create_range_decorations_collection(Vec::new(), cx)));
        self.marked = (String::new(), Vec::new());
        self.editor = Some((path.to_owned(), editor.clone()));
        editor
    }

    /// The editor of the file shown now, if it is text.
    fn shown_editor(&self, cx: &App) -> Option<Entity<EditorState>> {
        let FileView::Text { path, .. } = self.model.read(cx).view() else {
            return None;
        };
        match &self.editor {
            Some((shown, editor)) if shown == path => Some(editor.clone()),
            Some(_) | None => None,
        }
    }

    fn search_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some((input, _)) = &self.search_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.set_query(&query, cx));
            }
            InputEvent::PressEnter { shift, .. } => {
                let forward = !*shift;
                view.model.update(cx, |m, cx| m.step_match(forward, cx));
            }
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        });
        self.search_input = Some((input.clone(), events));
        input
    }

    fn name_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some((input, _)) = &self.name_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search files..."));
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.set_name_query(&query, cx));
            }
            InputEvent::PressEnter { .. } => {}
            InputEvent::Focus | InputEvent::Blur => cx.notify(),
        });
        self.name_input = Some((input.clone(), events));
        input
    }

    /// Show the panel's query in its box and put the cursor there.
    fn focus_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = match self.model.read(cx).search() {
            SearchPanel::Open(search) => search.query.clone(),
            SearchPanel::Closed => return,
        };
        let input = self.search_input(window, cx);
        input.update(cx, |input, cx| {
            if input.value().as_ref() != query {
                input.set_value(query, window, cx);
            }
            input.focus(window, cx);
            input.select_all(window, cx);
        });
    }

    /// Cmd/Ctrl+F: open the panel with the editor's selection.
    fn on_search(&mut self, _: &Search, window: &mut Window, cx: &mut Context<Self>) {
        let selection = match self.shown_editor(cx) {
            Some(editor) if editor.read(cx).focus_handle(cx).is_focused(window) => editor.read(cx).selected_value().to_string(),
            Some(_) | None => String::new(),
        };
        self.model.update(cx, |m, cx| m.open_search(&selection, None, cx));
        self.focus_search(window, cx);
    }

    /// Context menu "Search": the selection, or the word under the cursor.
    fn on_search_word(&mut self, _: &SearchWord, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.shown_editor(cx) else {
            return;
        };
        let (selection, cursor) = {
            let editor = editor.read(cx);
            (editor.selected_value().to_string(), editor.cursor())
        };
        self.model.update(cx, |m, cx| m.open_search(&selection, Some(cursor), cx));
        self.focus_search(window, cx);
    }

    fn on_go_to_definition(&mut self, _: &GoToDefinition, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.shown_editor(cx) else {
            return;
        };
        let offset = editor.read(cx).cursor();
        self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
    }

    /// Cmd/Ctrl+click goes to the definition, a click on a line number starts a comment
    /// there (`GUTTER_LINE_NUMBERS`): the press already put the cursor on the clicked word, or
    /// at the start of the clicked line.
    fn on_editor_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.shown_editor(cx) else {
            return;
        };
        let (selected, offset, lines) = {
            let editor = editor.read(cx);
            (editor.selected_range(), editor.cursor(), gpui_kit::component::input::RopeExt::lines_len(editor.text()))
        };
        if !selected.is_empty() {
            return;
        }
        if event.modifiers.secondary(cx) {
            self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
            return;
        }
        if event.click_count != 1 || !self.on_line_numbers(event.position, lines) {
            return;
        }
        let FileView::Text { text, .. } = self.model.read(cx).view() else {
            return;
        };
        let line = line_at(text, offset);
        self.model.update(cx, |m, cx| m.start_comment(line, cx));
    }

    /// Whether `position` is on the editor's glyph margin or line numbers (left of the fold
    /// column).
    fn on_line_numbers(&self, position: Point<Pixels>, lines: usize) -> bool {
        let Some(bounds) = self.editor_bounds.get() else {
            return false;
        };
        let x = f32::from(position.x - bounds.origin.x);
        bounds.contains(&position) && x < gutter_width(lines, true) - FOLD_COLUMN
    }

    /// Select what the model asked to reveal, once the editor has a layout to scroll.
    fn apply_reveal(&mut self, editor: &Entity<EditorState>, window: &mut Window, cx: &mut Context<Self>) {
        let Some((seq, range)) = self.model.read(cx).reveal_range() else {
            return;
        };
        if seq == self.revealed {
            return;
        }
        if editor.read(cx).line_height().is_none() {
            window.request_animation_frame();
            return;
        }
        self.revealed = seq;
        crate::file_browser_view::reveal_in_center(editor, range, cx);
    }

    // ---- tree --------------------------------------------------------------------------

    /// `.filesystem-browser-tree`: the header over the tree.
    fn tree_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let search = self.name_search_box(window, cx);
        let favourites = self.favourites_section(cx);
        let body = if self.model.read(cx).name_search().is_active() { self.name_results(cx) } else { self.tree_rows(cx) };
        let c = colors(cx);
        let (secondary, primary, hover) = (c.text_secondary, c.text_primary, c.bg_hover);
        let header = h_flex()
            .flex_shrink_0()
            .justify_between()
            .pl(px(10.))
            .pr(px(4.))
            .py(px(4.))
            .border_r_1()
            .border_color(c.border)
            .bg(c.bg_secondary)
            .child(ui::caps_title("Files", cx))
            .child(
                div()
                    .id("hide-file-tree")
                    .flex()
                    .p(px(4.))
                    .rounded(px(4.))
                    .text_color(secondary)
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover).text_color(primary))
                    .child(Icon::new(IconName::PanelLeftClose).size(px(16.)))
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Hide file tree").build(window, cx))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.tree_collapsed = true;
                        cx.notify();
                    })),
            );
        let tree = div().flex_1().min_h_0().border_r_1().border_color(c.border).bg(c.bg_secondary).child(ui::scroll_area(
            "file-tree",
            &self.tree_scroll,
            v_flex().w_full().child(search).children(favourites).children(body),
            cx,
        ));
        v_flex()
            .w(px(self.tree_width))
            .h_full()
            .min_h_0()
            .flex_shrink_0()
            .child(header)
            .child(tree)
            .into_any_element()
    }

    /// `.file-tree-search`: the "Search files..." box and its clear button.
    fn name_search_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input = self.name_input(window, cx);
        let c = colors(cx);
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let has_query = !self.model.read(cx).name_search().query.is_empty();
        let (secondary, primary, hover) = (c.text_secondary, c.text_primary, c.bg_hover);
        div()
            .relative()
            .p(px(8.))
            .border_b_1()
            .border_color(c.border)
            .child(
                div()
                    .w_full()
                    .h(px(30.))
                    .pl(px(10.))
                    .pr(px(28.))
                    .flex()
                    .items_center()
                    .border_1()
                    // `var(--accent-color)` is not defined: the focused border falls back to the text colour.
                    .border_color(if focused { c.text_primary } else { c.border })
                    .rounded(px(4.))
                    .bg(c.bg_primary)
                    .text_size(px(ui::BODY))
                    .text_color(c.text_primary)
                    .child(Styled::h(Input::new(&input).appearance(false).text_size(px(ui::BODY)).px_0().py_0(), px(16.))),
            )
            .when(has_query, |d| {
                d.child(
                    div()
                        .id("clear-name-search")
                        .absolute()
                        .right(px(12.))
                        .top(px(14.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(3.))
                        .text_size(px(12.))
                        .text_color(secondary)
                        .cursor_pointer()
                        .hover(move |d| d.bg(hover).text_color(primary))
                        .child("\u{2715}")
                        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Clear search").build(window, cx))
                        .on_click(cx.listener(|view, _, window, cx| {
                            view.model.update(cx, |m, cx| m.clear_name_search(cx));
                            if let Some((input, _)) = &view.name_input {
                                input.update(cx, |input, cx| input.set_value("", window, cx));
                            }
                        })),
                )
            })
            .into_any_element()
    }

    /// `.file-tree-item`: a 13px row, hover-filled, accent-filled when selected.
    fn row_frame(id: impl Into<gpui_kit::ElementId>, selected: bool, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
        let c = colors(cx);
        let (hover, active) = (c.bg_hover, c.bg_active);
        h_flex()
            .id(id)
            .flex_shrink_0()
            .w_full()
            .py(px(4.))
            .px(px(8.))
            .text_size(px(ui::BODY))
            .whitespace_nowrap()
            .cursor_pointer()
            .when(selected, |d| d.bg(active))
            .when(!selected, |d| d.hover(move |d| d.bg(hover)))
    }

    /// `.file-tree-icon`: the 📁 / 📄 glyph in a 20px column, its 14px line box 18.5px
    /// tall. Chromium draws Apple Color Emoji at 14px about as big as GPUI does at 11px.
    fn entry_icon(is_dir: bool) -> gpui_kit::Div {
        div()
            .w(px(20.))
            .h(px(18.5))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(11.))
            .child(if is_dir { "\u{1F4C1}" } else { "\u{1F4C4}" })
    }

    /// A message row (`Loading...`, an error, `No files found`).
    fn message_row(text: impl Into<SharedString>, indent: f32, error: bool, cx: &App) -> AnyElement {
        let secondary = colors(cx).text_secondary;
        let text: SharedString = text.into();
        h_flex()
            .flex_shrink_0()
            .w_full()
            .py(px(4.))
            .pr(px(8.))
            .pl(px(indent))
            .text_size(px(ui::BODY))
            .whitespace_nowrap()
            .map(|d| if error { d.text_color(rgb(ui::FILE_ERROR)) } else { d.text_color(secondary).italic() })
            .child(text)
            .into_any_element()
    }

    /// `.file-tree-favourite`: the star, shown (taking its space) while the row is hovered
    /// or when starred.
    fn star_button(&self, path: &str, is_dir: bool, cx: &mut Context<Self>) -> Option<AnyElement> {
        let star = self.model.read(cx).star(path, cx);
        let active = star != Star::Off;
        if !active && self.hovered_row.as_deref() != Some(path) {
            return None;
        }
        let title = match star {
            Star::On => "Remove from favourites",
            Star::Covered => "Inherited or included by a favourite folder",
            Star::Off if is_dir => "Add folder to favourites",
            Star::Off => "Add file to favourites",
        };
        let owned = path.to_owned();
        div()
            .id(SharedString::from(format!("star-{path}")))
            .flex_shrink_0()
            .p(px(2.))
            .cursor_pointer()
            .text_color(if active { rgb(FAVOURITE).into() } else { colors(cx).text_secondary })
            .flex()
            .child(ui::star_icon(13., active))
            .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation();
                view.model.update(cx, |m, cx| m.toggle_favourite(&owned, cx));
            }))
            .into_any_element()
            .into()
    }

    /// Track which row the pointer is over: its star shows (`.file-tree-item:hover`).
    fn track_hover(path: String, cx: &Context<Self>) -> impl Fn(&bool, &mut Window, &mut App) + 'static {
        cx.listener(move |view, hovered: &bool, _, cx| {
            let over = view.hovered_row.as_deref() == Some(path.as_str());
            if *hovered && !over {
                view.hovered_row = Some(path.clone());
                cx.notify();
            } else if !*hovered && over {
                view.hovered_row = None;
                cx.notify();
            }
        })
    }

    /// The directory tree (`renderEntry`), with loading and error rows under expanding
    /// directories.
    fn tree_rows(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let secondary = colors(cx).text_secondary;
        let model = self.model.read(cx);
        let selected = model.selected().map(str::to_owned);
        let mut rows = match model.root_listing() {
            Some(Listing::Failed(error)) => return vec![Self::message_row(error.clone(), 8., true, cx)],
            Some(Listing::Loading) | None => return vec![Self::message_row("Loading...", 8., false, cx)],
            Some(Listing::Loaded(_)) => Vec::new(),
        };
        let entries = model.rows();
        let child_states: Vec<Option<Listing>> =
            entries.iter().map(|row| row.expanded.then(|| model.listing(&row.path).cloned()).flatten()).collect();
        for (row, child_state) in entries.into_iter().zip(child_states) {
            let is_selected = selected.as_deref() == Some(row.path.as_str());
            let (path, is_dir) = (row.path.clone(), row.is_dir);
            let star = self.star_button(&row.path, row.is_dir, cx);
            rows.push(
                Self::row_frame(SharedString::from(format!("file-{}", row.path)), is_selected, cx)
                    .on_hover(Self::track_hover(row.path.clone(), cx))
                    .pl(px(row.depth as f32 * 16. + 8.))
                    .child(
                        div()
                            .w(px(16.))
                            .flex_shrink_0()
                            .flex()
                            .justify_center()
                            .text_size(px(10.))
                            .text_color(secondary)
                            .child(match (row.is_dir, row.expanded) {
                                (true, true) => "\u{25BC}",
                                (true, false) => "\u{25B6}",
                                (false, _) => "",
                            }),
                    )
                    .child(Self::entry_icon(row.is_dir))
                    .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
                    .children(star)
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.model.update(cx, |model, cx| {
                            if is_dir {
                                model.toggle_dir(&path, cx);
                            } else {
                                model.open(&path, false, cx);
                            }
                        });
                    }))
                    .into_any_element(),
            );
            // An expanded directory still loading, or failing, says so under it.
            let indent = (row.depth + 1) as f32 * 16. + 8.;
            match child_state {
                Some(Listing::Loading) => rows.push(Self::message_row("Loading...", indent, false, cx)),
                Some(Listing::Failed(error)) => rows.push(Self::message_row(error, indent, true, cx)),
                Some(Listing::Loaded(_)) | None => {}
            }
        }
        rows
    }

    /// The file name search's results (`renderSearchResult`): icon, name with the match
    /// marked, and path, stacked.
    fn name_results(&mut self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let search = self.model.read(cx).name_search().clone();
        let selected = self.model.read(cx).selected().map(str::to_owned);
        let entries = match search.results {
            NameResults::Searching => return vec![Self::message_row("Searching...", 8., false, cx)],
            NameResults::Failed(error) => return vec![Self::message_row(error, 8., true, cx)],
            NameResults::Idle => Vec::new(),
            NameResults::Found(entries) => entries,
        };
        if entries.is_empty() {
            return vec![Self::message_row("No files found", 8., false, cx)];
        }
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        entries
            .into_iter()
            .map(|entry| {
                let star = self.star_button(&entry.path, entry.is_directory, cx);
                let name = match split_match(&entry.name, &search.query) {
                    // `display: flex; gap: 6px` spaces the match from the text around it.
                    Some((before, matched, after)) => h_flex()
                        .gap(px(6.))
                        .when(!before.is_empty(), |d| d.child(before.to_owned()))
                        .child(div().px(px(2.)).rounded(px(2.)).text_color(primary).child(matched.to_owned()))
                        .when(!after.is_empty(), |d| d.child(after.to_owned())),
                    None => h_flex().child(entry.name.clone()),
                };
                let tooltip = SharedString::from(entry.relative_path.clone());
                Self::row_frame(SharedString::from(format!("name-result-{}", entry.path)), selected.as_deref() == Some(entry.path.as_str()), cx)
                    .on_hover(Self::track_hover(entry.path.clone(), cx))
                    .flex_col()
                    .items_start()
                    .py(px(6.))
                    .child(Self::entry_icon(entry.is_directory))
                    .child(name.font_weight(FontWeight::MEDIUM))
                    .child(
                        div()
                            .max_w_full()
                            .ml(px(26.))
                            .text_size(px(ui::SMALL))
                            .text_color(secondary)
                            .truncate()
                            .child(entry.relative_path.clone()),
                    )
                    .children(star)
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
                    .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.open_name_result(&entry, cx))))
                    .into_any_element()
            })
            .collect()
    }

    /// The files the favourites name, above the tree (`FavouriteFilesSection`).
    fn favourites_section(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, primary, border, bg_primary) = (c.text_secondary, c.text_primary, c.border, c.bg_primary);
        let model = self.model.read(cx);
        let selected = model.selected().map(str::to_owned);
        let message = |text: String| {
            div().pt(px(4.)).px(px(8.)).pb(px(8.)).text_size(px(ui::SMALL)).text_color(secondary).child(text).into_any_element()
        };
        let body = match model.favourites().clone() {
            Favourites::Empty => return None,
            Favourites::Loading => vec![message("Loading favourites...".into())],
            Favourites::Failed(error) => vec![
                div()
                    .pt(px(4.))
                    .px(px(8.))
                    .pb(px(8.))
                    .text_size(px(ui::SMALL))
                    .text_color(rgb(ui::FILE_ERROR))
                    .child(error)
                    .child(
                        div()
                            .id("retry-favourites")
                            .mt(px(5.))
                            .px(px(7.))
                            .py(px(2.))
                            .w_auto()
                            .self_start()
                            .border_1()
                            .border_color(border)
                            .rounded(px(3.))
                            .bg(bg_primary)
                            .text_color(primary)
                            .cursor_pointer()
                            .child("Retry")
                            .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.resolve_favourites(cx)))),
                    )
                    .into_any_element(),
            ],
            Favourites::Ready(files) if files.is_empty() => vec![message("No favourite files exist in this workspace".into())],
            Favourites::Ready(files) => files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    let is_selected = selected.as_deref() == Some(file.path.as_str());
                    let name = favourite_file_name(&file).to_owned();
                    let path = file.path.clone();
                    let tooltip = SharedString::from(file.relative_path.clone());
                    Self::row_frame(("favourite", index), is_selected, cx)
                        .child(Self::entry_icon(false))
                        .child(div().flex_1().min_w_0().truncate().text_size(px(12.)).child(name))
                        .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
                        .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.open(&path, false, cx))))
                        .into_any_element()
                })
                .collect(),
        };
        Some(
            v_flex()
                .flex_shrink_0()
                .pb(px(4.))
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .gap(px(5.))
                        .px(px(8.))
                        .py(px(6.))
                        .text_size(px(ui::SMALL))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(FAVOURITE))
                        .child(ui::star_icon(12., true))
                        .child("FAVOURITES"),
                )
                .children(body)
                .into_any_element(),
        )
    }

    // ---- viewer ------------------------------------------------------------------------

    /// Go-to-definition progress, failures and the pick list, over the viewer.
    fn definition_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, primary, border, bg, hover) = (c.text_secondary, c.text_primary, c.border, c.bg_secondary, c.bg_hover);
        let close = |cx: &mut Context<Self>| {
            ui::find_button("close-definition", IconName::X, false, cx)
                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.dismiss_definition(cx))))
        };
        let status = |text: String, color: Hsla, cx: &mut Context<Self>| {
            h_flex()
                .flex_shrink_0()
                .gap(px(4.))
                .px(px(8.))
                .py(px(4.))
                .text_size(px(ui::SMALL))
                .bg(bg)
                .border_b_1()
                .border_color(border)
                .child(div().flex_1().text_color(color).child(text))
                .child(close(cx))
                .into_any_element()
        };
        let error: Hsla = rgb(ui::FILE_ERROR).into();
        match self.model.read(cx).definition().clone() {
            DefinitionLookup::Idle => None,
            DefinitionLookup::Searching { symbol } => Some(status(format!("Finding the definition of {symbol}…"), secondary, cx)),
            DefinitionLookup::NotFound { symbol } => Some(status(format!("No definition found for '{symbol}'"), secondary, cx)),
            DefinitionLookup::Unavailable { language } => {
                Some(status(format!("Go to definition is not available for {language} files"), secondary, cx))
            }
            DefinitionLookup::Failed { symbol, error: message } => Some(status(format!("Could not look up {symbol}: {message}"), error, cx)),
            DefinitionLookup::Choose { symbol, locations } => {
                let mut list = v_flex().id("definitions").max_h(px(240.)).overflow_y_scroll().pb(px(4.));
                for (index, location) in locations.iter().enumerate() {
                    list = list.child(
                        h_flex()
                            .id(("definition", index))
                            .gap(px(8.))
                            .pl(px(16.))
                            .pr(px(8.))
                            .py(px(2.))
                            .font_family(MONO)
                            .text_size(px(ui::SMALL))
                            .text_color(secondary)
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover).text_color(primary))
                            .child(format!("{}:{}", location.file_path, location.line_number))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.choose_definition(index, cx));
                            })),
                    );
                }
                Some(
                    v_flex()
                        .flex_shrink_0()
                        .bg(bg)
                        .border_b_1()
                        .border_color(border)
                        .child(
                            h_flex()
                                .gap(px(4.))
                                .px(px(8.))
                                .py(px(4.))
                                .text_size(px(ui::SMALL))
                                .child(div().flex_1().text_color(secondary).child(format!(
                                    "{} definitions of '{symbol}'",
                                    locations.len()
                                )))
                                .child(close(cx)),
                        )
                        .child(list)
                        .into_any_element(),
                )
            }
        }
    }

    /// `FileDownloadButton`: pick where to save a copy of `path` on this computer; a failure
    /// shows beside a retry.
    fn download_button(&self, path: &str, cx: &mut Context<Self>) -> Vec<AnyElement> {
        use treeterm_model::file_browser::DownloadState;
        let state = self.model.read(cx).download_state().clone();
        let (label, busy) = match &state {
            DownloadState::Idle => ("Download", false),
            DownloadState::Downloading => ("Downloading…", true),
            DownloadState::Failed(_) => ("Retry download", false),
        };
        let (model, owned) = (self.model.clone(), path.to_owned());
        let mut out = Vec::new();
        if let DownloadState::Failed(error) = state {
            let tooltip = SharedString::from(error.clone());
            out.push(
                div()
                    .id("download-error")
                    .max_w(px(240.))
                    .truncate()
                    .text_size(px(ui::SMALL))
                    .text_color(rgb(ui::FILE_ERROR))
                    .child(format!("Download failed: {error}"))
                    .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip.clone()).build(window, cx))
                    .into_any_element(),
            );
        }
        let button = ui::viewer_button("download-file", busy.then_some(0.6), cx)
            .gap(px(3.))
            .child(Icon::new(IconName::Download).size(px(ui::SMALL)))
            .child(label)
            .tooltip(|window, cx| {
                gpui_kit::component::tooltip::Tooltip::new("Save a copy of this file to your computer").build(window, cx)
            });
        let button = if busy {
            button
        } else {
            button.on_click(cx.listener(move |_, _, _, cx| {
                let name = owned.rsplit('/').next().unwrap_or("download").to_owned();
                let dir = dirs::download_dir().or_else(dirs::home_dir).unwrap_or_default();
                let picked = cx.prompt_for_new_path(&dir, Some(&name));
                let (model, path) = (model.clone(), owned.clone());
                cx.spawn(async move |_, cx| {
                    let destination = match picked.await {
                        Ok(Ok(Some(destination))) => destination,
                        Ok(Ok(None)) | Err(_) => return, // cancelled
                        Ok(Err(error)) => return tracing::error!(%error, "save dialog failed"),
                    };
                    model.update(cx, |m, cx| m.download(&path, destination, cx));
                })
                .detach();
            }))
        };
        out.push(button.into_any_element());
        out
    }

    /// `.file-viewer-header`: the file name, then the actions and the language.
    fn viewer_header(&mut self, path: &str, language: SharedString, text: bool, cx: &mut Context<Self>) -> AnyElement {
        let download = self.download_button(path, cx);
        let c = colors(cx);
        let markdown = text && detect_monaco_language(path) == "markdown";
        let preview = self.markdown_preview.as_deref() == Some(path);
        let mut actions = h_flex().flex_shrink_0().gap(px(8.));
        if markdown {
            actions = actions.child(
                ui::viewer_button("markdown-view-mode", None, cx)
                    .child(if preview { "</> Source" } else { "Preview" })
                    .tooltip(move |window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new(if preview { "View source" } else { "View preview" }).build(window, cx)
                    })
                    .on_click(cx.listener({
                        let path = path.to_owned();
                        move |view, _, _, cx| {
                            view.markdown_preview = if preview { None } else { Some(path.clone()) };
                            cx.notify();
                        }
                    })),
            );
        }
        actions = actions.children(download);
        if text {
            let edit_path = path.to_owned();
            actions = actions.child(
                ui::viewer_button("open-in-tab", None, cx)
                    .child("\u{21D7} Open in Tab")
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Open in new tab for editing").build(window, cx))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.workspace.update(cx, |w, cx| w.open_in_editor(&edit_path, cx));
                    })),
            );
        }
        actions = actions.child(ui::language_pill(language, cx));
        h_flex()
            .flex_shrink_0()
            .justify_between()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(ui::BODY))
                    .font_weight(FontWeight::MEDIUM)
                    .child(file_name(path).to_owned()),
            )
            .child(actions)
            .into_any_element()
    }

    /// `.file-viewer`: a placeholder, an error, or the header over the file.
    fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let frame = v_flex().flex_1().min_h_0().min_w_0().overflow_hidden().bg(c.bg_primary);
        let view = self.model.read(cx).view().clone();
        // A markdown preview belongs to its file; another file opens on its source.
        if let Some(previewed) = &self.markdown_preview
            && !matches!(&view, FileView::Text { path, .. } if path == previewed)
            && !matches!(&view, FileView::Loading(_))
        {
            self.markdown_preview = None;
        }
        let (path, language, body): (String, SharedString, AnyElement) = match view {
            FileView::Nothing => {
                return frame.child(ui::viewer_placeholder("Select a file to view its contents", cx)).into_any_element();
            }
            FileView::Loading(_) => return frame.child(ui::viewer_placeholder("Loading...", cx)).into_any_element(),
            FileView::Failed { error: message, .. } => return frame.child(ui::viewer_error(message)).into_any_element(),
            FileView::TooLarge { path, size } => {
                let message = format!(
                    "File too large to open ({}, limit {})",
                    format_megabytes(size),
                    format_megabytes(max_read_bytes_for(&path))
                );
                let button = ui::viewer_button("open-anyway", None, cx)
                    .mt(px(12.))
                    .child(format!("Open anyway ({})", format_megabytes(size)))
                    .tooltip(|window, cx| {
                        gpui_kit::component::tooltip::Tooltip::new("Load the whole file despite the size limit").build(window, cx)
                    })
                    .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |model, cx| model.open(&path, true, cx))));
                return frame.child(ui::viewer_error(div().child(message)).child(button)).into_any_element();
            }
            FileView::Image { path, bytes, kind } => {
                use treeterm_model::file_browser::ImageKind;
                let format = match kind {
                    ImageKind::Png => gpui_kit::ImageFormat::Png,
                    ImageKind::Jpeg => gpui_kit::ImageFormat::Jpeg,
                    ImageKind::Gif => gpui_kit::ImageFormat::Gif,
                    ImageKind::Webp => gpui_kit::ImageFormat::Webp,
                    ImageKind::Svg => gpui_kit::ImageFormat::Svg,
                    ImageKind::Bmp => gpui_kit::ImageFormat::Bmp,
                };
                let image = std::sync::Arc::new(gpui_kit::Image::from_bytes(format, (*bytes).clone()));
                let body = div()
                    .size_full()
                    .p(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(gpui_kit::img(image).max_w_full().max_h_full().object_fit(gpui_kit::ObjectFit::Contain))
                    .into_any_element();
                (path, "image".into(), body)
            }
            FileView::Pdf { path, bytes } => {
                let page = self.page_for(path.clone(), || crate::browser_view::AssetPageView::pdf(bytes), cx);
                (path, "pdf".into(), div().size_full().child(page).into_any_element())
            }
            FileView::Model { path, bytes, format } => {
                let page = self.page_for(path.clone(), || crate::browser_view::AssetPageView::model(bytes, format.as_str()), cx);
                (path, format.as_str().into(), div().size_full().child(page).into_any_element())
            }
            FileView::Binary { path, .. } => {
                (path, "binary".into(), ui::viewer_placeholder("Binary file \u{2014} not shown as text", cx).into_any_element())
            }
            FileView::Text { path, text } => {
                let language: SharedString = detect_monaco_language(&path).into();
                let body = if self.markdown_preview.as_deref() == Some(path.as_str()) {
                    markdown_preview(SharedString::from(format!("md-{path}")), text, &self.preview_scroll, cx)
                } else {
                    self.text_body(&path, &text, window, cx)
                };
                let header = self.viewer_header(&path, language, true, cx);
                return frame.child(header).child(div().flex_1().min_h_0().overflow_hidden().child(body)).into_any_element();
            }
        };
        let header = self.viewer_header(&path, language, false, cx);
        frame.child(header).child(div().flex_1().min_h_0().overflow_hidden().child(body)).into_any_element()
    }

    /// The read-only editor (Monaco: 14px Menlo, 21px rows, 8px above the first line), and
    /// the file's review comments under it.
    fn text_body(&mut self, path: &str, text: &str, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let editor = self.editor_for(path, text, window, cx);
        // Monaco shows the search panel's matches only by selecting the chosen one.
        self.apply_reveal(&editor, window, cx);
        let overlay = self.definition_overlay(cx);
        let comments = self.model.read(cx).file_comments(cx);
        self.mark_comment_lines(path, text, comments.as_deref().unwrap_or_default(), cx);
        let panel = self.comments_panel(path, comments, window, cx);
        let bounds = self.editor_bounds.clone();
        let measure = gpui_kit::canvas(move |b, _, _| bounds.set(Some(b)), |_, _, _, _| {}).absolute().size_full();
        v_flex()
            .size_full()
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .key_context(VIEWER_CONTEXT)
                    .on_action(cx.listener(Self::on_go_to_definition))
                    .on_action(cx.listener(Self::on_search_word))
                    .children(overlay)
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
                            .child(measure)
                            .child(
                                Editor::new(&editor)
                                    .readonly(true)
                                    .bordered(false)
                                    .h_full()
                                    .font_family(MONO)
                                    .text_size(px(14.))
                                    .context_menu(|menu, _, _| {
                                        menu.menu("Go to Definition", Box::new(GoToDefinition))
                                            .separator()
                                            .menu("Copy", Box::new(CopyText))
                                            .menu("Search", Box::new(SearchWord))
                                            .separator()
                                            .menu("Select All", Box::new(SelectAll))
                                    }),
                            ),
                    ),
            )
            .children(panel)
            .into_any_element()
    }

    /// Tint the commented lines and mark them in the glyph margin, when they change.
    fn mark_comment_lines(&mut self, path: &str, text: &str, comments: &[ReviewComment], cx: &mut Context<Self>) {
        let mut marks: Vec<(u32, Freshness)> = comments.iter().map(|c| (c.line_number, c.freshness)).collect();
        marks.sort_unstable_by_key(|(line, freshness)| (*line, *freshness == Freshness::Current));
        marks.dedup_by_key(|(line, _)| *line);
        if self.marked.0 == path && self.marked.1 == marks {
            return;
        }
        let (Some(gutter), Some(lines)) = (&self.gutter, &self.comment_lines) else {
            return;
        };
        let spans: Vec<(std::ops::Range<usize>, Freshness)> =
            marks.iter().filter_map(|(line, freshness)| Some((line_bytes(text, *line)?, *freshness))).collect();
        gutter.set(
            spans.iter().map(|(range, freshness)| GutterDecoration::new(range.clone()).with_color(comment_line_colors(*freshness).1)).collect(),
            cx,
        );
        lines.set(
            spans
                .iter()
                .map(|(range, freshness)| {
                    RangeDecoration::new(range.clone())
                        .with_style(RangeDecorationStyle::Fill)
                        .with_color(comment_line_colors(*freshness).0)
                })
                .collect(),
            cx,
        );
        self.marked = (path.to_owned(), marks);
    }

    /// The draft's comment box, made when the draft changes.
    fn draft_box(&mut self, draft: &LineDraft, window: &mut Window, cx: &mut Context<Self>) -> (Entity<TextareaState>, bool) {
        if let Some(shown) = &self.draft_input
            && shown.draft == *draft
        {
            return (shown.input.clone(), !shown.text.trim().is_empty());
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Add your comment... (Cmd/Ctrl+Enter to submit, Esc to cancel)")
        });
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { secondary: true, .. } => {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.submit_comment(&text, cx));
            }
            InputEvent::Change => {
                if let Some(draft) = &mut view.draft_input {
                    draft.text = input.read(cx).value().to_string();
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.draft_input = Some(DraftInput { draft: draft.clone(), input: input.clone(), text: String::new(), _events: events });
        (input, false)
    }

    /// The file's review comments, grouped by line (`.inline-comment-display-group`), with
    /// the comment box (`.inline-comment-zone`) first; nothing while there are neither.
    fn comments_panel(
        &mut self,
        path: &str,
        comments: Result<Vec<ReviewComment>, MetadataError>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let c = colors(cx);
        let (border, accent, secondary, bg_secondary) = (c.border, c.accent, c.text_secondary, c.bg_secondary);
        let (draft, adding, notice) = {
            let model = self.model.read(cx);
            (model.comment_draft().filter(|d| d.path == path).cloned(), model.adding_comment().clone(), model.comment_notice().clone())
        };
        let comments = match comments {
            Ok(comments) => comments,
            Err(error) => {
                return Some(
                    div().px(px(12.)).py(px(8.)).border_t_1().border_color(border).child(ui::viewer_error(error.to_string())).into_any_element(),
                );
            }
        };
        let notice = match notice {
            Notice::Clear => None,
            Notice::Error(error) => Some(error),
        };
        let draft = match draft {
            Some(draft) => Some(draft),
            None => {
                self.draft_input = None;
                None
            }
        };
        if comments.is_empty() && draft.is_none() && notice.is_none() {
            return None;
        }
        let (delete, cancel, submit, reveal) = (self.model.clone(), self.model.clone(), self.model.clone(), self.model.clone());
        let shown = treeterm_core::search::to_workspace_relative(self.model.read(cx).root(), path).to_owned();
        let actions = CommentActions {
            delete: Rc::new(move |id, cx| delete.update(cx, |m, cx| m.delete_comment(id, cx))),
            cancel: Rc::new(move |cx| cancel.update(cx, |m, cx| m.cancel_comment(cx))),
            submit: Rc::new(move |text, cx| submit.update(cx, |m, cx| m.submit_comment(text, cx))),
        };
        let placement = Placement::Viewer {
            reveal: Rc::new(move |line, cx| reveal.update(cx, |m, cx| m.open_location(&shown, line as usize, 1, 0, cx))),
        };
        let draft_zone = draft.map(|draft| {
            let (input, has_text) = self.draft_box(&draft, window, cx);
            let label = format!("Comment on line {}", draft.line_number);
            div()
                .mx(px(8.))
                .my(px(4.))
                .p(px(12.))
                .rounded(px(4.))
                .border_1()
                .border_color(accent)
                .bg(bg_secondary)
                .shadow_md()
                .child(comment_input(&input, has_text, label, &adding, &actions, window, cx))
                .into_any_element()
        });
        let mut lines: Vec<(u32, Vec<ReviewComment>)> = Vec::new();
        for comment in comments {
            match lines.iter_mut().find(|(line, _)| *line == comment.line_number) {
                Some((_, group)) => group.push(comment),
                None => lines.push((comment.line_number, vec![comment])),
            }
        }
        lines.sort_by_key(|(line, _)| *line);
        let count: usize = lines.iter().map(|(_, group)| group.len()).sum();
        let groups = lines.into_iter().map(|(line, group)| {
            v_flex()
                .id(SharedString::from(format!("comment-group-{line}")))
                .gap(px(4.))
                .mx(px(8.))
                .my(px(4.))
                .px(px(8.))
                .py(px(2.))
                .rounded(px(4.))
                .border_1()
                .border_color(border)
                .bg(bg_secondary)
                .children(group.iter().map(|comment| comment_display(comment, &placement, &actions, cx)))
                .into_any_element()
        });
        let title = h_flex()
            .flex_shrink_0()
            .gap(px(6.))
            .px(px(12.))
            .py(px(6.))
            .text_size(px(11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(secondary)
            .child(format!("REVIEW COMMENTS ({count})"));
        let body = v_flex()
            .pb(px(4.))
            .children(draft_zone)
            .children(groups)
            .when_some(notice, |d, error| {
                d.child(div().mx(px(12.)).my(px(4.)).text_size(px(12.)).text_color(rgb(0xf85149)).child(error))
            });
        Some(
            v_flex()
                .flex_shrink_0()
                .max_h(relative(0.4))
                .border_t_1()
                .border_color(border)
                .bg(colors(cx).bg_primary)
                .child(title)
                .child(div().flex_1().min_h_0().child(ui::scroll_area("file-comments", &self.comments_scroll, body, cx)))
                .into_any_element(),
        )
    }

    // ---- search panel ------------------------------------------------------------------

    /// `.search-panel`: the find bar over the results, along the bottom of the viewer.
    fn search_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let SearchPanel::Open(search) = self.model.read(cx).search().clone() else {
            return None;
        };
        let input = self.search_input(window, cx);
        let (rows, count, has_matches) = {
            let model = self.model.read(cx);
            (model.search_rows(), model.match_count_label(), !model.file_matches().is_empty())
        };
        let (select, open, step, close) = (self.model.clone(), self.model.clone(), self.model.clone(), self.model.clone());
        let actions = crate::search_panel::PanelActions {
            select: Rc::new(move |index, cx| select.update(cx, |m, cx| m.select_match(index, cx))),
            open_repo: Rc::new(move |hit, cx| open.update(cx, |m, cx| m.open_repo_hit(hit, cx))),
            step: Rc::new(move |forward, cx| step.update(cx, |m, cx| m.step_match(forward, cx))),
            close: Rc::new(move |cx| close.update(cx, |m, cx| m.close_search(cx))),
        };
        let bar = crate::search_panel::Bar { input, count, has_matches, has_query: !search.query.is_empty() };
        Some(crate::search_panel::panel(bar, rows, &mut self.search_results, &actions, window, cx))
    }
}

/// `MarkdownPreview`: GitHub-flavoured markdown in 20px of padding.
pub(crate) fn markdown_preview(id: SharedString, text: String, scroll: &gpui_kit::ScrollHandle, cx: &App) -> AnyElement {
    markdown::preview(id, text, scroll, None, cx)
}

/// [`markdown_preview`] whose links go to `on_link` (with the `href` as written).
pub(crate) fn markdown_preview_linked(
    id: SharedString,
    text: String,
    scroll: &gpui_kit::ScrollHandle,
    on_link: impl Fn(&str, &mut Window, &mut App) + Send + Sync + 'static,
    cx: &App,
) -> AnyElement {
    let on_link: markdown::LinkHandler = std::sync::Arc::new(on_link);
    markdown::preview(id, text, scroll, Some(on_link), cx)
}

impl Render for FileBrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (border, accent, secondary, primary, hover) = (c.border, c.accent, c.text_secondary, c.text_primary, c.bg_hover);
        if self.resizing && !cx.has_active_drag() {
            self.resizing = false;
        }
        let tree: Vec<AnyElement> = if self.layout == Layout::Viewer {
            Vec::new()
        } else if self.tree_collapsed {
            vec![
                div()
                    .id("show-file-tree")
                    .flex_shrink_0()
                    .self_start()
                    .m(px(4.))
                    .p(px(4.))
                    .flex()
                    .rounded(px(4.))
                    .text_color(secondary)
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover).text_color(primary))
                    .child(Icon::new(IconName::PanelLeftOpen).size(px(16.)))
                    .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Show file tree").build(window, cx))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.tree_collapsed = false;
                        cx.notify();
                    }))
                    .into_any_element(),
            ]
        } else {
            let resizing = self.resizing;
            vec![
                self.tree_pane(window, cx),
                // `.divider`: 4px, accent while hovered or dragged.
                div()
                    .id("file-tree-divider")
                    .w(px(4.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(if resizing { accent } else { border })
                    .hover(move |d| d.bg(accent))
                    .cursor_col_resize()
                    .on_drag(TreeResize, |drag, _, _, cx| cx.new(|_| drag.clone()))
                    .into_any_element(),
            ]
        };
        let viewer = self.viewer(window, cx);
        let search = self.search_panel(window, cx);
        h_flex()
            .size_full()
            .items_stretch()
            .on_action(cx.listener(Self::on_search))
            .on_drag_move(cx.listener(|view, event: &DragMoveEvent<TreeResize>, _, cx| {
                let width = f32::from(event.event.position.x - event.bounds.left()).clamp(TREE_MIN_WIDTH, TREE_MAX_WIDTH);
                view.resizing = true;
                if (width - view.tree_width).abs() >= 1. {
                    view.tree_width = width;
                }
                cx.notify();
            }))
            .children(tree)
            .child(v_flex().flex_1().min_w_0().h_full().overflow_hidden().child(viewer).children(search))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn languages_follow_extensions_and_names() {
        assert_eq!(language_for("/w/src/main.rs"), "rust");
        assert_eq!(language_for("/w/App.tsx"), "tsx");
        assert_eq!(language_for("/w/Makefile"), "make");
        assert_eq!(language_for("/w/README"), "text");
    }

    #[test]
    fn sizes_read_in_megabytes_to_one_place() {
        assert_eq!(format_megabytes(5_000_000), "4.8MB");
        assert_eq!(format_megabytes(1024 * 1024), "1.0MB");
    }

    #[test]
    fn the_first_match_is_split_out_of_the_name() {
        assert_eq!(split_match("FileTree.tsx", "tree"), Some(("File", "Tree", ".tsx")));
        assert_eq!(split_match("main.rs", "MAIN"), Some(("", "main", ".rs")));
        assert_eq!(split_match("main.rs", "x"), None);
        assert_eq!(split_match("main.rs", " "), None);
    }

    #[test]
    fn comment_lines_are_found_by_offset_and_number() {
        let text = "one\n\nthree";
        assert_eq!(line_at(text, 0), 1);
        assert_eq!(line_at(text, 4), 2);
        assert_eq!(line_at(text, 5), 3);
        assert_eq!(line_at(text, 99), 3);
        assert_eq!(line_bytes(text, 1), Some(0..4));
        assert_eq!(line_bytes(text, 2), Some(4..5));
        assert_eq!(line_bytes(text, 3), Some(5..10));
        assert_eq!(line_bytes(text, 4), None);
        assert_eq!(line_bytes(text, 0), None);
        // The empty line after a final newline has nothing to mark.
        assert_eq!(line_bytes("one\n", 1), Some(0..4));
        assert_eq!(line_bytes("one\n", 2), None);
    }

    #[test]
    fn the_glyph_strip_makes_up_monacos_gutter() {
        // GPUI Kit's gutter is 34px plus the numbers; the strip brings it to Monaco's.
        let gutter = gutter_width;
        assert!((gutter(18, true) - 89.15).abs() < 0.01);
        assert!((gutter(500, false) - 68.15).abs() < 0.01);
        // Monaco's numbers stay 5 digits wide until there are more lines than that.
        assert!((gutter(4000, true) - 89.15).abs() < 0.01);
        // Without a glyph margin, past 5 digits GPUI Kit's fixed columns are already wider.
        assert_eq!(glyph_strip(250_000, false), 0.);
    }
}
