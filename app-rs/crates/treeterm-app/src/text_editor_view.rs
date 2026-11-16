//! The Editor tab view. Port of the view half of `FileEditor.tsx` (Monaco → GPUI Kit's
//! code editor with tree-sitter highlighting), styled as `.file-editor*` in `index.css`: a
//! header with the file name and its unsaved mark, the markdown Edit/Preview toggle, Save
//! and the language, over the editor or the markdown preview.
//!
//! As in Monaco: F12, Cmd/Ctrl+click and the context menu go to the definition of the word
//! under the cursor (holding Cmd/Ctrl over a word with one underlines it); Cmd/Ctrl+F2
//! puts a cursor on every occurrence of the word or selection (Change All Occurrences).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{Copy as CopyText, Cut, Editor, EditorState, InputEvent, Paste, Replace};
use gpui_kit::component::v_flex;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FontWeight, KeyBinding, MouseButton, MouseUpEvent, PromptLevel, SharedString, Subscription,
    Window, actions, div, px, rgb,
};
use treeterm_model::text_editor::{EditorDoc, SaveState, TextEditor};

use crate::file_browser_view::{GoToDefinition, PointerOver, language_for};
use crate::theme::colors;
use crate::ui;

actions!(text_editor, [Save, ChangeAllOccurrences]);

const CONTEXT: &str = "TextEditor";

/// Cmd/Ctrl+S saves. Monaco's replace is Cmd+Alt+F on macOS (GPUI Kit's Cmd+Shift+F
/// stays); elsewhere it is Ctrl+H, as in the Kit. F12 goes to the definition and
/// Cmd/Ctrl+F2 changes all occurrences, as in Monaco.
pub fn bind_keys(cx: &mut gpui_kit::App) {
    cx.bind_keys([KeyBinding::new("f12", GoToDefinition, Some(CONTEXT))]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-s", Save, Some(CONTEXT)),
            KeyBinding::new("cmd-alt-f", Replace, Some(CONTEXT)),
            KeyBinding::new("cmd-f2", ChangeAllOccurrences, Some(CONTEXT)),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-s", Save, Some(CONTEXT)),
            KeyBinding::new("ctrl-f2", ChangeAllOccurrences, Some(CONTEXT)),
        ]);
    }
}

