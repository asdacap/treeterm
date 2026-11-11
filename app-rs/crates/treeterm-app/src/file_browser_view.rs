//! The Files tab. Port of the view half of `FilesystemBrowser.tsx`, `FileViewer.tsx` and
//! `SearchPanel.tsx`: a directory tree, a read-only, syntax-highlighted viewer (tree-sitter
//! through GPUI Kit's code editor) and the search panel.
//!
//! In the viewer, Cmd/Ctrl+F opens the search panel with the selection, Cmd/Ctrl+click or
//! F12 goes to the definition of the word under the cursor, and the context menu offers
//! both.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    Copy as CopyText, Editor, EditorState, Escape, Input, InputEvent, InputState, RangeDecoration,
    RangeDecorationCollection, RangeDecorationStyle, Search, SelectAll,
};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Context, Entity, EntityId, Focusable as _, Hsla, KeyBinding, MouseButton, MouseUpEvent, SharedString,
    Subscription, Window, actions, div, px, rgba, uniform_list,
};
use treeterm_core::files::favourite_file_name;
use treeterm_model::file_browser::{DefinitionLookup, Favourites, FileBrowser, FileView, Listing, SearchPanel, SearchRow, Star};

use crate::theme::colors;

actions!(file_browser, [GoToDefinition, SearchWord]);

/// The viewer and its editor.
const VIEWER_CONTEXT: &str = "FileViewer";

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("f12", GoToDefinition, Some(VIEWER_CONTEXT))]);
}

/// Height of a search panel row (the list is virtualized at a uniform height).
const SEARCH_ROW_HEIGHT: f32 = 22.;

pub struct FileBrowserView {
    model: Entity<FileBrowser>,
    /// The PDF shown, and the webview showing it.
    pdf: Option<(String, Entity<crate::browser_view::PdfView>)>,
    /// Markdown files show their source instead of the preview.
    markdown_source: bool,
    workspace: Entity<treeterm_model::workspace_store::WorkspaceStore>,
    /// The editor showing the current file, and which file it shows.
    editor: Option<(String, Entity<EditorState>)>,
    /// The model's last [`treeterm_model::file_browser::Reveal`] applied to the editor.
    revealed: u64,
    /// The search matches highlighted in the editor, and the editor and query they are for.
    highlights: Option<(EntityId, String, RangeDecorationCollection)>,
    /// The search panel's query box, made when the panel first opens (it needs the window).
    search_input: Option<(Entity<InputState>, Subscription)>,
    _observe: Subscription,
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

fn format_size(bytes: u64) -> String {
    match bytes {
        b if b >= 1 << 20 => format!("{:.1} MB", b as f64 / (1u64 << 20) as f64),
        b if b >= 1 << 10 => format!("{:.1} KB", b as f64 / 1024.),
        b => format!("{b} B"),
    }
}

impl FileBrowserView {
    pub fn new(
        model: Entity<FileBrowser>,
        workspace: Entity<treeterm_model::workspace_store::WorkspaceStore>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            pdf: None,
            markdown_source: false,
            workspace,
            editor: None,
            revealed: 0,
            highlights: None,
            search_input: None,
            _observe: observe,
        }
    }

