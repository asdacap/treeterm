//! Monaco's language features in the code editors, through GPUI Kit's LSP hooks: the
//! underline under a word with a definition while Cmd/Ctrl is held over it, which a click
//! follows (both editors), and word suggestions while typing (the Editor tab). The model
//! answers both ([`treeterm_model::definition`], [`treeterm_model::words`]).

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::input::{CompletionProvider, DefinitionProvider, EditorState, Rope, RopeExt as _};
use gpui_kit::{App, Entity, Task, Window};
use lsp_types::{
    CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse, CompletionTextEdit,
    CompletionTriggerKind, LocationLink, Range, ShowDocumentParams, TextEdit, Uri,
};
use treeterm_model::definition::DefinitionProbe;
use treeterm_model::words::{self, Trigger};

/// The underline's target: the byte offset of the word, which a click looks up as go to
/// definition does (the Kit has no way to say "this word" otherwise).
const DEFINITION_SCHEME: &str = "treeterm-definition:";

/// The word's offset a [`DEFINITION_SCHEME`] target names; `None` for another target.
fn definition_offset(uri: &Uri) -> Option<usize> {
    let offset = uri.as_str().strip_prefix(DEFINITION_SCHEME)?;
    match offset.parse() {
        Ok(offset) => Some(offset),
        Err(error) => {
            tracing::error!("definition target without an offset: {}: {error}", uri.as_str());
            None
        }
    }
}

/// Monaco's grep `definitionProvider`, for the underline: the word under the pointer has a
/// definition when grep finds one.
struct GrepDefinitions {
    probe: DefinitionProbe,
}

impl DefinitionProvider for GrepDefinitions {
    fn definitions(
        &self,
        text: &Rope,
        offset: usize,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<Vec<LocationLink>>> {
        // A word never spans lines: probe the pointer's line, not the whole text.
        let row = text.offset_to_point(offset).row;
        let line_start = text.line_start_offset(row);
        let line = text.slice_line(row).to_string();
        let Some((word, found)) = self.probe.probe(&line, offset - line_start, cx) else {
            return Task::ready(Ok(Vec::new()));
        };
        let (start, end) = (line_start + word.start, line_start + word.end);
        let origin = Range::new(text.offset_to_position(start), text.offset_to_position(end));
        cx.foreground_executor().spawn(async move {
            if !found.await {
                return Ok(Vec::new());
            }
            let target: Uri = format!("{DEFINITION_SCHEME}{start}").parse()?;
            Ok(vec![LocationLink {
                origin_selection_range: Some(origin),
                target_uri: target,
                target_range: Range::default(),
                target_selection_range: Range::default(),
            }])
        })
    }
}

/// Give `editor` the Cmd/Ctrl+hover underline of `probe`'s file. A click on the underlined
/// word puts the cursor on it and calls `go` with its offset, and sets `clicked` for the
/// view's mouse-up handler, which would otherwise take the click as Cmd/Ctrl+click on the
/// cursor (the Kit takes the press and leaves the cursor where it was). The element around
/// the editor is a [`definition_frame`], which clears `clicked` at each press.
pub(crate) fn install_definitions(
    editor: &Entity<EditorState>,
    probe: DefinitionProbe,
    clicked: Rc<Cell<bool>>,
    go: impl Fn(usize, &mut App) + 'static,
    cx: &mut App,
) {
    let weak = editor.downgrade();
    let show = move |params: &ShowDocumentParams, window: &mut Window, cx: &mut App| -> bool {
        let Some(offset) = definition_offset(&params.uri) else {
            return false;
        };
        clicked.set(true);
        // Monaco moves the cursor to the clicked word first. The editor is busy with the
        // click until this returns.
        let weak = weak.clone();
        window.defer(cx, move |_, cx| {
            if let Some(editor) = weak.upgrade() {
                editor.update(cx, |editor, cx| editor.set_selected_range(offset..offset, cx));
            }
        });
        go(offset, cx);
        true
    };
    editor.update(cx, |editor, _| {
        let lsp = editor.lsp_mut();
        lsp.definition_provider = Some(Rc::new(GrepDefinitions { probe }));
        lsp.show_document = Some(Rc::new(show));
    });
}

/// The element around `editor` (given [`install_definitions`] with `clicked`):
/// - each press starts with `clicked` cleared, before the editor can take it as a click on
///   the underline, so the flag tells about this press only. The mouse-up that reads it
///   does not always come: the button may be released outside the element, or after go to
///   definition switched to another tab.
/// - the underline drops as soon as Cmd/Ctrl is released, as Monaco does (the Kit waits for
///   the pointer to move).
pub(crate) fn definition_frame(
    frame: gpui_kit::Stateful<gpui_kit::Div>,
    editor: &Entity<EditorState>,
    clicked: &Rc<Cell<bool>>,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    use gpui_kit::InteractiveElement as _;
    let editor = editor.downgrade();
    let clicked = clicked.clone();
    frame.capture_any_mouse_down(move |_, _, _| clicked.set(false)).on_modifiers_changed(move |event, _, cx| {
        if event.modifiers.secondary(cx) {
            return;
        }
        if let Some(editor) = editor.upgrade() {
            editor.update(cx, |editor, cx| editor.clear_hover_state(cx));
        }
    })
}

/// Monaco's word-based suggestions (the base editor worker's `textualSuggest`).
struct WordSuggestions;

impl CompletionProvider for WordSuggestions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<CompletionResponse>> {
        let trigger = match trigger.trigger_kind {
            CompletionTriggerKind::INVOKED => Trigger::Invoked,
            _ => Trigger::Typing,
        };
        let text = text.clone();
        cx.background_executor().spawn(async move {
            let suggestions = words::suggestions(&text.to_string(), offset, trigger);
            let replace = Range::new(
                text.offset_to_position(suggestions.replace.start),
                text.offset_to_position(suggestions.replace.end),
            );
            let items = suggestions
                .words
                .into_iter()
                .map(|word| CompletionItem {
                    label: word.clone(),
                    kind: Some(CompletionItemKind::TEXT),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit { range: replace, new_text: word })),
                    ..CompletionItem::default()
                })
                .collect();
            Ok(CompletionResponse::Array(items))
        })
    }

    /// Every typed character: a word character asks for suggestions, anything else closes
    /// them (it leaves no word being typed). Pasted text does not, as in Monaco.
    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().count() == 1
    }
}

