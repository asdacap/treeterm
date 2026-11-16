//! The Editor tab: one file, edited and saved. Port of the non-view half of
//! `FileEditor.tsx`.
//!
//! The tab state keeps Electron's `EditorState` shape (`status`, `filePath`,
//! `originalContent`, `currentContent`, `language`, `isDirty`, `viewMode`), so an editor tab
//! opened in either app shows the same unsaved edits in the other.
//!
//! Every edit goes into the tab state at once, as Electron's `handleEditorChange` writes
//! it, and the editor follows the state other windows and Electron write
//! ([`TextEditor::follow_state`]), as Monaco follows `value={state.currentContent}`.
//!
//! The editor's scroll offset is saved as `scrollTop` when the tab is hidden (Electron
//! unmounts the editor then) and shown again when the tab is built from a loaded state; a
//! `scrollToLine` is moved to instead, then removed ([`TextEditor::take_scroll_target`]).
//!
//! Go to definition searches the text as it is, unsaved edits included
//! ([`crate::definition`]); a definition in another file opens in a new Editor tab at its
//! line (`openFileAtLine`).

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{Context, EventEmitter, Task, WeakEntity};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecApi, ExecError};
use treeterm_core::fs::{FsClient, FsError, ReadOutcome, WriteGuard, WriteOutcome};
use treeterm_core::search::{self, DefinitionLocation};

use crate::definition::{Begin, DefinitionLookup, DefinitionProbe, Settled};
use crate::file_browser::{fs_error_text, fs_outcome, scroll_top_of, scroll_top_value};
use crate::workspace_store::WorkspaceStore;

/// How many of the texts it last wrote the editor remembers ([`TextEditor::follow_state`]).
const WRITTEN_KEPT: usize = 8;

