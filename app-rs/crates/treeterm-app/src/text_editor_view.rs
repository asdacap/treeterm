//! The Editor tab view. Port of the view half of `FileEditor.tsx` (Monaco → GPUI Kit's
//! code editor with tree-sitter highlighting).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Editor, EditorState, InputEvent};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, KeyBinding, Subscription, Window, actions, div, px};
use treeterm_model::text_editor::{EditorDoc, SaveState, TextEditor};

use crate::file_browser_view::language_for;
use crate::theme::colors;

actions!(text_editor, [Save]);

const CONTEXT: &str = "TextEditor";

pub fn bind_keys(cx: &mut gpui_kit::App) {
    let key = if cfg!(target_os = "macos") { "cmd-s" } else { "ctrl-s" };
    cx.bind_keys([KeyBinding::new(key, Save, Some(CONTEXT))]);
}

pub struct TextEditorView {
    model: Entity<TextEditor>,
    editor: Option<Entity<EditorState>>,
    _subscriptions: Vec<Subscription>,
}

impl TextEditorView {
    pub fn new(model: Entity<TextEditor>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            editor: None,
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
        let editor = cx.new(|cx| EditorState::new(window, cx).language(language).default_value(current));
        let model = self.model.clone();
        let changes = cx.subscribe(&editor, move |_, editor, event: &InputEvent, cx| {
            if let InputEvent::Change = event {
                let text = editor.read(cx).value().to_string();
                model.update(cx, |model, cx| model.set_content(text, cx));
            }
        });
        self._subscriptions.push(changes);
        self.editor = Some(editor.clone());
        Some(editor)
    }

    fn save(&mut self, _: &Save, _window: &mut Window, cx: &mut Context<Self>) {
        self.model.update(cx, |model, cx| model.save(cx));
    }
}

impl Render for TextEditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, border, accent) = (c.text_secondary, c.error, c.border, c.accent);
        let model = self.model.read(cx);
        let path = model.path().to_owned();
        let dirty = model.is_dirty();
        let save_state = model.save_state().clone();
        let doc = model.doc().clone();
        let body = match doc {
            EditorDoc::Loading => div().p_4().text_color(secondary).child("Loading…").into_any_element(),
            EditorDoc::Error(message) => div().p_4().text_color(error).child(message).into_any_element(),
            EditorDoc::Ready { .. } => match self.editor(window, cx) {
                Some(editor) => Editor::new(&editor).bordered(false).h_full().into_any_element(),
                None => div().into_any_element(),
            },
        };
        let status = match save_state {
            SaveState::Idle => div().into_any_element(),
            SaveState::Saving => div().text_color(secondary).child("Saving…").into_any_element(),
            SaveState::Failed(message) => div().text_color(error).child(message).into_any_element(),
        };
        v_flex()
            .size_full()
            .key_context(CONTEXT)
            .on_action(cx.listener(Self::save))
            .child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_color(secondary).truncate().child(path))
                    .when(dirty, |d| d.child(div().size(px(7.)).rounded_full().bg(accent)))
                    .child(status)
                    .child(div().flex_1())
                    .child(
                        Button::new("save-file")
                            .xsmall()
                            .ghost()
                            .label("Save")
                            .on_click(cx.listener(|view, _, window, cx| view.save(&Save, window, cx))),
                    ),
            )
            .child(div().flex_1().min_h_0().child(body))
    }
}
