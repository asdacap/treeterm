//! The Files tab. Port of the view half of `FilesystemBrowser.tsx`: a directory tree and a
//! read-only, syntax-highlighted viewer (tree-sitter through GPUI Kit's code editor).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::file_browser::{FileBrowser, FileView, Listing};

use crate::theme::colors;

pub struct FileBrowserView {
    model: Entity<FileBrowser>,
    /// The editor showing the current file, and which file it shows.
    editor: Option<(String, Entity<EditorState>)>,
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
    pub fn new(model: Entity<FileBrowser>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            editor: None,
            _observe: observe,
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
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language).default_value(text));
        self.editor = Some((path.to_owned(), editor.clone()));
        editor
    }

    fn tree(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let model = self.model.read(cx);
        let selected = model.selected().map(str::to_owned);
        match model.root_listing() {
            Some(Listing::Failed(error)) => {
                return div().p_2().text_sm().text_color(c.error).child(error.clone()).into_any_element();
            }
            Some(Listing::Loading) | None => {
                return div().p_2().text_sm().text_color(c.text_secondary).child("Loading…").into_any_element();
            }
            Some(Listing::Loaded(_)) => {}
        }
        let rows = model.rows();
        let mut list = v_flex().id("file-tree").size_full().overflow_y_scroll().py_1();
        for row in rows {
            let is_selected = selected.as_deref() == Some(row.path.as_str());
            let icon = match (row.is_dir, row.expanded) {
                (true, true) => "▾",
                (true, false) => "▸",
                (false, _) => " ",
            };
            let path = row.path.clone();
            let is_dir = row.is_dir;
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
                    .child(div().truncate().child(row.name.clone()))
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

    fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error, border) = (c.text_secondary, c.error, c.border);
        let centered = |child: AnyElement| div().size_full().flex().items_center().justify_center().child(child).into_any_element();
        let view = self.model.read(cx).view().clone();
        match view {
            FileView::Nothing => centered(div().text_color(secondary).child("Select a file").into_any_element()),
            FileView::Loading(_) => centered(div().text_color(secondary).child("Loading…").into_any_element()),
            FileView::Failed { error: message, .. } => centered(div().text_color(error).child(message).into_any_element()),
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
                let editor = self.editor_for(&path, &text, window, cx);
                v_flex()
                    .size_full()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(secondary)
                            .border_b_1()
                            .border_color(border)
                            .child(path.clone()),
                    )
                    .child(div().flex_1().min_h_0().child(Editor::new(&editor).readonly(true).bordered(false).h_full()))
                    .into_any_element()
            }
        }
    }
}

impl Render for FileBrowserView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let border = colors(cx).border;
        let tree = self.tree(cx);
        let viewer = self.viewer(window, cx);
        h_flex()
            .size_full()
            .child(div().w(px(260.)).h_full().flex_shrink_0().border_r_1().border_color(border).child(tree))
            .child(div().flex_1().min_w_0().h_full().child(viewer))
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