/// For how long after the editor wrote over a text of its own another writer's copy of that
/// text counts as a stale copy rather than an edit ([`TextEditor::follow_state`]).
const STALE_COPY_WINDOW: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq)]
pub enum EditorDoc {
    Loading,
    Ready {
        original: String,
        current: String,
        language: String,
    },
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum SaveState {
    Idle,
    Saving,
    Failed(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum TextEditorEvent {
    /// Persist this into the tab state (merged over it).
    StateChanged(Value),
    /// This key of the tab state was used up: remove it (`scrollToLine: undefined`).
    StateRemoved(&'static str),
}

/// Where the editor moves when it shows the text.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollTarget {
    /// Reveal this 1-based line in the middle (`scrollToLine`, go to definition).
    Line(usize),
    /// Scroll this many pixels down (the saved `scrollTop`).
    Top(f32),
}

impl EventEmitter<TextEditorEvent> for TextEditor {}

/// `EditorViewMode`: markdown shows a preview until switched to the editor.
const VIEW_MODE_EDITOR: &str = "editor";
const VIEW_MODE_PREVIEW: &str = "preview";

/// The tab title: the file name, with ` •` while there are unsaved edits.
pub fn tab_title(path: &str, dirty: bool) -> String {
    let name = path.rsplit('/').next().filter(|name| !name.is_empty()).unwrap_or(path);
    if dirty { format!("{name} \u{2022}") } else { name.to_owned() }
}

/// The title a persisted editor state calls for, if it names a file.
pub fn tab_title_of_state(state: &Value) -> Option<String> {
    let path = state.get("filePath").and_then(Value::as_str).filter(|path| !path.is_empty())?;
    let dirty = state.get("isDirty").and_then(Value::as_bool).unwrap_or(false);
    Some(tab_title(path, dirty))
}

/// The document of a `status: "ready"` editor state, which has its text; `None` for any other.
fn ready_doc(state: &Value) -> Option<EditorDoc> {
    let text = |key: &str| state.get(key).and_then(Value::as_str);
    match (text("status"), text("originalContent"), text("currentContent")) {
        (Some("ready"), Some(original), Some(current)) => Some(EditorDoc::Ready {
            original: original.to_owned(),
            current: current.to_owned(),
            language: text("language").unwrap_or("plaintext").to_owned(),
        }),
        _ => None,
    }
}

/// An editor state's `viewMode`, the editor when it has none.
fn view_mode_of(state: &Value) -> &str {
    state.get("viewMode").and_then(Value::as_str).unwrap_or(VIEW_MODE_EDITOR)
}

pub struct TextEditor {
    fs: FsClient,
    exec: Arc<dyn ExecApi>,
    /// The workspace the tab is in, where go to definition opens other files.
    workspace: WeakEntity<WorkspaceStore>,
    root: String,
    path: String,
    doc: EditorDoc,
    save: SaveState,
    view_mode: String,
    /// Where to move once the text is shown (go to definition), until the view has.
    scroll_to_line: Option<usize>,
    /// The `scrollTop` of the loaded state the tab was built from, until the view has shown it.
    scroll_restore: Option<f32>,
    definition: DefinitionLookup,
    /// The texts this editor last wrote to the tab state, newest last, each with when.
    written: VecDeque<(String, Instant)>,
    /// Bumped whenever the text is replaced by another writer's, so the view takes it.
    external_revision: u64,
    _load: Option<Task<()>>,
    _save: Option<Task<()>>,
    _definition: Option<Task<()>>,
}

impl TextEditor {
    /// `state` is the tab's persisted `EditorState`; a Ready state with content is used as
    /// is (unsaved edits survive), anything else loads the file.
    pub fn new(
        fs: FsClient,
        exec: Arc<dyn ExecApi>,
        workspace: WeakEntity<WorkspaceStore>,
        root: String,
        state: &Value,
        cx: &mut Context<Self>,
    ) -> Self {
        let path = state.get("filePath").and_then(Value::as_str).unwrap_or_default().to_owned();
        let mut editor = Self {
            fs,
            exec,
            workspace,
            root,
            path,
            doc: EditorDoc::Loading,
            save: SaveState::Idle,
            view_mode: view_mode_of(state).to_owned(),
            scroll_to_line: state.get("scrollToLine").and_then(Value::as_u64).map(|line| line as usize),
            scroll_restore: None,
            definition: DefinitionLookup::Idle,
            written: VecDeque::new(),
            external_revision: 0,
            _load: None,
            _save: None,
            _definition: None,
        };
        match ready_doc(state) {
            Some(doc) => {
                editor.doc = doc;
                // Only a loaded state's offset counts, and a line asked for wins over it.
                if editor.scroll_to_line.is_none() {
                    editor.scroll_restore = scroll_top_of(state);
                }
            }
            None if editor.path.is_empty() => editor.doc = EditorDoc::Error("Invalid tab".to_owned()),
            None => editor.load(cx),
        }
        editor
    }

    /// The tab's state as another window or Electron wrote it: its text, unsaved mark and
    /// view mode replace this editor's, as Monaco takes `value={state.currentContent}`.
    /// States still loading or failed are left to the window that wrote them, which goes on
    /// to write the text. A save under way still finishes.
    ///
    /// A writer that changed something else (an unread marker, a terminal) and won the race
    /// to save the workspace over this editor's newest write can bring back a text this
    /// editor wrote moments before: then the text typed since is kept and written again.
    pub fn follow_state(&mut self, state: &Value, cx: &mut Context<Self>) {
        let Some(doc) = ready_doc(state) else {
            return;
        };
        let view_mode = view_mode_of(state);
        if doc == self.doc && view_mode == self.view_mode {
            return;
        }
        if let EditorDoc::Ready { current, .. } = &doc
            && self.is_stale_copy(current, cx.background_executor().now())
        {
            self.persist_now(cx);
            return;
        }
        self.doc = doc;
        self.view_mode = view_mode.to_owned();
        // Texts written before are history now: the other writer's edits start from this one.
        self.written.clear();
        self._load = None;
        self.external_revision += 1;
        cx.notify();
    }

    /// `current` is a text this editor wrote and, moments ago, wrote over: another writer's
    /// copy of an older state, not an edit of its own.
    fn is_stale_copy(&self, current: &str, now: Instant) -> bool {
        let Some((newest, _)) = self.written.back() else {
            return false;
        };
        let mut replaced = self.written.iter().zip(self.written.iter().skip(1));
        newest != current
            && replaced.any(|((text, _), (_, replaced_at))| {
                text == current && now.saturating_duration_since(*replaced_at) < STALE_COPY_WINDOW
            })
    }

    /// Bumped whenever another writer's text replaced this editor's ([`Self::follow_state`]).
    pub fn external_revision(&self) -> u64 {
        self.external_revision
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn doc(&self) -> &EditorDoc {
        &self.doc
    }

    pub fn save_state(&self) -> &SaveState {
        &self.save
    }

    /// The language the header names (Monaco's id for the file).
    pub fn language(&self) -> &'static str {
        treeterm_core::files::detect_monaco_language(&self.path)
    }

    pub fn is_markdown(&self) -> bool {
        self.language() == "markdown"
    }

    /// A markdown file shows its preview rather than the editor.
    pub fn is_preview(&self) -> bool {
        self.is_markdown() && self.view_mode == VIEW_MODE_PREVIEW
    }

    /// "Edit" / "Preview" on a markdown file.
    pub fn toggle_view_mode(&mut self, cx: &mut Context<Self>) {
        let EditorDoc::Ready { .. } = self.doc else {
            return;
        };
        self.view_mode = if self.view_mode == VIEW_MODE_PREVIEW { VIEW_MODE_EDITOR } else { VIEW_MODE_PREVIEW }.to_owned();
        self.persist_now(cx);
        cx.notify();
    }

    /// Where to move once the editor shows the text, taken once: a line asked for, which is
    /// then removed from the tab state, or else the saved offset.
    pub fn take_scroll_target(&mut self, cx: &mut Context<Self>) -> Option<ScrollTarget> {
        if let Some(line) = self.scroll_to_line.take() {
            cx.emit(TextEditorEvent::StateRemoved("scrollToLine"));
            return Some(ScrollTarget::Line(line));
        }
        self.scroll_restore.take().map(ScrollTarget::Top)
    }

    /// The editor was hidden (Electron unmounts it) scrolled `top` pixels down: save that as
    /// `scrollTop`, in a loaded state only. Not while the saved offset is still to be shown,
    /// which the editor has not scrolled to yet: it stays saved.
    pub fn save_scroll_top(&mut self, top: f32, cx: &mut Context<Self>) {
        let EditorDoc::Ready { .. } = self.doc else {
            return;
        };
        if self.scroll_restore.is_some() {
            return;
        }
        cx.emit(TextEditorEvent::StateChanged(json!({ "scrollTop": scroll_top_value(top) })));
    }

    /// The tab title for the current state.
    pub fn title(&self) -> String {
        tab_title(&self.path, self.is_dirty())
    }

    /// The user acknowledged a failed save (Electron's `alert`).
    pub fn dismiss_save_error(&mut self, cx: &mut Context<Self>) {
        if let SaveState::Failed(_) = self.save {
            self.save = SaveState::Idle;
            cx.notify();
        }
    }

    pub fn is_dirty(&self) -> bool {
        match &self.doc {
            EditorDoc::Ready { original, current, .. } => original != current,
            EditorDoc::Loading | EditorDoc::Error(_) => false,
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.doc = EditorDoc::Loading;
        let read = self.fs.read_file(&self.root, &self.path, u64::MAX);
        let language = self.language();
        self._load = Some(cx.spawn(async move |this, cx| {
            let doc = match fs_outcome(read.await) {
                Ok(ReadOutcome::Read(file)) => match String::from_utf8(file.content) {
                    Ok(text) => EditorDoc::Ready {
                        original: text.clone(),
                        current: text,
                        language: language.to_owned(),
                    },
                    Err(_) => EditorDoc::Error("Binary files cannot be edited".to_owned()),
                },
                Ok(ReadOutcome::TooLarge { size }) => EditorDoc::Error(format!("File too large ({size} bytes)")),
                // The daemon's reason as it gave it (`error: result.error`).
                Err(error) => EditorDoc::Error(fs_error_text(&error, "", "Error loading file: ")),
            };
            match this.update(cx, |editor, cx| {
                // A markdown file opens on its preview.
                if let EditorDoc::Ready { .. } = doc {
                    editor.view_mode = if language == "markdown" { VIEW_MODE_PREVIEW } else { VIEW_MODE_EDITOR }.to_owned();
                }
                editor.doc = doc;
                editor.persist_now(cx);
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// The user edited the text.
    pub fn set_content(&mut self, text: String, cx: &mut Context<Self>) {
        let EditorDoc::Ready { current, .. } = &mut self.doc else {
            return;
        };
        if *current == text {
            return;
        }
        *current = text;
        self.persist_now(cx);
        cx.notify();
    }

    /// Write the current text to the file (Cmd/Ctrl+S). Unconditional, as in Electron.
    pub fn save(&mut self, cx: &mut Context<Self>) {
        let EditorDoc::Ready { current, .. } = &self.doc else {
            return;
        };
        if !self.is_dirty() || self.save == SaveState::Saving {
            return;
        }
        let saved = current.clone();
        let write = self
            .fs
            .write_file(&self.root, &self.path, saved.clone().into_bytes(), WriteGuard::Unconditional);
        self.save = SaveState::Saving;
        self._save = Some(cx.spawn(async move |this, cx| {
            let result = match fs_outcome(write.await) {
                Ok(WriteOutcome::Written { .. }) => Ok(()),
                Err(error) => Err(save_error_text(&error)),
                // Unreachable: the write is unconditional.
                Ok(WriteOutcome::Conflict) => Err("Failed to save: the file changed on disk".to_owned()),
            };
            match this.update(cx, |editor, cx| {
                match result {
                    Ok(()) => {
                        if let EditorDoc::Ready { original, .. } = &mut editor.doc {
                            *original = saved;
                        }
                        editor.save = SaveState::Idle;
                        editor.persist_now(cx);
                    }
                    Err(error) => editor.save = SaveState::Failed(error),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        cx.notify();
    }

    // ---- go to definition --------------------------------------------------------------

    pub fn definition(&self) -> &DefinitionLookup {
        &self.definition
    }

    /// Whether this file's language has go to definition.
    pub fn has_definitions(&self) -> bool {
        crate::definition::has_definitions(&self.path)
    }

    /// Look up the definition of the word at byte `offset` of the text as it is now: one
    /// result is gone to, several are offered to choose from.
    pub fn go_to_definition(&mut self, offset: usize, cx: &mut Context<Self>) {
        let EditorDoc::Ready { current, .. } = &self.doc else {
            return;
        };
        self._definition = None;
        self.definition = match crate::definition::begin(&self.path, current, offset) {
            Begin::Idle => DefinitionLookup::Idle,
            Begin::Unavailable { language } => DefinitionLookup::Unavailable { language },
            Begin::Search(query) => {
                let symbol = query.symbol.clone();
                let search = crate::definition::run(self.exec.clone(), self.root.clone(), query);
                let searched = symbol.clone();
                self._definition = Some(cx.spawn(async move |this, cx| {
                    let result = search.await;
                    match this.update(cx, |editor, cx| editor.on_definitions(searched, result, cx)) {
                        Ok(()) | Err(_) => {} // Err: tab closed
                    }
                }));
                DefinitionLookup::Searching { symbol }
            }
        };
        cx.notify();
    }

    fn on_definitions(&mut self, symbol: String, result: Result<Vec<DefinitionLocation>, ExecError>, cx: &mut Context<Self>) {
        self.definition = match crate::definition::settle(symbol, result) {
            Settled::Go(location) => {
                self.go_to_location(&location, cx);
                DefinitionLookup::Idle
            }
            Settled::Show(lookup) => lookup,
        };
        cx.notify();
    }

    /// Open one of several candidates.
    pub fn choose_definition(&mut self, index: usize, cx: &mut Context<Self>) {
        let DefinitionLookup::Choose { locations, .. } = &self.definition else {
            return;
        };
        let Some(location) = locations.get(index).cloned() else {
            return;
        };
        self.definition = DefinitionLookup::Idle;
        self.go_to_location(&location, cx);
        cx.notify();
    }

    pub fn dismiss_definition(&mut self, cx: &mut Context<Self>) {
        self.definition = DefinitionLookup::Idle;
        self._definition = None;
        cx.notify();
    }

    /// A definition in this file moves the editor there, as the Files viewer does; one in
    /// another file opens in a new Editor tab at its line (`openFileAtLine`).
    fn go_to_location(&mut self, location: &DefinitionLocation, cx: &mut Context<Self>) {
        let line = location.line_number as usize;
        if search::to_workspace_relative(&self.root, &self.path) == location.file_path {
            self.scroll_to_line = Some(line);
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let path = search::to_workspace_absolute(&self.root, &location.file_path);
        workspace.update(cx, |w, cx| w.open_in_editor_at(&path, line, cx));
    }

    /// Open a link Cmd/Ctrl+clicked in the file ([`WorkspaceStore::open_link`]).
    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        workspace.update(cx, |w, cx| w.open_link(url, cx));
    }

    /// The texts of the other files open in the workspace's Editor tabs and Files viewers that
    /// are of this file's language, whose words are suggested too
    /// ([`WorkspaceStore::open_texts`]); `me` is this editor's entity.
    pub fn other_open_texts(&self, me: gpui_kit::EntityId, cx: &gpui_kit::App) -> Vec<String> {
        match self.workspace.upgrade() {
            Some(workspace) => workspace.read(cx).open_texts(self.language(), me, cx),
            None => Vec::new(),
        }
    }

    /// The Cmd/Ctrl+hover underline for this file.
    pub fn definition_probe(&self) -> DefinitionProbe {
        DefinitionProbe::new(self.exec.clone(), self.root.clone(), self.path.clone())
    }

    fn persist_now(&mut self, cx: &mut Context<Self>) {
        if let EditorDoc::Ready { current, .. } = &self.doc
            && self.written.back().is_none_or(|(text, _)| text != current)
        {
            if self.written.len() == WRITTEN_KEPT {
                self.written.pop_front();
            }
            self.written.push_back((current.clone(), cx.background_executor().now()));
        }
        let state = match &self.doc {
            EditorDoc::Loading => json!({ "status": "loading", "filePath": self.path }),
            EditorDoc::Error(error) => json!({ "status": "error", "filePath": self.path, "error": error }),
            EditorDoc::Ready { original, current, language } => json!({
                "status": "ready",
                "filePath": self.path,
                "originalContent": original,
                "currentContent": current,
                "language": language,
                "isDirty": original != current,
                "viewMode": self.view_mode,
            }),
        };
        cx.emit(TextEditorEvent::StateChanged(state));
    }
}

/// The alert a failed save shows: the daemon's refusal, or the call failing outright (a
/// rejected IPC call in Electron).
fn save_error_text(error: &FsError) -> String {
    match error {
        FsError::Daemon(reason) => format!("Failed to save: {reason}"),
        FsError::Api(error) => format!("Error saving file: {error}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_save_fails_in_electrons_words() {
        assert_eq!(save_error_text(&FsError::Daemon("Permission denied".into())), "Failed to save: Permission denied");
        let deadline = treeterm_core::daemon::ApiError::DeadlineExceeded(Duration::from_secs(30));
        assert_eq!(
            save_error_text(&FsError::Api(deadline)),
            "Error saving file: 4 DEADLINE_EXCEEDED: Deadline exceeded after 30.000s"
        );
    }

    #[test]
    fn the_title_is_the_file_name_marked_while_unsaved() {
        assert_eq!(tab_title("/w/src/main.rs", false), "main.rs");
        assert_eq!(tab_title("/w/src/main.rs", true), "main.rs \u{2022}");
        assert_eq!(tab_title_of_state(&json!({ "filePath": "/w/a.md", "isDirty": true })).as_deref(), Some("a.md \u{2022}"));
        assert_eq!(tab_title_of_state(&json!({ "status": "loading", "filePath": "/w/a.md" })).as_deref(), Some("a.md"));
        assert_eq!(tab_title_of_state(&json!({ "status": "loading" })), None);
    }
}
