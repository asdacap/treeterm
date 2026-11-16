//! Monaco's language features in the code editors, through GPUI Kit's LSP hooks. The model
//! answers them all ([`treeterm_model::definition`], [`treeterm_model::words`],
//! [`treeterm_model::code_intel`]):
//! - both editors: the underline under a word with a definition while Cmd/Ctrl is held over
//!   it, which a click follows; links (underlined, "Follow link (cmd + click)" on hover,
//!   Cmd/Ctrl+click opens a web link in the browser and a `file:` link in an Editor tab);
//!   the language workers' problems (squiggles, their message on hover); and CSS colours;
//! - the Editor tab: suggestions while typing, the language's (CSS properties, HTML elements
//!   and attributes) or else the words of the file and of the other open files of its
//!   language; none while typing in a comment or a string, unless the character typed asks
//!   the language's worker.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_kit::component::input::{
    CompletionProvider, DefinitionProvider, DocumentColorProvider, EditorState, HoverProvider, Rope, RopeExt as _,
    TextDecoration,
};
use gpui_kit::{App, Entity, Task, Window};
use lsp_types::{
    Color, ColorInformation, CompletionContext, CompletionItem, CompletionItemKind, CompletionResponse,
    CompletionTextEdit, CompletionTriggerKind, Hover, HoverContents, LocationLink, MarkupContent, MarkupKind, Range,
    ShowDocumentParams, TextEdit, Uri,
};
use treeterm_model::code_intel::{self, Asked, Severity, Suggested, links};
use treeterm_model::definition::DefinitionProbe;
use treeterm_model::words;

/// A Cmd/Ctrl+hover target: the byte offset of the word (go to definition) or link there,
/// which a click looks up (the Kit has no way to say "this word" otherwise).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    Definition(usize),
    Link(usize),
}

const DEFINITION_SCHEME: &str = "treeterm-definition:";
const LINK_SCHEME: &str = "treeterm-link:";

impl Target {
    fn uri(self) -> gpui_kit::Result<Uri> {
        let uri = match self {
            Self::Definition(offset) => format!("{DEFINITION_SCHEME}{offset}"),
            Self::Link(offset) => format!("{LINK_SCHEME}{offset}"),
        };
        Ok(uri.parse()?)
    }

    /// The target `uri` names; `None` for another one.
    fn of(uri: &Uri) -> Option<Self> {
        let (offset, target): (&str, fn(usize) -> Self) = match uri.as_str() {
            uri if uri.starts_with(DEFINITION_SCHEME) => (&uri[DEFINITION_SCHEME.len()..], Self::Definition),
            uri if uri.starts_with(LINK_SCHEME) => (&uri[LINK_SCHEME.len()..], Self::Link),
            _ => return None,
        };
        match offset.parse() {
            Ok(offset) => Some(target(offset)),
            Err(error) => {
                tracing::error!("hover target without an offset: {}: {error}", uri.as_str());
                None
            }
        }
    }
}

/// The LSP range of bytes `range` of `text`.
fn lsp_range(text: &Rope, range: std::ops::Range<usize>) -> Range {
    Range::new(text.offset_to_position(range.start), text.offset_to_position(range.end))
}

/// Monaco's grep `definitionProvider` and its link detector, for the underline: a link under
/// the pointer is underlined whole; another word when grep finds a definition of it.
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
        // Neither spans lines: look at the pointer's line, not the whole text.
        let row = text.offset_to_point(offset).row;
        let line_start = text.line_start_offset(row);
        let line = text.slice_line(row).to_string();
        let link_or_word = |target: Target, range: std::ops::Range<usize>| -> gpui_kit::Result<Vec<LocationLink>> {
            Ok(vec![LocationLink {
                origin_selection_range: Some(lsp_range(text, range)),
                target_uri: target.uri()?,
                target_range: Range::default(),
                target_selection_range: Range::default(),
            }])
        };
        if let Some(link) = links::link_at(&line, offset - line_start) {
            let range = line_start + link.range.start..line_start + link.range.end;
            return Task::ready(link_or_word(Target::Link(range.start), range));
        }
        let Some((word, found)) = self.probe.probe(&line, offset - line_start, cx) else {
            return Task::ready(Ok(Vec::new()));
        };
        let (start, end) = (line_start + word.start, line_start + word.end);
        let location = link_or_word(Target::Definition(start), start..end);
        cx.foreground_executor().spawn(async move {
            if !found.await {
                return Ok(Vec::new());
            }
            location
        })
    }
}