/// Give `editor` Monaco's word-based suggestions.
pub(crate) fn install_word_suggestions(editor: &Entity<EditorState>, cx: &mut App) {
    editor.update(cx, |editor, _| editor.lsp_mut().completion_provider = Some(Rc::new(WordSuggestions)));
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::Arc;

    use futures::StreamExt as _;
    use futures::stream::BoxStream;
    use gpui_kit::component::input::Editor;
    use gpui_kit::prelude::*;
    use gpui_kit::{Context, Modifiers, MouseButton, div, px};
    use treeterm_core::exec::{ExecApi, ExecError, ExecEvent, ExecRequest};

    use super::*;

    /// grep finds one definition, whatever is searched for.
    struct GrepFindsOne;

    impl ExecApi for GrepFindsOne {
        fn start(&self, _: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>> {
            let found = ExecEvent::Stdout(b"./a.rs:1:fn helper() {}\n".to_vec());
            futures::stream::iter([Ok(found), Ok(ExecEvent::Exit { code: 0 })]).boxed()
        }
    }

    /// A code editor in its definition frame.
    struct Host {
        editor: Entity<EditorState>,
        clicked: Rc<Cell<bool>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            definition_frame(div().id("frame"), &self.editor, &self.clicked)
                .w(px(400.))
                .h(px(300.))
                .child(Editor::new(&self.editor).h_full())
        }
    }

    #[gpui_kit::test]
    fn each_press_tells_only_whether_it_went_to_the_underline(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let text = "use a::helper;\nfn main() { helper() }\n";
        let clicked = Rc::new(Cell::new(false));
        let (host, cx) = cx.add_window_view(|window, cx| Host {
            editor: cx.new(|cx| EditorState::new(window, cx).language("rust").default_value(text)),
            clicked: clicked.clone(),
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        let gone = Rc::new(RefCell::new(Vec::new()));
        cx.update(|_, cx| {
            let probe = DefinitionProbe::new(Arc::new(GrepFindsOne), "/w".into(), "/w/b.rs".into());
            let gone = gone.clone();
            install_definitions(&editor, probe, clicked.clone(), move |offset, _| gone.borrow_mut().push(offset), cx);
        });
        cx.run_until_parked();
        let helper = text.rfind("helper").unwrap();
        let word = cx.update(|_, cx| editor.read(cx).range_to_bounds(&(helper..helper + 6))).expect("laid out").center();

        // A press on the underline was released outside the editor: no mouse-up read the flag.
        clicked.set(true);
        cx.simulate_mouse_down(word, MouseButton::Left, Modifiers::none());
        assert!(!clicked.get(), "a plain press is not a click on the underline");
        cx.simulate_mouse_up(word, MouseButton::Left, Modifiers::none());

        // Holding Cmd/Ctrl over the word underlines it; a press there follows it.
        let secondary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        cx.simulate_mouse_move(word, None, secondary);
        cx.run_until_parked();
        cx.simulate_mouse_down(word, MouseButton::Left, secondary);
        assert!(clicked.get(), "the press went to the underline");
        assert_eq!(*gone.borrow(), [helper]);
    }

    #[test]
    fn a_definition_target_names_the_words_offset() {
        let ours: Uri = format!("{DEFINITION_SCHEME}42").parse().unwrap();
        assert_eq!(definition_offset(&ours), Some(42));
        let other: Uri = "file:///w/a.rs".parse().unwrap();
        assert_eq!(definition_offset(&other), None);
    }
}
