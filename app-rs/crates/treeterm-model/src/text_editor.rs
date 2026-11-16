//! The Editor tab: one file, edited and saved. Port of the non-view half of
//! `FileEditor.tsx`.
//!
//! The tab state keeps Electron's `EditorState` shape (`status`, `filePath`,
//! `originalContent`, `currentContent`, `language`, `isDirty`, `viewMode`), so an editor tab
//! opened in either app shows the same unsaved edits in the other.
//!
//! Go to definition searches the text as it is, unsaved edits included
//! ([`crate::definition`]); a definition in another file opens in a new Editor tab at its
//! line (`openFileAtLine`).

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task, WeakEntity};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecApi, ExecError};
use treeterm_core::fs::{FsClient, FsError, ReadOutcome, WriteGuard, WriteOutcome};
use treeterm_core::search::{self, DefinitionLocation};

use crate::definition::{Begin, DefinitionLookup, DefinitionProbe, Settled};
use crate::file_browser::{fs_error_text, fs_outcome};
use crate::workspace_store::WorkspaceStore;

/// Edits are persisted to the tab state this long after the last keystroke.
const PERSIST_DEBOUNCE: Duration = Duration::from_millis(800);

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
    definition: DefinitionLookup,
    _load: Option<Task<()>>,
    _save: Option<Task<()>>,
    _persist: Option<Task<()>>,
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
        let text = |key: &str| state.get(key).and_then(Value::as_str).map(str::to_owned);
        let path = text("filePath").unwrap_or_default();
        let mut editor = Self {
            fs,
            exec,
            workspace,
            root,
            path,
            doc: EditorDoc::Loading,
            save: SaveState::Idle,
            view_mode: text("viewMode").unwrap_or_else(|| VIEW_MODE_EDITOR.to_owned()),
            scroll_to_line: state.get("scrollToLine").and_then(Value::as_u64).map(|line| line as usize),
            definition: DefinitionLookup::Idle,
            _load: None,
            _save: None,
            _persist: None,
            _definition: None,
        };
        match (text("status").as_deref(), text("originalContent"), text("currentContent")) {
            (Some("ready"), Some(original), Some(current)) => {
                editor.doc = EditorDoc::Ready {
                    original,
                    current,
                    language: text("language").unwrap_or_else(|| "plaintext".to_owned()),
                };
            }
            _ if editor.path.is_empty() => editor.doc = EditorDoc::Error("Invalid tab".to_owned()),
            _ => editor.load(cx),
        }
        editor
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

    /// The line to reveal once the editor shows the text, taken once.
    pub fn take_scroll_to_line(&mut self) -> Option<usize> {
        self.scroll_to_line.take()
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
        let was_dirty = self.is_dirty();
        if let EditorDoc::Ready { current, .. } = &mut self.doc {
            *current = text;
        }
        // The tab title marks unsaved edits as soon as there are some.
        if self.is_dirty() != was_dirty {
            self.persist_now(cx);
            cx.notify();
            return;
        }
        let executor = cx.background_executor().clone();
        self._persist = Some(cx.spawn(async move |this, cx| {
            executor.timer(PERSIST_DEBOUNCE).await;
            match this.update(cx, |editor, cx| editor.persist_now(cx)) {
                Ok(()) | Err(_) => {}
            }
        }));
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

    /// The Cmd/Ctrl+hover underline for this file.
    pub fn definition_probe(&self) -> DefinitionProbe {
        DefinitionProbe::new(self.exec.clone(), self.root.clone(), self.path.clone())
    }

    fn persist_now(&mut self, cx: &mut Context<Self>) {
        self._persist = None;
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