    /// Show the PDF webview only while this tab is shown, a PDF is the current file, and no
    /// dialog covers it.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        let is_pdf = matches!(self.model.read(cx).view(), FileView::Pdf { .. });
        if let Some((_, pdf)) = &self.pdf {
            pdf.update(cx, |p, cx| p.set_shown(shown && is_pdf, cx));
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
        let text = text.to_owned();
        // Not searchable: Cmd/Ctrl+F reaches this view's search panel instead.
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language).searchable(false).default_value(text));
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
            InputEvent::Focus | InputEvent::Blur => {}
        });
        self.search_input = Some((input.clone(), events));
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

    fn toggle_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match self.model.read(cx).search() {
            SearchPanel::Open(_) => self.model.update(cx, |m, cx| m.close_search(cx)),
            SearchPanel::Closed => {
                self.model.update(cx, |m, cx| m.open_search("", None, cx));
                self.focus_search(window, cx);
            }
        }
    }

    fn on_go_to_definition(&mut self, _: &GoToDefinition, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.shown_editor(cx) else {
            return;
        };
        let offset = editor.read(cx).cursor();
        self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
    }

    /// Cmd/Ctrl+click: the press already put the cursor on the clicked word.
    fn on_editor_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        if !event.modifiers.secondary(cx) {
            return;
        }
        let Some(editor) = self.shown_editor(cx) else {
            return;
        };
        let (selected, offset) = {
            let editor = editor.read(cx);
            (editor.selected_range(), editor.cursor())
        };
        if selected.is_empty() {
            self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
        }
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
        editor.update(cx, |editor, cx| editor.set_selected_range(range, cx));
    }

    /// Highlight every search match in the editor.
    fn sync_highlights(&mut self, editor: &Entity<EditorState>, cx: &mut Context<Self>) {
        let query = match self.model.read(cx).search() {
            SearchPanel::Open(search) => search.query.clone(),
            SearchPanel::Closed => String::new(),
        };
        let current = match &self.highlights {
            Some((id, shown, _)) if *id == editor.entity_id() && *shown == query => return,
            Some((id, _, collection)) if *id == editor.entity_id() => Some(collection.clone()),
            Some(_) | None => None,
        };
        let color: Hsla = rgba(0xe2c08d55).into();
        let decorations: Vec<RangeDecoration> = self
            .model
            .read(cx)
            .file_matches()
            .into_iter()
            .map(|found| RangeDecoration::new(found.range).with_style(RangeDecorationStyle::Fill).with_color(color))
            .collect();
        let collection = match current {
            Some(collection) => {
                collection.set(decorations, cx);
                collection
            }
            None => editor.update(cx, |editor, cx| editor.create_range_decorations_collection(decorations, cx)),
        };
        self.highlights = Some((editor.entity_id(), query, collection));
    }

    fn tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let model = self.model.read(cx);
        let selected = model.selected().map(str::to_owned);
        let searching = matches!(model.search(), SearchPanel::Open(_));
        let header = h_flex()
            .px_2()
            .h(px(28.))
            .items_center()
            .border_b_1()
            .border_color(c.border)
            .child(div().flex_1().text_xs().text_color(c.text_secondary).child("FILES"))
            .child(
                Button::new("toggle-search")
                    .xsmall()
                    .ghost()
                    .label(if searching { "Close search" } else { "Search" })
                    .on_click(cx.listener(|view, _, window, cx| view.toggle_search(window, cx))),
            );
        let body = match model.root_listing() {
            Some(Listing::Failed(error)) => div().p_2().text_sm().text_color(c.error).child(error.clone()).into_any_element(),
            Some(Listing::Loading) | None => {
                div().p_2().text_sm().text_color(c.text_secondary).child("Loading…").into_any_element()
            }
            Some(Listing::Loaded(_)) => {
                let mut list = v_flex().id("file-tree").flex_1().min_h_0().overflow_y_scroll().py_1();
                if let Some(favourites) = self.favourites_section(cx) {
                    list = list.child(favourites);
                }
                let model = self.model.read(cx);
                for row in model.rows() {
                    let is_selected = selected.as_deref() == Some(row.path.as_str());
                    let icon = match (row.is_dir, row.expanded) {
                        (true, true) => "▾",
                        (true, false) => "▸",
                        (false, _) => " ",
                    };
                    let path = row.path.clone();
                    let is_dir = row.is_dir;
                    let star = model.star(&row.path, cx);
                    let star_path = row.path.clone();
                    let star_button = Button::new(SharedString::from(format!("star-{}", row.path)))
                        .xsmall()
                        .ghost()
                        .label(match star {
                            Star::Off => "☆",
                            Star::On | Star::Covered => "★",
                        })
                        .disabled(star == Star::Covered)
                        .tooltip(match star {
                            Star::Off => "Add to favourites",
                            Star::On => "Remove from favourites",
                            Star::Covered => "Included by a favourite folder",
                        })
                        .on_click(cx.listener(move |view, _, _, cx| {
                            cx.stop_propagation();
                            view.model.update(cx, |m, cx| m.toggle_favourite(&star_path, cx));
                        }));
                    list = list.child(
                        h_flex()
                            .id(SharedString::from(format!("file-{}", row.path)))
                            .h(px(22.))
                            .pl(px(8. + 12. * row.depth as f32))
                            .gap_1()
                            .text_sm()
                            .cursor_pointer()
                            .when(is_selected, |d| d.bg(c.bg_active))
                            .when(!is_selected, |d| d.hover(move |d| d.bg(c.bg_hover)))
                            .child(div().w(px(12.)).text_color(c.text_secondary).child(icon))
                            .child(div().flex_1().min_w_0().truncate().child(row.name.clone()))
                            .child(star_button)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |model, cx| {
                                    if is_dir {
                                        model.toggle_dir(&path, cx);
                                    } else {
                                        model.open(&path, false, cx);
                                    }
                                });
                            })),
                    );
                }
                list.into_any_element()
            }
        };
        v_flex().size_full().child(header).child(body).into_any_element()
    }

    /// The files the favourites name, above the tree (`FavouriteFilesSection`).
    fn favourites_section(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, error, hover, active, border) = (c.text_secondary, c.error, c.bg_hover, c.bg_active, c.border);
        let model = self.model.read(cx);
        let selected = model.selected().map(str::to_owned);
        let note = |text: String, color| div().px_2().py_1().text_xs().text_color(color).child(text).into_any_element();
        let body = match model.favourites().clone() {
            Favourites::Empty => return None,
            Favourites::Loading => vec![note("Loading favourites…".into(), secondary)],
            Favourites::Failed(message) => vec![
                note(message, error),
                Button::new("retry-favourites")
                    .xsmall()
                    .ghost()
                    .label("Retry")
                    .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.resolve_favourites(cx))))
                    .into_any_element(),
            ],
            Favourites::Ready(files) if files.is_empty() => {
                vec![note("No favourite files exist in this workspace".into(), secondary)]
            }
            Favourites::Ready(files) => files
                .into_iter()
                .enumerate()
                .map(|(index, file)| {
                    let is_selected = selected.as_deref() == Some(file.path.as_str());
                    let name = favourite_file_name(&file).to_owned();
                    let path = file.path.clone();
                    h_flex()
                        .id(("favourite", index))
                        .h(px(22.))
                        .pl(px(20.))
                        .text_sm()
                        .cursor_pointer()
                        .when(is_selected, |d| d.bg(active))
                        .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                        .child(div().truncate().child(name))
                        .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.open(&path, false, cx))))
                        .into_any_element()
                })
                .collect(),
        };
        Some(
            v_flex()
                .pb_1()
                .mb_1()
                .border_b_1()
                .border_color(border)
                .child(div().px_2().py_1().text_xs().text_color(secondary).child("★ FAVOURITES"))
                .children(body)
                .into_any_element(),
        )
    }

    /// Go-to-definition progress, failures and the pick list, over the viewer.
    fn definition_overlay(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let c = colors(cx);
        let (secondary, error, border, bg, hover) = (c.text_secondary, c.error, c.border, c.bg_secondary, c.bg_hover);
        let close = |cx: &mut Context<Self>| {
            Button::new("close-definition")
                .xsmall()
                .ghost()
                .label("×")
                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.dismiss_definition(cx))))
        };
        let status = |text: String, color, cx: &mut Context<Self>| {
            h_flex()
                .px_3()
                .py_1()
                .gap_2()
                .items_center()
                .text_xs()
                .bg(bg)
                .border_b_1()
                .border_color(border)
                .child(div().flex_1().text_color(color).child(text))
                .child(close(cx))
                .into_any_element()
        };
        match self.model.read(cx).definition().clone() {
            DefinitionLookup::Idle => None,
            DefinitionLookup::Searching { symbol } => Some(status(format!("Finding the definition of {symbol}…"), secondary, cx)),
            DefinitionLookup::NotFound { symbol } => Some(status(format!("No definition found for {symbol}"), secondary, cx)),
            DefinitionLookup::Unavailable { language } => {
                Some(status(format!("Go to definition is not available for {language} files"), secondary, cx))
            }
            DefinitionLookup::Failed { symbol, error: message } => {
                Some(status(format!("Could not look up {symbol}: {message}"), error, cx))
            }
            DefinitionLookup::Choose { symbol, locations } => {
                let mut list = v_flex().id("definitions").max_h(px(240.)).overflow_y_scroll();
                for (index, location) in locations.iter().enumerate() {
                    list = list.child(
                        div()
                            .id(("definition", index))
                            .px_3()
                            .py_1()
                            .text_sm()
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child(format!("{}:{}", location.file_path, location.line_number))
                            .on_click(cx.listener(move |view, _, _, cx| {
                                view.model.update(cx, |m, cx| m.choose_definition(index, cx));
                            })),
                    );
                }
                Some(
                    v_flex()
                        .bg(bg)
                        .border_b_1()
                        .border_color(border)
                        .child(
                            h_flex()
                                .px_3()
                                .py_1()
                                .items_center()
                                .text_xs()
                                .child(div().flex_1().text_color(secondary).child(format!(
                                    "{} definitions of {symbol}",
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

    /// "Download": pick where to save a copy of `path` on this computer.
    fn download_button(&self, path: &str, cx: &mut Context<Self>) -> AnyElement {
        use treeterm_model::file_browser::DownloadState;
        let state = self.model.read(cx).download_state().clone();
        let error_color = colors(cx).error;
        let (label, busy) = match &state {
            DownloadState::Idle => ("Download", false),
            DownloadState::Downloading => ("Downloading…", true),
            DownloadState::Failed(_) => ("Retry download", false),
        };
        let (model, owned) = (self.model.clone(), path.to_owned());
        h_flex()
            .gap_2()
            .when_some(
                match state {
                    DownloadState::Failed(error) => Some(error),
                    DownloadState::Idle | DownloadState::Downloading => None,
                },
                |d, error| d.child(div().text_xs().text_color(error_color).truncate().child(format!("Download failed: {error}"))),
            )
            .child(Button::new("download-file").xsmall().ghost().label(label).disabled(busy).on_click(cx.listener(
                move |_, _, _, cx| {
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
                },
            )))
            .into_any_element()
    }

    fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let body = self.viewer_body(window, cx);
        // Views without their own header get a bar with Download.
        let path = match self.model.read(cx).view() {
            FileView::Image { path, .. } | FileView::Pdf { path, .. } | FileView::Binary { path, .. } | FileView::TooLarge { path, .. } => {
                Some(path.clone())
            }
            FileView::Nothing | FileView::Loading(_) | FileView::Failed { .. } | FileView::Text { .. } => None,
        };
        let Some(path) = path else {
            return body;
        };
        let c = colors(cx);
        let (secondary, border) = (c.text_secondary, c.border);
        let download = self.download_button(&path, cx);
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .text_color(secondary)
                    .border_b_1()
                    .border_color(border)
                    .child(div().flex_1().truncate().child(path))
                    .child(download),
            )
            .child(div().flex_1().min_h_0().child(body))
            .into_any_element()
    }

    fn viewer_body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error, border) = (c.text_secondary, c.error, c.border);
        let centered = |child: AnyElement| div().size_full().flex().items_center().justify_center().child(child).into_any_element();
        let view = self.model.read(cx).view().clone();
        match view {
            FileView::Nothing => centered(div().text_color(secondary).child("Select a file").into_any_element()),
            FileView::Loading(_) => centered(div().text_color(secondary).child("Loading…").into_any_element()),
            FileView::Failed { error: message, .. } => centered(div().text_color(error).child(message).into_any_element()),
            FileView::Image { bytes, kind, .. } => {
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
                div()
                    .size_full()
                    .p_4()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(gpui_kit::img(image).max_w_full().max_h_full().object_fit(gpui_kit::ObjectFit::Contain))
                    .into_any_element()
            }
            FileView::Pdf { path, bytes } => {
                let view = match &self.pdf {
                    Some((shown, view)) if *shown == path => view.clone(),
                    Some(_) | None => {
                        let view = cx.new(|_| crate::browser_view::PdfView::new(bytes));
                        self.pdf = Some((path, view.clone()));
                        view
                    }
                };
                div().size_full().child(view).into_any_element()
            }
            FileView::Binary { size, .. } => {
                centered(div().text_color(secondary).child(format!("Binary file ({})", format_size(size))).into_any_element())
            }
            FileView::TooLarge { path, size } => centered(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(div().text_color(secondary).child(format!("This file is {}.", format_size(size))))
                    .child(Button::new("open-anyway").small().primary().label("Open anyway").on_click(cx.listener(
                        move |view, _, _, cx| view.model.update(cx, |model, cx| model.open(&path, true, cx)),
                    )))
                    .into_any_element(),
            ),
            FileView::Text { path, text } => {
                // Markdown opens as a preview (`MarkdownPreview`), with a Source toggle.
                let markdown = language_for(&path) == "markdown";
                if markdown && !self.markdown_source {
                    return v_flex()
                        .size_full()
                        .child(
                            h_flex()
                                .px_3()
                                .py_1()
                                .gap_2()
                                .items_center()
                                .text_xs()
                                .text_color(secondary)
                                .border_b_1()
                                .border_color(border)
                                .child(div().flex_1().truncate().child(path.clone()))
                                .child(Button::new("md-source").xsmall().ghost().label("</> Source").on_click(cx.listener(
                                    |view, _, _, cx| {
                                        view.markdown_source = true;
                                        cx.notify();
                                    },
                                ))),
                        )
                        .child(
                            div()
                                .id("md-preview")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .p_4()
                                .child(gpui_kit::component::text::TextView::markdown(SharedString::from(format!("md-{path}")), text)),
                        )
                        .into_any_element();
                }
                let editor = self.editor_for(&path, &text, window, cx);
                self.sync_highlights(&editor, cx);
                self.apply_reveal(&editor, window, cx);
                let edit_path = path.clone();
                let overlay = self.definition_overlay(cx);
                v_flex()
                    .size_full()
                    .key_context(VIEWER_CONTEXT)
                    .on_action(cx.listener(Self::on_go_to_definition))
                    .on_action(cx.listener(Self::on_search_word))
                    .child(
                        h_flex()
                            .px_3()
                            .py_1()
                            .gap_2()
                            .items_center()
                            .text_xs()
                            .text_color(secondary)
                            .border_b_1()
                            .border_color(border)
                            .child(div().flex_1().truncate().child(path.clone()))
                            .when(markdown, |d| {
                                d.child(Button::new("md-preview-toggle").xsmall().ghost().label("Preview").on_click(cx.listener(
                                    |view, _, _, cx| {
                                        view.markdown_source = false;
                                        cx.notify();
                                    },
                                )))
                            })
                            .child(self.download_button(&path, cx))
                            .child(Button::new("edit-file").xsmall().ghost().label("Edit").on_click(cx.listener(
                                move |view, _, _, cx| {
                                    view.workspace.update(cx, |w, cx| w.open_in_editor(&edit_path, cx));
                                },
                            ))),
                    )
                    .children(overlay)
                    .child(
                        div()
                            .flex_1()
                            .min_h_0()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
                            .child(
                                Editor::new(&editor)
                                    .readonly(true)
                                    .bordered(false)
                                    .h_full()
                                    .context_menu(|menu, _, _| {
                                        menu.menu("Copy", Box::new(CopyText))
                                            .menu("Select All", Box::new(SelectAll))
                                            .separator()
                                            .menu("Search", Box::new(SearchWord))
                                            .menu("Go to Definition", Box::new(GoToDefinition))
                                    }),
                            ),
                    )
                    .into_any_element()
            }
        }
    }

    fn search_panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<AnyElement> {
        let SearchPanel::Open(_) = self.model.read(cx).search() else {
            return None;
        };
        let c = colors(cx);
        let (secondary, error, border, hover, active, accent) =
            (c.text_secondary, c.error, c.border, c.bg_hover, c.bg_active, c.accent);
        let input = self.search_input(window, cx);
        let (rows, count, has_matches) = {
            let model = self.model.read(cx);
            (model.search_rows(), model.match_count_label(), !model.file_matches().is_empty())
        };
        let rows = Rc::new(rows);
        let model = self.model.clone();
        let list = uniform_list("search-results", rows.len(), {
            let rows = rows.clone();
            move |range, _, _| {
                range
                    .map(|index| {
                        let row = div().id(("search-row", index)).h(px(SEARCH_ROW_HEIGHT)).px_2().flex().items_center().text_xs();
                        match rows[index].clone() {
                            SearchRow::Section(title) => row.pt_1().text_color(secondary).child(title.to_uppercase()),
                            SearchRow::File(path) => row.text_color(accent).child(div().truncate().child(path)),
                            SearchRow::Note(text) => row.text_color(secondary).child(text),
                            SearchRow::Error(text) => row.text_color(error).child(div().truncate().child(text)),
                            SearchRow::InFile { index: at, line_number, preview, active: is_active } => {
                                let model = model.clone();
                                row.gap_2()
                                    .cursor_pointer()
                                    .when(is_active, |d| d.bg(active))
                                    .when(!is_active, |d| d.hover(move |d| d.bg(hover)))
                                    .child(div().w(px(36.)).flex_none().text_color(secondary).child(line_number.to_string()))
                                    .child(div().flex_1().truncate().child(preview.trim().to_owned()))
                                    .on_click(move |_, _, cx| model.update(cx, |m, cx| m.select_match(at, cx)))
                            }
                            SearchRow::InRepo(hit) => {
                                let model = model.clone();
                                let (line, preview) = (hit.line_number, hit.preview.trim().to_owned());
                                row.gap_2()
                                    .cursor_pointer()
                                    .hover(move |d| d.bg(hover))
                                    .child(div().w(px(36.)).flex_none().text_color(secondary).child(line.to_string()))
                                    .child(div().flex_1().truncate().child(preview))
                                    .on_click(move |_, _, cx| model.update(cx, |m, cx| m.open_repo_hit(&hit, cx)))
                            }
                        }
                    })
                    .collect()
            }
        });
        let nav = |id: &'static str, label: &'static str, forward: bool, cx: &mut Context<Self>| {
            Button::new(id)
                .xsmall()
                .ghost()
                .label(label)
                .disabled(!has_matches)
                .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.step_match(forward, cx))))
        };
        Some(
            v_flex()
                .w(px(340.))
                .h_full()
                .flex_shrink_0()
                .border_l_1()
                .border_color(border)
                .on_action(cx.listener(|view, _: &Escape, _, cx| view.model.update(cx, |m, cx| m.close_search(cx))))
                .child(
                    h_flex()
                        .p_1()
                        .gap_1()
                        .items_center()
                        .border_b_1()
                        .border_color(border)
                        .child(div().flex_1().child(Input::new(&input).small()))
                        .child(div().text_xs().text_color(secondary).child(count))
                        .child(nav("search-prev", "↑", false, cx))
                        .child(nav("search-next", "↓", true, cx))
                        .child(Button::new("search-close").xsmall().ghost().label("×").on_click(
                            cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.close_search(cx))),
                        )),
                )
                .child(list.flex_1().min_h_0())
                .into_any_element(),
        )
    }
}

impl Render for FileBrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = colors(cx).border;
        let tree = self.tree(cx);
        let viewer = self.viewer(window, cx);
        let search = self.search_panel(window, cx);
        h_flex()
            .size_full()
            .on_action(cx.listener(Self::on_search))
            .child(div().w(px(260.)).h_full().flex_shrink_0().border_r_1().border_color(border).child(tree))
            .child(div().flex_1().min_w_0().h_full().child(viewer))
            .children(search)
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
        assert_eq!(format_size(1536), "1.5 KB");
    }
}
