//! The Editor tab view. Port of the view half of `FileEditor.tsx` (Monaco → GPUI Kit's
//! code editor with tree-sitter highlighting), styled as `.file-editor*` in `index.css`: a
//! header with the file name and its unsaved mark, the markdown Edit/Preview toggle, Save
//! and the language, over the editor or the markdown preview.
//!
//! As in Monaco: F12, Cmd/Ctrl+click and the context menu go to the definition of the word
//! under the cursor (holding Cmd/Ctrl over a word with one underlines it); Cmd/Ctrl+F2
//! puts a cursor on every occurrence of the word or selection (Change All Occurrences);
//! typing suggests CSS property names, HTML elements, attributes, attribute values and
//! character entities, or JSON keys and values where the language's worker offers them (a
//! picked HTML attribute asks for its values), else the words of the document and of the
//! other open files of its language that have the typed letters in order, the first at a word
//! start or camelCase hump, but nothing while typing in a comment or string (Ctrl+Space
//! asks); Enter or Tab takes the selected one. The language workers' problems are marked
//! again as the text changes, CSS colours are painted and links underlined; Cmd/Ctrl+click
//! opens a link ([`crate::editor_hooks`]).
//!
//! Text another window or Electron writes to the tab replaces the editor's, as Monaco's
//! `value` prop does: undoably, keeping the cursor and the scroll position where they were.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{
    Copy as CopyText, Cut, Editor, EditorState, Enter, Escape, IndentInline, InputEvent, Paste, Replace,
    ShowCompletions,
};
use gpui_kit::component::v_flex;
use gpui_kit::component::h_flex;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Context, Entity, FontWeight, KeyBinding, MouseButton, MouseUpEvent, PromptLevel, SharedString, Subscription,
    Window, actions, div, px, rgb,
};
use treeterm_model::code_intel::{self, FormatScope};
use treeterm_model::indentation::{Indentation, guess_indentation};
use treeterm_model::text_editor::{EditorDoc, SaveState, ScrollTarget, TextEditor};

use crate::editor_hooks::{Follow, SuggestSession};
use crate::file_browser_view::{GoToDefinition, PeekDefinition, PointerOver, language_for};
use crate::theme::colors;
use crate::ui;

actions!(text_editor, [Save, ChangeAllOccurrences, FormatDocument, FormatSelection]);

const CONTEXT: &str = "TextEditor";

/// Cmd/Ctrl+S saves. Monaco's replace is Cmd+Alt+F on macOS (GPUI Kit's Cmd+Shift+F
/// stays); elsewhere it is Ctrl+H, as in the Kit. F12 goes to the definition
/// ([`crate::file_browser_view::definition_keys`]), Alt+F12 peeks at it, Cmd/Ctrl+F2 changes
/// all occurrences, Shift+Alt+F (Ctrl+Shift+I on Linux) formats the document and Cmd/Ctrl+K
/// Cmd/Ctrl+F the selection, and Ctrl+Space or Cmd/Ctrl+I (Alt+Escape on macOS) ask for
/// suggestions, as in Monaco.
pub fn bind_keys(cx: &mut gpui_kit::App) {
    cx.bind_keys(crate::file_browser_view::definition_keys(CONTEXT));
    cx.bind_keys([crate::file_browser_view::peek_key(CONTEXT)]);
    cx.bind_keys([KeyBinding::new("ctrl-space", ShowCompletions, Some(CONTEXT))]);
    let format_document = if cfg!(target_os = "linux") { "ctrl-shift-i" } else { "shift-alt-f" };
    cx.bind_keys([KeyBinding::new(format_document, FormatDocument, Some(CONTEXT))]);
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-s", Save, Some(CONTEXT)),
            KeyBinding::new("cmd-alt-f", Replace, Some(CONTEXT)),
            KeyBinding::new("cmd-f2", ChangeAllOccurrences, Some(CONTEXT)),
            KeyBinding::new("cmd-k cmd-f", FormatSelection, Some(CONTEXT)),
            KeyBinding::new("cmd-i", ShowCompletions, Some(CONTEXT)),
            KeyBinding::new("alt-escape", ShowCompletions, Some(CONTEXT)),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-s", Save, Some(CONTEXT)),
            KeyBinding::new("ctrl-f2", ChangeAllOccurrences, Some(CONTEXT)),
            KeyBinding::new("ctrl-k ctrl-f", FormatSelection, Some(CONTEXT)),
            KeyBinding::new("ctrl-i", ShowCompletions, Some(CONTEXT)),
        ]);
    }
}

