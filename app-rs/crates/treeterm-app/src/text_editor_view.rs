//! The Editor tab view. Port of the view half of `FileEditor.tsx` (Monaco → GPUI Kit's
//! code editor with tree-sitter highlighting), styled as `.file-editor*` in `index.css`: a
//! header with the file name and its unsaved mark, the markdown Edit/Preview toggle, Save
//! and the language, over the editor or the markdown preview.

use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::v_flex;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, KeyBinding, PromptLevel, SharedString, Subscription, Window, actions, div, px, rgb};
use treeterm_model::text_editor::{EditorDoc, SaveState, TextEditor};

use crate::file_browser_view::language_for;
use crate::theme::colors;
use crate::ui;

actions!(text_editor, [Save]);

const CONTEXT: &str = "TextEditor";

pub fn bind_keys(cx: &mut gpui_kit::App) {
    let key = if cfg!(target_os = "macos") { "cmd-s" } else { "ctrl-s" };
    cx.bind_keys([KeyBinding::new(key, Save, Some(CONTEXT))]);
}

pub struct TextEditorView {
    model: Entity<TextEditor>,
    editor: Option<Entity<EditorState>>,
    /// The editor's glyph margin (Monaco's gutter width).
    gutter: Option<gpui_kit::component::input::GutterDecorationCollection>,
    preview_scroll: gpui_kit::ScrollHandle,
    _subscriptions: Vec<Subscription>,
}

impl TextEditorView {
    pub fn new(model: Entity<TextEditor>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            editor: None,
            gutter: None,
            preview_scroll: gpui_kit::ScrollHandle::new(),
            _subscriptions: vec![observe],
        }
    }

    /// Created once the document is loaded, from its current (possibly unsaved) text.
    fn editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<EditorState>> {
        if let Some(editor) = &self.editor {
            return Some(editor.clone());
        }
        let EditorDoc::Ready { current, .. } = self.model.read(cx).doc().clone() else {
            return None;
        };
        let language = language_for(self.model.read(cx).path());
        let monaco_language = self.model.read(cx).language();
        let brackets = crate::file_browser_view::bracket_colors(&current, monaco_language);
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language).default_value(current));
        self.gutter = Some(crate::file_browser_view::monaco_gutter(&editor, false, cx));
        let brackets = editor.update(cx, |editor, cx| editor.create_decorations_collection(brackets, cx));
        let model = self.model.clone();
        let changes = cx.subscribe(&editor, move |_, editor, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = editor.read(cx).value().to_string();
                brackets.set(crate::file_browser_view::bracket_colors(&text, monaco_language), cx);
                model.update(cx, |model, cx| model.set_content(text, cx));
            }
        });
        self._subscriptions.push(changes);
        self.editor = Some(editor.clone());
        Some(editor)
    }

    /// Go to definition opened this tab at a line: move there once the editor has a layout.
    fn reveal_line(&mut self, editor: &Entity<EditorState>, window: &mut Window, cx: &mut Context<Self>) {
        if editor.read(cx).line_height().is_none() {
            window.request_animation_frame();
            return;
        }
        let Some(line) = self.model.update(cx, |model, _| model.take_scroll_to_line()) else {
            return;
        };
        let text = editor.read(cx).value().to_string();
        let Some(range) = treeterm_model::file_browser::line_range(&text, line, 1, 0) else {
            return;
        };
        crate::file_browser_view::reveal_in_center(editor, range, cx);
    }

    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.save(cx));
    }

    /// A failed save is an alert, as in Electron.
    fn alert_failed_save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let SaveState::Failed(message) = self.model.read(cx).save_state().clone() else {
            return;
        };
        self.model.update(cx, |model, cx| model.dismiss_save_error(cx));
        let answer = window.prompt(PromptLevel::Critical, &message, None, &["OK"], cx);
        cx.spawn(async move |_, _| match answer.await {
            Ok(_) | Err(_) => {} // dismissed
        })
        .detach();
    }
}

impl Render for TextEditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg_secondary, border, accent) = (c.bg_secondary, c.border, c.accent);
        let frame = v_flex().size_full().min_w_0().overflow_hidden().bg(c.bg_primary);
        if let SaveState::Failed(_) = self.model.read(cx).save_state() {
            cx.defer_in(window, |view, window, cx| view.alert_failed_save(window, cx));
        }
        let model = self.model.read(cx);
        let path = model.path().to_owned();
        let dirty = model.is_dirty();
        let saving = matches!(model.save_state(), SaveState::Saving);
        let (markdown, preview, language) = (model.is_markdown(), model.is_preview(), model.language());
        let doc = model.doc().clone();
        let current = match doc {
            EditorDoc::Loading => return frame.child(ui::viewer_placeholder("Loading...", cx)).into_any_element(),
            EditorDoc::Error(message) => {
                return frame
                    .child(ui::viewer_placeholder(message, cx).text_color(rgb(ui::FILE_ERROR)))
                    .into_any_element();
            }
            EditorDoc::Ready { current, .. } => current,
        };
        let body = if preview {
            crate::file_browser_view::markdown_preview(SharedString::from(format!("md-{path}")), current, &self.preview_scroll, cx)
        } else {
            match self.editor(window, cx) {
                Some(editor) => {
                    self.reveal_line(&editor, window, cx);
                    Editor::new(&editor)
                        .bordered(false)
                        .h_full()
                        .font_family("Menlo")
                        .text_size(px(14.))
                        .into_any_element()
                }
                None => div().into_any_element(),
            }
        };
        let name = path.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(&path).to_owned();
        let toggle = markdown.then(|| {
            ui::editor_button("editor-view-mode", false, cx)
                .child(if preview { "\u{270F} Edit" } else { "\u{1F441} Preview" })
                .tooltip(move |window, cx| {
                    gpui_kit::component::tooltip::Tooltip::new(if preview { "Edit" } else { "Preview" }).build(window, cx)
                })
                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.toggle_view_mode(cx))))
        });
        let save_disabled = !dirty || saving;
        let save = ui::editor_button("save-file", save_disabled, cx)
            .child(if saving { "Saving..." } else { "Save" })
            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Save (Cmd+S)").build(window, cx));
        let save = if save_disabled { save } else { save.on_click(cx.listener(|view, _, window, cx| view.save(&Save, window, cx))) };
        frame
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::save))
            .child(
                h_flex()
                    .flex_shrink_0()
                    .justify_between()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .bg(bg_secondary)
                    .border_b_1()
                    .border_color(border)
                    .child(
                        h_flex()
                            .min_w_0()
                            .overflow_hidden()
                            .text_size(px(ui::BODY))
                            .font_weight(FontWeight::MEDIUM)
                            .child(div().min_w_0().truncate().child(name))
                            // `<span className="file-editor-dirty"> •</span>`: JSX text keeps the escape as written.
                            .when(dirty, |d| {
                                d.child(
                                    div()
                                        .flex_shrink_0()
                                        .whitespace_nowrap()
                                        .text_color(accent)
                                        .font_weight(FontWeight::BOLD)
                                        .child(" \\u2022"),
                                )
                            }),
                    )
                    .child(
                        h_flex()
                            .flex_shrink_0()
                            .gap(px(8.))
                            .children(toggle)
                            .child(save)
                            .child(ui::language_pill(language, cx)),
                    ),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(body))
            .into_any_element()
    }
}
