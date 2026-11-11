//! The Editor tab: one file, edited and saved. Port of the non-view half of
//! `FileEditor.tsx`.
//!
//! The tab state keeps Electron's `EditorState` shape (`status`, `filePath`,
//! `originalContent`, `currentContent`, `language`, `isDirty`, `viewMode`), so an editor tab
//! opened in either app shows the same unsaved edits in the other.

use std::time::Duration;

use gpui_kit::{Context, EventEmitter, Task};
use serde_json::{Value, json};
use treeterm_core::fs::{FsClient, ReadOutcome, WriteGuard, WriteOutcome};

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

pub struct TextEditor {
    fs: FsClient,
    root: String,
    path: String,
    doc: EditorDoc,
    save: SaveState,
    view_mode: String,
    _load: Option<Task<()>>,
    _save: Option<Task<()>>,
    _persist: Option<Task<()>>,
}

impl TextEditor {
    /// `state` is the tab's persisted `EditorState`; a Ready state with content is used as
    /// is (unsaved edits survive), anything else loads the file.
    pub fn new(fs: FsClient, root: String, state: &Value, cx: &mut Context<Self>) -> Self {
        let text = |key: &str| state.get(key).and_then(Value::as_str).map(str::to_owned);
        let path = text("filePath").unwrap_or_default();
        let mut editor = Self {
            fs,
            root,
            path,
            doc: EditorDoc::Loading,
            save: SaveState::Idle,
            view_mode: text("viewMode").unwrap_or_else(|| "editor".to_owned()),
            _load: None,
            _save: None,
            _persist: None,
        };
        match (text("status").as_deref(), text("originalContent"), text("currentContent")) {
            (Some("ready"), Some(original), Some(current)) => {
                editor.doc = EditorDoc::Ready {
                    original,
                    current,
                    language: text("language").unwrap_or_else(|| "plaintext".to_owned()),
                };
            }
            _ if editor.path.is_empty() => editor.doc = EditorDoc::Error("No file to edit".to_owned()),
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

    pub fn is_dirty(&self) -> bool {
        match &self.doc {
            EditorDoc::Ready { original, current, .. } => original != current,
            EditorDoc::Loading | EditorDoc::Error(_) => false,
        }
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.doc = EditorDoc::Loading;
        let read = self.fs.read_file(&self.root, &self.path, u64::MAX);
        self._load = Some(cx.spawn(async move |this, cx| {
            let doc = match read.await {
                Ok(Ok(ReadOutcome::Read(file))) => match String::from_utf8(file.content) {
                    Ok(text) => EditorDoc::Ready {
                        original: text.clone(),
                        current: text,
                        language: file.header.language.clone(),
                    },
                    Err(_) => EditorDoc::Error("Binary files cannot be edited".to_owned()),
                },
                Ok(Ok(ReadOutcome::TooLarge { size })) => EditorDoc::Error(format!("File too large ({size} bytes)")),
                Ok(Err(error)) => EditorDoc::Error(error.to_string()),
                Err(lost) => EditorDoc::Error(lost.to_string()),
            };
            match this.update(cx, |editor, cx| {
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
            let result = match write.await {
                Ok(Ok(WriteOutcome::Written { .. })) => Ok(()),
                Ok(Ok(WriteOutcome::Conflict)) => Err("the file changed on disk".to_owned()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(lost) => Err(lost.to_string()),
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
                    Err(error) => editor.save = SaveState::Failed(format!("Failed to save: {error}")),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        cx.notify();
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