pub struct TextEditorView {
    model: Entity<TextEditor>,
    editor: Option<Entity<EditorState>>,
    /// The model's [`TextEditor::external_revision`] the editor's text is from.
    revision: u64,
    /// The editor's glyph margin (Monaco's gutter width).
    gutter: Option<gpui_kit::component::input::GutterDecorationCollection>,
    /// The pointer is over the editor ([`crate::file_browser_view::pointer_over`]).
    pointer_over: PointerOver,
    /// The current press went to an underlined definition ([`crate::editor_hooks::definition_frame`]).
    definition_clicked: Rc<Cell<bool>>,
    /// The language worker's problems in the file, marked again at every change.
    problems: crate::editor_hooks::ProblemMarks,
    /// Whether suggestions show or are on their way, and where they were asked for.
    suggestions: SuggestSession,
    /// What the editor indents with, read from the file when it opened (Monaco's model
    /// options), which formatting indents with too.
    indentation: Indentation,
    preview_scroll: gpui_kit::ScrollHandle,
    /// The tab is on screen ([`Self::set_visible`]).
    visible: bool,
    _subscriptions: Vec<Subscription>,
}

impl TextEditorView {
    pub fn new(model: Entity<TextEditor>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        let problems = crate::editor_hooks::ProblemMarks::new(model.read(cx).language(), model.read(cx).path().to_owned());
        Self {
            model,
            editor: None,
            revision: 0,
            gutter: None,
            pointer_over: PointerOver::default(),
            definition_clicked: Rc::new(Cell::new(false)),
            problems,
            suggestions: SuggestSession::default(),
            indentation: Indentation::MONACO_DEFAULT,
            preview_scroll: gpui_kit::ScrollHandle::new(),
            visible: false,
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
        self.revision = self.model.read(cx).external_revision();
        let language = language_for(self.model.read(cx).path());
        let monaco_language = self.model.read(cx).language();
        let brackets = crate::file_browser_view::bracket_colors(&current, monaco_language);
        let links = crate::editor_hooks::link_underlines(&current);
        self.indentation = guess_indentation(&current, Indentation::MONACO_DEFAULT);
        let editor = cx.new(|cx| crate::file_browser_view::code_editor(language, current, window, cx));
        self.gutter = Some(crate::file_browser_view::monaco_gutter(&editor, false, &self.pointer_over, cx));
        let brackets = editor.update(cx, |editor, cx| editor.create_decorations_collection(brackets, cx));
        let links = editor.update(cx, |editor, cx| editor.create_decorations_collection(links, cx));
        self.problems.mark(&editor, cx);
        crate::editor_hooks::install_colors(&editor, monaco_language, cx);
        let model = self.model.clone();
        let changes = cx.subscribe(&editor, move |view, editor, event: &InputEvent, cx| match event {
            InputEvent::Change => {
                let text = editor.read(cx).value().to_string();
                brackets.set(crate::file_browser_view::bracket_colors(&text, monaco_language), cx);
                links.set(crate::editor_hooks::link_underlines(&text), cx);
                view.problems.mark(&editor, cx);
                editor.update(cx, |editor, cx| crate::file_browser_view::fit_glyph_strip(editor, false, cx));
                model.update(cx, |model, cx| model.set_content(text, cx));
            }
            // Monaco's suggest model is cancelled when the editor loses focus.
            InputEvent::Blur => view.suggestions.end(),
            InputEvent::PressEnter { .. } | InputEvent::Focus => {}
        });
        self._subscriptions.push(changes);
        let mut highlights = crate::editor_hooks::CursorHighlights::new(&editor, monaco_language, cx);
        self._subscriptions.push(cx.observe(&editor, move |_, editor, cx| highlights.follow(&editor, cx)));
        let probe = self.model.read(cx).definition_probe();
        let model = self.model.downgrade();
        let go = move |follow: Follow, cx: &mut App| {
            if let Some(model) = model.upgrade() {
                model.update(cx, |m, cx| match follow {
                    Follow::Definition(offset) => m.go_to_definition(offset, cx),
                    Follow::Link(url) => m.open_link(&url, cx),
                });
            }
        };
        let clicked = self.definition_clicked.clone();
        crate::editor_hooks::install_definitions(&editor, monaco_language, probe, clicked, go, cx);
        let model = self.model.downgrade();
        let others: crate::editor_hooks::OtherTexts = Rc::new(move |cx: &App| match model.upgrade() {
            Some(model) => model.read(cx).other_open_texts(model.entity_id(), cx),
            None => Vec::new(),
        });
        let path = self.model.read(cx).path().to_owned();
        let suggestions = self.suggestions.clone();
        crate::editor_hooks::install_suggestions(&editor, &path, monaco_language, others, suggestions, cx);
        self.editor = Some(editor.clone());
        Some(editor)
    }

    /// Another writer's text replaced the model's: the editor takes it as one undoable edit
    /// (whose change event finds the model's text already equal), the cursor and the view
    /// staying where they were.
    fn follow_model_text(&mut self, editor: &Entity<EditorState>, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.read(cx);
        if model.external_revision() == self.revision {
            return;
        }
        self.revision = model.external_revision();
        let EditorDoc::Ready { current, .. } = model.doc() else {
            return;
        };
        if *editor.read(cx).text() == *current {
            return;
        }
        let current = current.clone();
        editor.update(cx, |editor, cx| {
            let (selected, scroll) = (editor.selected_range(), editor.scroll_offset());
            editor.replace_all(current, window, cx);
            let len = editor.text().len();
            editor.set_selected_range(selected.start.min(len)..selected.end.min(len), cx);
            editor.set_scroll_offset(scroll, cx);
        });
    }

    /// Go to definition opened this tab at a line, or found the definition in this file, or
    /// the tab was left scrolled: move there once the editor has a layout.
    fn reveal_line(&mut self, editor: &Entity<EditorState>, window: &mut Window, cx: &mut Context<Self>) {
        if editor.read(cx).line_height().is_none() {
            window.request_animation_frame();
            return;
        }
        match self.model.update(cx, |model, cx| model.take_scroll_target(cx)) {
            None => {}
            Some(ScrollTarget::Line(line)) => {
                let text = editor.read(cx).value().to_string();
                if let Some(range) = treeterm_model::file_browser::line_range(&text, line, 1, 0) {
                    crate::file_browser_view::reveal_in_center(editor, range, cx);
                }
            }
            Some(ScrollTarget::Top(top)) => editor.update(cx, |editor, cx| {
                editor.set_scroll_offset(gpui_kit::point(editor.scroll_offset().x, px(-top)), cx);
            }),
        }
    }

    /// Whether the tab is on screen. Hidden, it saves where the editor was scrolled to, as
    /// `FileEditor` does on unmount; the cached editor keeps that offset, in the markdown
    /// preview too.
    pub fn set_visible(&mut self, visible: bool, cx: &mut Context<Self>) {
        if self.visible
            && !visible
            && let Some(editor) = &self.editor
        {
            let top = f32::from(-editor.read(cx).scroll_offset().y);
            self.model.update(cx, |model, cx| model.save_scroll_top(top, cx));
        }
        self.visible = visible;
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

    fn on_peek_definition(&mut self, _: &PeekDefinition, _: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        let offset = editor.read(cx).cursor();
        self.model.update(cx, |m, cx| m.peek_definition(offset, cx));
    }

    fn on_format_document(&mut self, _: &FormatDocument, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.editor {
            let language = self.model.read(cx).language();
            format(editor, language, self.indentation, |_| FormatScope::Document, window, cx);
        }
    }

    fn on_format_selection(&mut self, _: &FormatSelection, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(editor) = &self.editor {
            let language = self.model.read(cx).language();
            format(editor, language, self.indentation, FormatScope::Selection, window, cx);
        }
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

    /// Tab takes the selected suggestion while suggestions show, as Enter does
    /// (`acceptSelectedSuggestion`); otherwise it indents.
    fn on_tab(&mut self, _: &IndentInline, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = &self.editor else {
            return;
        };
        if accept_suggestion(editor, window, cx) {
            cx.stop_propagation();
        }
    }

    /// Escape closes the suggestions, and a character typed where they were then starts anew
    /// (`hideSuggestWidget` cancels Monaco's suggest model).
    fn on_escape(&mut self, _: &Escape, _: &mut Window, _: &mut Context<Self>) {
        self.suggestions.end();
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

/// Format `editor`'s text, a file of Monaco's `language` it indents with `indentation`: the
/// document, or the part `scope` takes of the selection, as the language's worker does, as
/// one undoable edit. The selection moves with the text around it, and the view stays where
/// it was.
fn format(
    editor: &Entity<EditorState>,
    language: &str,
    indentation: Indentation,
    scope: impl FnOnce(std::ops::Range<usize>) -> FormatScope,
    window: &mut Window,
    cx: &mut App,
) {
    let (text, selection, scroll) = {
        let editor = editor.read(cx);
        (editor.value().to_string(), editor.selected_range(), editor.scroll_offset())
    };
    let edits = code_intel::format(language, &text, scope(selection.clone()), indentation);
    if edits.is_empty() {
        return;
    }
    let (formatted, selection) = code_intel::apply_edits(&text, &edits, selection);
    editor.update(cx, |editor, cx| {
        editor.replace_all(formatted, window, cx);
        editor.set_selected_range(selection, cx);
        editor.set_scroll_offset(scroll, cx);
    });
}

/// Take the suggestion selected in `editor`'s menu, if it shows one: whether it did.
fn accept_suggestion(editor: &Entity<EditorState>, window: &mut Window, cx: &mut App) -> bool {
    if !editor.read(cx).completion_menu_state().open {
        return false;
    }
    let enter = Box::new(Enter { secondary: false, shift: false });
    editor.update(cx, |editor, cx| editor.route_overlay_action(enter, window, cx))
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
        let formats = code_intel::formats(language);
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
            crate::file_browser_view::markdown_preview(
                SharedString::from(format!("md-{path}")),
                current,
                &self.preview_scroll,
                window,
                cx,
            )
        } else {
            match self.editor(window, cx) {
                Some(editor) => {
                    self.follow_model_text(&editor, window, cx);
                    self.reveal_line(&editor, window, cx);
                    let overlay = crate::file_browser_view::definition_overlay(
                        definition,
                        |view: &mut Self, index, cx| view.model.update(cx, |m, cx| m.choose_definition(index, cx)),
                        |view: &mut Self, cx| view.model.update(cx, |m, cx| m.dismiss_definition(cx)),
                        cx,
                    );
                    // Monaco's menu for an editable file: Go to Definition and the Peek submenu
                    // where the language has them, Change All Occurrences, Format Document
                    // and (with a selection) Format Selection where it has a formatter, then
                    // the clipboard.
                    let selecting = editor.downgrade();
                    let editor_element = Editor::new(&editor)
                        .bordered(false)
                        .h_full()
                        .font_family("Menlo")
                        .text_size(px(14.))
                        .context_menu(move |menu, _, cx| {
                            let menu = if has_definitions {
                                let menu = menu.menu("Go to Definition", Box::new(GoToDefinition));
                                crate::file_browser_view::peek_menu(menu, true).separator()
                            } else {
                                menu
                            };
                            let menu = menu.menu("Change All Occurrences", Box::new(ChangeAllOccurrences));
                            let selected = selecting.upgrade().is_some_and(|editor| !editor.read(cx).selected_range().is_empty());
                            let menu = if formats { menu.menu("Format Document", Box::new(FormatDocument)) } else { menu };
                            let menu = if formats && selected { menu.menu("Format Selection", Box::new(FormatSelection)) } else { menu };
                            menu.separator()
                                .menu("Cut", Box::new(Cut))
                                .menu("Copy", Box::new(CopyText))
                                .menu("Paste", Box::new(Paste))
                        });
                    v_flex()
                        .size_full()
                        .children(overlay)
                        .child(
                            crate::editor_hooks::definition_frame(
                                crate::file_browser_view::pointer_over(div().id("text-editor"), &self.pointer_over),
                                &editor,
                                &self.definition_clicked,
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
            .on_action(cx.listener(Self::on_peek_definition))
            .on_action(cx.listener(Self::on_change_all))
            .on_action(cx.listener(Self::on_format_document))
            .on_action(cx.listener(Self::on_format_selection))
            .capture_action(cx.listener(Self::on_tab))
            .capture_action(cx.listener(Self::on_escape))
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

    /// A code editor in the Editor tab's key context, Escape ending its suggest session as the
    /// tab's does.
    struct Host {
        editor: Entity<EditorState>,
        suggestions: SuggestSession,
    }

    impl Host {
        fn new(editor: Entity<EditorState>) -> Self {
            Self { editor, suggestions: SuggestSession::default() }
        }
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(CONTEXT)
                .capture_action(cx.listener(|host, _: &Escape, _, _| host.suggestions.end()))
                .w(px(400.))
                .h(px(300.))
                .child(Editor::new(&self.editor).h_full())
        }
    }

    fn no_others() -> crate::editor_hooks::OtherTexts {
        Rc::new(|_| Vec::new())
    }

    /// Give the host's editor, a file at `path` of Monaco's `language`, Monaco's suggestions
    /// with the host's suggest session; `others` are the other open files' texts.
    fn install(
        host: &Entity<Host>,
        path: &str,
        language: &'static str,
        others: crate::editor_hooks::OtherTexts,
        cx: &mut App,
    ) {
        let (editor, suggestions) = (host.read(cx).editor.clone(), host.read(cx).suggestions.clone());
        crate::editor_hooks::install_suggestions(&editor, path, language, others, suggestions, cx);
    }

    /// A code editor in the Editor tab's key context, Tab taking a suggestion as the tab's does.
    struct TabHost {
        editor: Entity<EditorState>,
    }

    impl Render for TabHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(CONTEXT)
                .capture_action(cx.listener(|host, _: &IndentInline, window, cx| {
                    if accept_suggestion(&host.editor, window, cx) {
                        cx.stop_propagation();
                    }
                }))
                .w(px(400.))
                .h(px(300.))
                .child(Editor::new(&self.editor).h_full())
        }
    }

    /// The suggestions `editor` shows, if any.
    fn shown(editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext) -> Option<Vec<String>> {
        cx.update(|_, cx| {
            let menu = editor.read(cx).completion_menu_state();
            menu.open.then(|| menu.items.iter().map(|item| item.label.clone()).collect())
        })
    }

    #[gpui_kit::test]
    fn tab_takes_the_selected_suggestion_and_otherwise_indents(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "fn food() {}
";
        let (host, cx) = cx.add_window_view(|window, cx| TabHost {
            editor: cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)),
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| {
            let suggestions = SuggestSession::default();
            crate::editor_hooks::install_suggestions(&editor, "/w/a.rs", "rust", no_others(), suggestions, cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f o");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["food".to_owned()]));
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).value().to_string()), "fn food() {}
food");
        assert_eq!(shown(&editor, cx), None);
        // With no suggestions showing, Tab indents.
        cx.simulate_keystrokes("tab");
        cx.run_until_parked();
        assert_ne!(cx.update(|_, cx| editor.read(cx).value().to_string()), "fn food() {}
food");
    }

    #[gpui_kit::test]
    fn typing_in_a_comment_suggests_nothing_but_asking_does(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "fn food() {}
// ";
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)))
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        let others: crate::editor_hooks::OtherTexts = Rc::new(|_| vec!["fn fond() {}".to_owned()]);
        cx.update(|window, cx| {
            install(&host, "/w/a.rs", "rust", others, cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f o");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), None);
        // Ctrl+Space asks: the words of this file and the other open ones.
        cx.simulate_keystrokes("ctrl-space");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["fond".to_owned(), "food".to_owned()]));
        // Typing goes on refining what shows, in the comment too.
        cx.simulate_keystrokes("n");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["fond".to_owned()]));
    }

    /// A code editor in the Editor tab's key context, counting go to definition and peek.
    struct DefinitionHost {
        editor: Entity<EditorState>,
        gone: usize,
        peeked: usize,
    }

    impl Render for DefinitionHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            div()
                .key_context(CONTEXT)
                .on_action(cx.listener(|host, _: &GoToDefinition, _, _| host.gone += 1))
                .on_action(cx.listener(|host, _: &PeekDefinition, _, _| host.peeked += 1))
                .w(px(400.))
                .h(px(300.))
                .child(Editor::new(&self.editor).h_full())
        }
    }

    #[gpui_kit::test]
    fn monacos_go_to_definition_and_open_to_the_side_keys_go_to_the_definition(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "fn main() {}\n";
        let (host, cx) = cx.add_window_view(|window, cx| DefinitionHost {
            editor: cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)),
            gone: 0,
            peeked: 0,
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| editor.update(cx, |editor, cx| editor.focus(window, cx)));
        cx.run_until_parked();
        let secondary = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        for keys in ["f12".to_owned(), format!("{secondary}-f12"), format!("{secondary}-k f12"), format!("{secondary}-k {secondary}-f12")]
        {
            cx.simulate_keystrokes(&keys);
        }
        assert_eq!(cx.update(|_, cx| host.read(cx).gone), 4);
        // Peek Definition: Alt+F12 (Ctrl+Shift+F10 on Linux).
        cx.simulate_keystrokes(if cfg!(target_os = "linux") { "ctrl-shift-f10" } else { "alt-f12" });
        assert_eq!(cx.update(|_, cx| host.read(cx).peeked), 1);
        assert_eq!(cx.update(|_, cx| editor.read(cx).value().to_string()), text);
    }