pub struct TextEditorView {
    model: Entity<TextEditor>,
    editor: Option<Entity<EditorState>>,
    /// The editor's glyph margin (Monaco's gutter width).
    gutter: Option<gpui_kit::component::input::GutterDecorationCollection>,
    /// The pointer is over the editor ([`crate::file_browser_view::pointer_over`]).
    pointer_over: PointerOver,
    /// The last press went to an underlined definition ([`crate::editor_hooks`]).
    definition_clicked: Rc<Cell<bool>>,
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
            pointer_over: PointerOver::default(),
            definition_clicked: Rc::new(Cell::new(false)),
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
        let editor = cx.new(|cx| crate::file_browser_view::code_editor(language, current, window, cx));
        self.gutter = Some(crate::file_browser_view::monaco_gutter(&editor, false, &self.pointer_over, cx));
        let brackets = editor.update(cx, |editor, cx| editor.create_decorations_collection(brackets, cx));
        let model = self.model.clone();
        let changes = cx.subscribe(&editor, move |_, editor, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = editor.read(cx).value().to_string();
                brackets.set(crate::file_browser_view::bracket_colors(&text, monaco_language), cx);
                editor.update(cx, |editor, cx| crate::file_browser_view::fit_glyph_strip(editor, false, cx));
                model.update(cx, |model, cx| model.set_content(text, cx));
            }
        });
        self._subscriptions.push(changes);
        let probe = self.model.read(cx).definition_probe();
        let model = self.model.downgrade();
        let go = move |offset: usize, cx: &mut App| {
            if let Some(model) = model.upgrade() {
                model.update(cx, |m, cx| m.go_to_definition(offset, cx));
            }
        };
        crate::editor_hooks::install_definitions(&editor, probe, self.definition_clicked.clone(), go, cx);
        self.editor = Some(editor.clone());
        Some(editor)
    }

    /// Go to definition opened this tab at a line, or found the definition in this file:
    /// move there once the editor has a layout.
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

    fn on_go_to_definition(&mut self, _: &GoToDefinition, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let offset = editor.read(cx).cursor();
        self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
    }

    /// Cmd/Ctrl+click goes to the definition of the clicked word (the press put the cursor
    /// on it), unless the press went to an underlined definition, which already went there.
    fn on_editor_mouse_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let definition_clicked = self.definition_clicked.replace(false);
        let Some(editor) = &self.editor else {
            return;
        };
        let (selected, offset) = {
            let editor = editor.read(cx);
            (editor.selected_range(), editor.cursor())
        };
        if !selected.is_empty() || !event.modifiers.secondary(cx) || definition_clicked {
            return;
        }
        self.model.update(cx, |m, cx| m.go_to_definition(offset, cx));
    }

    /// Change All Occurrences: a cursor on each occurrence, the one at the selection
    /// staying the primary one, and the view where it was.
    fn on_change_all(&mut self, _: &ChangeAllOccurrences, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let (occurrences, scroll) = {
            let editor = editor.read(cx);
            (treeterm_model::words::all_occurrences(&editor.value(), editor.selected_range()), editor.scroll_offset())
        };
        let Some((primary, others)) = occurrences.split_first() else {
            return;
        };
        editor.update(cx, |editor, cx| {
            editor.set_selected_range(primary.clone(), cx);
            for range in others {
                editor.add_selection(range.clone(), cx);
            }
            editor.set_scroll_offset(scroll, cx);
        });
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
        let (has_definitions, definition) = (model.has_definitions(), model.definition().clone());
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
                    let overlay = crate::file_browser_view::definition_overlay(
                        definition,
                        |view: &mut Self, index, cx| view.model.update(cx, |m, cx| m.choose_definition(index, cx)),
                        |view: &mut Self, cx| view.model.update(cx, |m, cx| m.dismiss_definition(cx)),
                        cx,
                    );
                    // Monaco's menu for an editable file: Go to Definition where the language
                    // has it, Change All Occurrences, then the clipboard.
                    let editor_element = Editor::new(&editor)
                        .bordered(false)
                        .h_full()
                        .font_family("Menlo")
                        .text_size(px(14.))
                        .context_menu(move |menu, _, _| {
                            let menu = if has_definitions {
                                menu.menu("Go to Definition", Box::new(GoToDefinition)).separator()
                            } else {
                                menu
                            };
                            menu.menu("Change All Occurrences", Box::new(ChangeAllOccurrences))
                                .separator()
                                .menu("Cut", Box::new(Cut))
                                .menu("Copy", Box::new(CopyText))
                                .menu("Paste", Box::new(Paste))
                        });
                    v_flex()
                        .size_full()
                        .children(overlay)
                        .child(
                            crate::editor_hooks::drop_underline_on_release(
                                crate::file_browser_view::pointer_over(div().id("text-editor"), &self.pointer_over),
                                &editor,
                            )
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_editor_mouse_up))
                            .child(editor_element)
                            .child(crate::file_browser_view::current_line_outline(&editor, false)),
                        )
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
            .on_action(cx.listener(Self::on_go_to_definition))
            .on_action(cx.listener(Self::on_change_all))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A code editor in the Editor tab's key context.
    struct Host {
        editor: Entity<EditorState>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().key_context(CONTEXT).w(px(400.)).h(px(300.)).child(Editor::new(&self.editor).h_full())
        }
    }

    #[gpui_kit::test]
    fn monacos_replace_key_opens_find_and_replace(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let (host, cx) = cx.add_window_view(|window, cx| Host {
            editor: cx.new(|cx| EditorState::new(window, cx).language("rust").default_value("fn main() {}\n")),
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus(window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-alt-f" } else { "ctrl-h" });
        let (open, replace) = cx.update(|_, cx| {
            let search = editor.read(cx).search_session();
            (search.open, search.replace_mode)
        });
        assert!(open && replace, "open {open}, replace {replace}");
    }
}