/// What a Cmd/Ctrl+click on the underline follows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Follow {
    /// The word at this byte offset: its definition.
    Definition(usize),
    /// A link: opened where Monaco's opener takes it (the model's `open_link`).
    Link(String),
}

/// Give `editor` the Cmd/Ctrl+hover underline of `probe`'s file, and of its links. A click
/// on an underlined word puts the cursor on it and calls `go` with its offset; one on a link
/// calls `go` with its URL. Either sets `clicked` for the view's mouse-up handler, which
/// would otherwise take the click as Cmd/Ctrl+click on the cursor (the Kit takes the press
/// and leaves the cursor where it was). The element around the editor is a
/// [`definition_frame`], which clears `clicked` at each press.
pub(crate) fn install_definitions(
    editor: &Entity<EditorState>,
    probe: DefinitionProbe,
    clicked: Rc<Cell<bool>>,
    go: impl Fn(Follow, &mut App) + 'static,
    cx: &mut App,
) {
    let weak = editor.downgrade();
    let go = Rc::new(go);
    let show = move |params: &ShowDocumentParams, window: &mut Window, cx: &mut App| -> bool {
        let Some(target) = Target::of(&params.uri) else {
            return false;
        };
        clicked.set(true);
        // The editor is busy with the click until this returns.
        let editor = weak.clone();
        match target {
            Target::Link(offset) => {
                let go = go.clone();
                window.defer(cx, move |_, cx| {
                    // The link's text is read again: the Kit gives back only the target.
                    let Some(editor) = editor.upgrade() else {
                        return;
                    };
                    let text = editor.read(cx).text().to_string();
                    match links::link_at(&text, offset) {
                        Some(link) => go(Follow::Link(link.url), cx),
                        None => tracing::error!("no link at {offset} to open"),
                    }
                });
            }
            Target::Definition(offset) => {
                // Monaco moves the cursor to the clicked word first.
                window.defer(cx, move |_, cx| {
                    if let Some(editor) = editor.upgrade() {
                        editor.update(cx, |editor, cx| editor.set_selected_range(offset..offset, cx));
                    }
                });
                go(Follow::Definition(offset), cx);
            }
        }
        true
    };
    editor.update(cx, |editor, _| {
        let lsp = editor.lsp_mut();
        lsp.definition_provider = Some(Rc::new(GrepDefinitions { probe }));
        lsp.show_document = Some(Rc::new(show));
        lsp.hover_provider = Some(Rc::new(LinkHover));
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

/// Monaco's hover over a link (`links.getHoverMessage`): "Follow link", itself a link, and
/// the click that follows it.
fn link_hover_message(url: &str) -> String {
    let click = if cfg!(target_os = "macos") { "cmd + click" } else { "ctrl + click" };
    let target = url.replace(' ', "%20").replace('(', "%28").replace(')', "%29");
    format!("[Follow link]({target}) ({click})")
}

/// The hover over a link.
struct LinkHover;

impl HoverProvider for LinkHover {
    fn hover(&self, text: &Rope, offset: usize, _: &mut Window, _: &mut App) -> Task<gpui_kit::Result<Option<Hover>>> {
        let row = text.offset_to_point(offset).row;
        let line_start = text.line_start_offset(row);
        let line = text.slice_line(row).to_string();
        let hover = links::link_at(&line, offset - line_start).map(|link| Hover {
            contents: HoverContents::Markup(MarkupContent { kind: MarkupKind::Markdown, value: link_hover_message(&link.url) }),
            range: Some(lsp_range(text, line_start + link.range.start..line_start + link.range.end)),
        });
        Task::ready(Ok(hover))
    }
}

/// Monaco's `detected-link` decoration: every link of `text` underlined.
pub(crate) fn link_underlines(text: &str) -> Vec<TextDecoration> {
    let underline = gpui_kit::HighlightStyle {
        underline: Some(gpui_kit::UnderlineStyle { thickness: gpui_kit::px(1.), ..Default::default() }),
        ..Default::default()
    };
    links::links(text).into_iter().map(|link| TextDecoration::new(link.range, underline)).collect()
}

/// The problems Monaco's language workers mark in a file of Monaco's `language` at `path`,
/// marked in its editor: squiggles, with their message on hover.
pub(crate) struct ProblemMarks {
    language: &'static str,
    path: String,
    /// The pass marking the text the editor has now.
    _pass: Task<()>,
}

impl ProblemMarks {
    pub(crate) fn new(language: &'static str, path: String) -> Self {
        Self { language, path, _pass: Task::ready(()) }
    }

    /// Mark the problems of `editor`'s text once found, unless it changed meanwhile (the
    /// change starts another pass, and the editor drops its marks at every change).
    pub(crate) fn mark(&mut self, editor: &Entity<EditorState>, cx: &mut App) {
        let text = editor.read(cx).text().clone();
        let (language, path) = (self.language, self.path.clone());
        let found = cx.background_executor().spawn(async move {
            let problems = code_intel::problems(language, &path, &text.to_string());
            let diagnostics: Vec<Diagnostic> = problems
                .into_iter()
                .map(|problem| {
                    let range = lsp_range(&text, problem.range);
                    let severity = match problem.severity {
                        Severity::Error => DiagnosticSeverity::Error,
                        Severity::Warning => DiagnosticSeverity::Warning,
                    };
                    Diagnostic::new(range.start..range.end, problem.message).with_severity(severity)
                })
                .collect();
            (text, diagnostics)
        });
        let editor = editor.downgrade();
        self._pass = cx.spawn(async move |cx| {
            let (text, diagnostics) = found.await;
            let marked = editor.update(cx, |editor, cx| {
                if *editor.text() != text {
                    return;
                }
                if let Some(marks) = editor.diagnostics_mut() {
                    marks.reset(&text);
                    marks.extend(diagnostics);
                    cx.notify();
                }
            });
            // An editor closed meanwhile has nothing to mark.
            if marked.is_err() {
                tracing::debug!("problems found for a closed editor");
            }
        });
    }
}

/// The CSS worker's colours, painted behind their text.
struct StyleSheetColors {
    language: &'static str,
}

impl DocumentColorProvider for StyleSheetColors {
    fn document_colors(&self, text: &Rope, _: &mut Window, cx: &mut App) -> Task<gpui_kit::Result<Vec<ColorInformation>>> {
        let (text, language) = (text.clone(), self.language);
        cx.background_executor().spawn(async move {
            let colors = code_intel::css::colors(language, &text.to_string())
                .into_iter()
                .map(|(range, color)| ColorInformation {
                    range: lsp_range(&text, range),
                    color: Color { red: color.red, green: color.green, blue: color.blue, alpha: color.alpha },
                })
                .collect();
            Ok(colors)
        })
    }
}

/// Give `editor`, a file of Monaco's `language`, the CSS worker's colours when it reads that
/// language (CSS, SCSS, Less).
pub(crate) fn install_colors(editor: &Entity<EditorState>, language: &'static str, cx: &mut App) {
    if code_intel::css::Dialect::of(language).is_none() {
        return;
    }
    editor.update(cx, |editor, _| {
        editor.lsp_mut().document_color_provider = Some(Rc::new(StyleSheetColors { language }));
    });
}

/// The texts of the other open files of the editor's language, whose words Monaco suggests too.
pub(crate) type OtherTexts = Rc<dyn Fn(&App) -> Vec<String>>;

/// Monaco's suggestions in a file of Monaco's `language` ([`code_intel::suggest`]): its
/// language worker's (CSS properties, HTML elements and attributes) where it offers some,
/// else the words of the file and of the other open files of its language (the base editor
/// worker's `textualSuggest`); typing in a comment or a string suggests nothing unless it
/// asks the worker.
struct Suggestions {
    language: &'static str,
    /// The GPUI Kit grammar the editor highlights the file with, which tells comments and
    /// strings ([`crate::file_browser_view::language_for`]).
    grammar: &'static str,
    others: OtherTexts,
    /// Where the last suggestions offered were asked for. The Kit closes them at every edit
    /// and asks again, so a character typed right there goes on with them.
    shown: Rc<Cell<Shown>>,
}

/// Where suggestions were last offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shown {
    Nothing,
    /// At this cursor offset.
    At(usize),
}

/// Whether the character before byte `offset` of `text` is in a comment or a string, as GPUI
/// Kit's `grammar` reads it; never in a file it has no grammar for.
fn in_comment_or_string(grammar: &str, text: &str, offset: usize) -> bool {
    let registry = gpui_kit::component::highlighter::LanguageRegistry::singleton();
    registry
        .language(grammar)
        .and_then(|config| config.language)
        .is_some_and(|grammar| code_intel::syntax::in_comment_or_string(&grammar, text, offset))
}

impl CompletionProvider for Suggestions {
    fn completions(
        &self,
        text: &Rope,
        offset: usize,
        trigger: CompletionContext,
        _: &mut Window,
        cx: &mut App,
    ) -> Task<gpui_kit::Result<CompletionResponse>> {
        let typed = trigger.trigger_kind != CompletionTriggerKind::INVOKED;
        let (text, language, grammar, others) = (text.clone(), self.language, self.grammar, (self.others)(cx));
        let (shown, last_shown) = (self.shown.clone(), self.shown.get());
        let found = cx.background_executor().spawn(async move {
            let string = text.to_string();
            let asked = if typed {
                let before = string.get(..offset).and_then(|before| before.chars().next_back());
                let typed_at = before.map_or(offset, |typed| offset - typed.len_utf8());
                if last_shown == Shown::At(typed_at) {
                    Asked::Refined
                } else {
                    Asked::Typed { in_comment_or_string: in_comment_or_string(grammar, &string, offset) }
                }
            } else {
                Asked::Invoked
            };
                let (replace, items, kind): (_, Vec<(String, String)>, _) =
                match code_intel::suggest(language, &string, &others, offset, asked) {
                    Suggested::Language(found) => {
                        let items = found.items.into_iter().map(|item| (item.label, item.insert)).collect();
                        (found.replace, items, CompletionItemKind::PROPERTY)
                    }
                    Suggested::Words(found) => {
                        let items = found.words.into_iter().map(|word| (word.clone(), word)).collect();
                        (found.replace, items, CompletionItemKind::TEXT)
                    }
                    Suggested::Nothing => return Vec::new(),
                };
            let replace = lsp_range(&text, replace);
            items
                .into_iter()
                .map(|(label, insert)| CompletionItem {
                    label,
                    kind: Some(kind),
                    text_edit: Some(CompletionTextEdit::Edit(TextEdit { range: replace, new_text: insert })),
                    ..CompletionItem::default()
                })
                .collect::<Vec<_>>()
        });
        cx.spawn(async move |_| {
            let items = found.await;
            shown.set(if items.is_empty() { Shown::Nothing } else { Shown::At(offset) });
            Ok(CompletionResponse::Array(items))
        })
    }

    /// Every typed character: a word character asks for suggestions, anything else closes
    /// them (it leaves no word being typed) unless the language offers some there (after
    /// `<` in HTML). Pasted text does not, as in Monaco.
    fn is_completion_trigger(&self, _: usize, new_text: &str, _: &mut App) -> bool {
        new_text.chars().count() == 1
    }
}

/// Give `editor`, a file at `path` of Monaco's `language`, Monaco's suggestions, their
/// letters that match what was typed highlighted; `others` are the other open files' texts.
pub(crate) fn install_suggestions(
    editor: &Entity<EditorState>,
    path: &str,
    language: &'static str,
    others: OtherTexts,
    cx: &mut App,
) {
    let grammar = crate::file_browser_view::language_for(path);
    let shown = Rc::new(Cell::new(Shown::Nothing));
    editor.update(cx, |editor, _| {
        let lsp = editor.lsp_mut();
        lsp.completion_provider = Some(Rc::new(Suggestions { language, grammar, others, shown }));
        lsp.set_completion_highlighter(Rc::new(|typed: &str, item: &CompletionItem| words::highlights(typed, &item.label)));
    });
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
            install_definitions(&editor, probe, clicked.clone(), move |follow, _| gone.borrow_mut().push(follow), cx);
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
        assert_eq!(*gone.borrow(), [Follow::Definition(helper)]);
    }

    #[test]
    fn a_hover_target_names_the_words_or_links_offset() {
        for target in [Target::Definition(42), Target::Link(7)] {
            assert_eq!(Target::of(&target.uri().unwrap()), Some(target));
        }
        let other: Uri = "file:///w/a.rs".parse().unwrap();
        assert_eq!(Target::of(&other), None);
    }

    /// The messages marked in `editor`, with the text each marks.
    fn marks(editor: &Entity<EditorState>, cx: &mut gpui_kit::VisualTestContext) -> Vec<(String, String)> {
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            let text = editor.text().to_string();
            editor
                .diagnostics()
                .map(|marks| marks.iter().map(|mark| (text[mark.range.clone()].to_owned(), mark.message.to_string())).collect())
                .unwrap_or_default()
        })
    }

    #[gpui_kit::test]
    fn a_files_problems_are_marked_and_marked_again_after_each_edit(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let text = "{\"a\": 1,}";
        let (host, cx) = cx.add_window_view(|window, cx| Host {
            editor: cx.new(|cx| EditorState::new(window, cx).language("json").default_value(text)),
            clicked: Rc::new(Cell::new(false)),
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        let mut problems = ProblemMarks::new("json", "/w/a.json".to_owned());
        cx.update(|_, cx| problems.mark(&editor, cx));
        cx.run_until_parked();
        assert_eq!(marks(&editor, cx), [(",".to_owned(), "Trailing comma".to_owned())]);
        // An edit drops the marks; the next pass marks the new text.
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| editor.set_value("{\"a\": 1 // c\n}", window, cx));
            problems.mark(&editor, cx);
        });
        cx.run_until_parked();
        assert_eq!(marks(&editor, cx), [("// c".to_owned(), "Comments are not permitted in JSON.".to_owned())]);
        // A file of a language without them has none.
        let mut none = ProblemMarks::new("rust", "/w/a.rs".to_owned());
        cx.update(|_, cx| none.mark(&editor, cx));
        cx.run_until_parked();
        assert_eq!(marks(&editor, cx), []);
    }

    #[gpui_kit::test]
    fn cmd_click_on_a_link_follows_it_and_goes_to_no_definition(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let text = "// see https://example.com/docs.\n// and file:///w/a.rs#L3\nfn main() {}\n";
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
            install_definitions(&editor, probe, clicked.clone(), move |follow, _| gone.borrow_mut().push(follow), cx);
        });
        cx.run_until_parked();
        let secondary = cx.update(|_, cx| Modifiers::secondary_key(cx));
        for word in ["docs", "a.rs"] {
            clicked.set(false);
            let at = text.find(word).unwrap();
            let link = cx.update(|_, cx| editor.read(cx).range_to_bounds(&(at..at + 4))).expect("laid out").center();
            cx.simulate_mouse_move(link, None, secondary);
            cx.run_until_parked();
            cx.simulate_mouse_down(link, MouseButton::Left, secondary);
            cx.simulate_mouse_up(link, MouseButton::Left, secondary);
            cx.run_until_parked();
            assert!(clicked.get(), "the press went to the link at {word}");
        }
        // The model opens each where Monaco's opener takes it; the hook opens nothing itself.
        assert_eq!(*gone.borrow(), [
            Follow::Link("https://example.com/docs".to_owned()),
            Follow::Link("file:///w/a.rs#L3".to_owned())
        ]);
        assert_eq!(cx.opened_url(), None);
    }

    #[test]
    fn a_link_is_underlined_and_its_hover_follows_it() {
        let underlines = link_underlines("see http://a.b/c and https://d.e");
        assert_eq!(underlines.iter().map(|underline| underline.range.clone()).collect::<Vec<_>>(), [4..16, 21..32]);
        let click = if cfg!(target_os = "macos") { "cmd + click" } else { "ctrl + click" };
        assert_eq!(link_hover_message("http://a.b/a (b)"), format!("[Follow link](http://a.b/a%20%28b%29) ({click})"));
    }

    #[gpui_kit::test]
    fn a_style_sheets_colours_are_given_to_the_editor(cx: &mut gpui_kit::TestAppContext) {
        cx.update(gpui_kit::init);
        let text = "a { color: #ff0000; }";
        let (host, cx) = cx.add_window_view(|window, cx| Host {
            editor: cx.new(|cx| EditorState::new(window, cx).language("css").default_value(text)),
            clicked: Rc::new(Cell::new(false)),
        });
        let editor = cx.update(|_, cx| host.read(cx).editor.clone());
        // Only a style sheet has them.
        cx.update(|_, cx| install_colors(&editor, "rust", cx));
        assert!(cx.update(|_, cx| editor.read(cx).lsp().document_color_provider.is_none()));
        cx.update(|_, cx| install_colors(&editor, "css", cx));
        let provider = cx.update(|_, cx| editor.read(cx).lsp().document_color_provider.clone()).expect("a colour provider");
        let found = Rc::new(RefCell::new(None));
        cx.update(|window, cx| {
            let colors = provider.document_colors(&Rope::from(text), window, cx);
            let found = found.clone();
            cx.spawn(async move |_| *found.borrow_mut() = Some(colors.await)).detach();
        });
        cx.run_until_parked();
        let colors = found.borrow_mut().take().expect("colours").unwrap();
        assert_eq!(colors.len(), 1);
        assert_eq!((colors[0].range.start.character, colors[0].range.end.character), (11, 18));
        assert_eq!((colors[0].color.red, colors[0].color.green, colors[0].color.alpha), (1., 0., 1.));
    }
}