    /// A code editor of Monaco's `language` in the Editor tab's key context, formatting as the
    /// tab does.
    struct FormatHost {
        editor: Entity<EditorState>,
        language: &'static str,
    }

    impl Render for FormatHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let indentation = Indentation::MONACO_DEFAULT;
            div()
                .key_context(CONTEXT)
                .on_action(cx.listener(move |host, _: &FormatDocument, window, cx| {
                    format(&host.editor, host.language, indentation, |_| FormatScope::Document, window, cx);
                }))
                .on_action(cx.listener(move |host, _: &FormatSelection, window, cx| {
                    format(&host.editor, host.language, indentation, FormatScope::Selection, window, cx);
                }))
                .w(px(400.))
                .h(px(300.))
                .child(Editor::new(&self.editor).h_full())
        }
    }

    #[gpui_kit::test]
    fn monacos_format_keys_format_a_json_document_or_the_selected_lines_undoably(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "{\"a\":1,\n\"b\":[2,3]}";
        let (host, cx) = cx.add_window_view(|window, cx| FormatHost {
            editor: cx.new(|cx| EditorState::new(window, cx).language("json").default_value(text)),
            language: "json",
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        let b = text.find("\"b\"").unwrap();
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(b..b, cx);
            });
        });
        cx.run_until_parked();
        // Cmd/Ctrl+K Cmd/Ctrl+F with nothing selected: the cursor's line.
        let secondary = if cfg!(target_os = "macos") { "cmd" } else { "ctrl" };
        cx.simulate_keystrokes(&format!("{secondary}-k {secondary}-f"));
        let (now, cursor) = text_and_cursor(&editor, cx);
        assert_eq!(now, "{\"a\":1,\n\"b\": [\n    2,\n    3\n]\n}");
        assert_eq!(cursor, b..b);
        // Shift+Alt+F (Ctrl+Shift+I on Linux): the whole document, the cursor moving with it.
        cx.simulate_keystrokes(if cfg!(target_os = "linux") { "ctrl-shift-i" } else { "shift-alt-f" });
        let (now, cursor) = text_and_cursor(&editor, cx);
        assert_eq!(now, "{\n    \"a\": 1,\n    \"b\": [\n        2,\n        3\n    ]\n}");
        let b = now.find("\"b\"").unwrap();
        assert_eq!(cursor, b..b);
        // Undone in one step.
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-z" } else { "ctrl-z" });
        assert_eq!(text_and_cursor(&editor, cx).0, "{\"a\":1,\n\"b\": [\n    2,\n    3\n]\n}");
        // A language without a formatter is left as it is.
        host.update(cx, |host, _| host.language = "rust");
        cx.simulate_keystrokes(if cfg!(target_os = "linux") { "ctrl-shift-i" } else { "shift-alt-f" });
        assert_eq!(text_and_cursor(&editor, cx).0, "{\"a\":1,\n\"b\": [\n    2,\n    3\n]\n}");
    }

    #[gpui_kit::test]
    fn monacos_replace_key_opens_find_and_replace(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language("rust").default_value("fn main() {}\n")))
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

    #[gpui_kit::test]
    fn typing_a_word_suggests_the_documents_words_and_enter_takes_one(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "fn food() {}\n";
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)))
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| {
            install(&host, "/w/a.rs", "rust", no_others(), cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f o");
        cx.run_until_parked();
        let labels = cx.update(|_, cx| {
            let menu = editor.read(cx).completion_menu_state();
            menu.open.then(|| menu.items.iter().map(|item| item.label.clone()).collect::<Vec<_>>())
        });
        assert_eq!(labels, Some(vec!["food".to_owned()]));
        // Enter takes the word in place of what was typed, and adds no line.
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).value().to_string()), "fn food() {}\nfood");
        // Typing past the word closes the suggestions.
        cx.simulate_keystrokes("space f");
        cx.run_until_parked();
        assert!(cx.update(|_, cx| editor.read(cx).completion_menu_state().open));
        cx.simulate_keystrokes("x");
        cx.run_until_parked();
        assert!(!cx.update(|_, cx| editor.read(cx).completion_menu_state().open));
    }

    #[gpui_kit::test]
    fn typed_letters_in_order_suggest_words_best_first_with_them_highlighted(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let text = "let fooBar = foobar;\n";
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)))
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| {
            install(&host, "/w/a.rs", "rust", no_others(), cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("f b");
        cx.run_until_parked();
        let shown = cx.update(|_, cx| {
            let editor = editor.read(cx);
            let menu = editor.completion_menu_state();
            let highlight = editor.lsp().completion_highlighter();
            menu.items.iter().map(|item| (item.label.clone(), highlight(&menu.query, item))).collect::<Vec<_>>()
        });
        // `b` at a hump matches better than inside a word.
        assert_eq!(shown, [("fooBar".to_owned(), vec![0..1, 3..4]), ("foobar".to_owned(), vec![0..1, 3..4])]);
    }

    /// An Editor tab's editor on `text` of Monaco's `language` (in a file at `path`), with
    /// Monaco's suggestions, focused with the cursor at the end.
    fn suggesting_editor<'a>(
        cx: &'a mut gpui_kit::TestAppContext,
        path: &str,
        language: &'static str,
        text: &str,
    ) -> (Entity<EditorState>, &'a mut gpui_kit::VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let kit_language = crate::file_browser_view::language_for(path);
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language(kit_language).default_value(text)))
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| {
            install(&host, path, language, no_others(), cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        (editor, cx)
    }

    /// The editor's text and where its cursor (or selection) is.
    fn text_and_cursor(editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext) -> (String, std::ops::Range<usize>) {
        cx.update(|_, cx| (editor.read(cx).value().to_string(), editor.read(cx).selected_range()))
    }

    #[gpui_kit::test]
    fn json_offers_the_keys_and_values_used_elsewhere_and_an_empty_object_takes_the_cursor_inside(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let text = "[{\"name\": {\"a\": 1}}, {\"name\": ";
        let (editor, cx) = suggesting_editor(cx, "/w/a.json", "json", text);
        cx.simulate_keystrokes("ctrl-space");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["{}".to_owned()]));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let at = text.len() + 1;
        assert_eq!(text_and_cursor(&editor, cx), (format!("{text}{{}}"), at..at));
        // A key: those of the similar object, typed after a quote, which asks for them.
        cx.simulate_keystrokes("\"");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["a".to_owned()]));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let (now, _) = text_and_cursor(&editor, cx);
        assert!(now.starts_with(&format!("{text}{{\"a\"")), "{now}");
    }

    #[gpui_kit::test]
    fn letters_typed_at_once_in_a_json_key_go_on_refining_its_suggestions(cx: &mut gpui_kit::TestAppContext) {
        let text = "[{\"alpha\": 1, \"beta\": 2, \"bravo\": 3}, {";
        let (editor, cx) = suggesting_editor(cx, "/w/a.json", "json", text);
        // The quote asks for the keys of the similar object.
        cx.simulate_keystrokes("\"");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["alpha".to_owned(), "beta".to_owned(), "bravo".to_owned()]));
        // Two letters, the second typed before the first one's suggestions came back.
        cx.simulate_keystrokes("b r");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["bravo".to_owned()]));
    }

    #[gpui_kit::test]
    fn letters_typed_at_once_after_asking_in_a_comment_go_on_refining_until_the_word_ends(
        cx: &mut gpui_kit::TestAppContext,
    ) {
        let text = "fn food() {}\nfn fond() {}\n// ";
        let (editor, cx) = suggesting_editor(cx, "/w/a.rs", "rust", text);
        cx.simulate_keystrokes("ctrl-space");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["fn".to_owned(), "fond".to_owned(), "food".to_owned()]));
        cx.simulate_keystrokes("f o");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["fond".to_owned(), "food".to_owned()]));
        // A space ends the word, and with it the suggestions: a letter after it is quiet again.
        cx.simulate_keystrokes("space f");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), None);
        // So does Escape: a letter typed where they were asked for is quiet too.
        cx.simulate_keystrokes("ctrl-space");
        cx.run_until_parked();
        assert!(shown(&editor, cx).is_some());
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), None);
        cx.simulate_keystrokes("o");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), None);
    }

    #[gpui_kit::test]
    fn an_html_attribute_is_completed_with_the_cursor_between_its_quotes(cx: &mut gpui_kit::TestAppContext) {
        let text = "<a hre";
        let (editor, cx) = suggesting_editor(cx, "/w/a.html", "html", text);
        cx.simulate_keystrokes("f");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx).map(|labels| labels[0].clone()), Some("href".to_owned()));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(text_and_cursor(&editor, cx), ("<a href=\"\"".to_owned(), 9..9));
    }

    #[gpui_kit::test]
    fn a_picked_attribute_with_values_offers_them_between_its_quotes(cx: &mut gpui_kit::TestAppContext) {
        let text = "<input ty";
        let (editor, cx) = suggesting_editor(cx, "/w/a.html", "html", text);
        cx.simulate_keystrokes("p");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["type".to_owned()]));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(text_and_cursor(&editor, cx), ("<input type=\"\"".to_owned(), 13..13));
        // Asked for again there: the input types.
        let values = shown(&editor, cx).expect("the values");
        assert_eq!(values[..2], ["button".to_owned(), "checkbox".to_owned()]);
        cx.simulate_keystrokes("t e");
        cx.run_until_parked();
        assert_eq!(shown(&editor, cx), Some(vec!["tel".to_owned(), "text".to_owned(), "time".to_owned()]));
        cx.simulate_keystrokes("x");
        cx.run_until_parked();
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(text_and_cursor(&editor, cx).0, "<input type=\"text\"");
    }

    #[gpui_kit::test]
    fn typing_in_html_and_css_offers_the_languages_suggestions(cx: &mut gpui_kit::TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            bind_keys(cx);
        });
        let labels = |editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| {
                let menu = editor.read(cx).completion_menu_state();
                menu.open.then(|| menu.items.iter().take(2).map(|item| item.label.clone()).collect::<Vec<_>>())
            })
        };
        let text = "<ul>\n";
        let (host, cx) = cx.add_window_view(|window, cx| {
            Host::new(cx.new(|cx| EditorState::new(window, cx).language("html").default_value(text)))
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        cx.update(|window, cx| {
            install(&host, "/w/a.html", "html", no_others(), cx);
            editor.update(cx, |editor, cx| {
                editor.focus(window, cx);
                editor.set_selected_range(text.len()..text.len(), cx);
            });
        });
        cx.run_until_parked();
        // After `<`: the open element's end tag first, then the elements.
        cx.simulate_keystrokes("<");
        cx.run_until_parked();
        assert_eq!(labels(&editor, cx), Some(vec!["/ul".to_owned(), "a".to_owned()]));
        cx.simulate_keystrokes("/");
        cx.run_until_parked();
        assert_eq!(labels(&editor, cx), Some(vec!["/ul".to_owned()]));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).value().to_string()), "<ul>\n</ul>");

        // CSS: property names in a rule, inserted with a colon.
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value("a { }", window, cx));
            install(&host, "/w/a.css", "css", no_others(), cx);
            editor.update(cx, |editor, cx| editor.set_selected_range(4..4, cx));
        });
        cx.simulate_keystrokes("c o l o");
        cx.run_until_parked();
        assert_eq!(labels(&editor, cx).map(|labels| labels[0].clone()), Some("color".to_owned()));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        assert_eq!(cx.update(|_, cx| editor.read(cx).value().to_string()), "a { color: }");
    }
}
