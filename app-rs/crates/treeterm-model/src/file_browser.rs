//! The Files tab: a lazily loaded directory tree and the selected file. Port of the
//! non-view half of `FilesystemBrowser.tsx` / `FileTree.tsx`.
//!
//! Tab state stays compatible with Electron's `FilesystemState`: `selectedPath` (absolute)
//! and `expandedDirs` (absolute directory paths, in the order they were opened).

use std::collections::HashMap;

use gpui_kit::{Context, EventEmitter, Task};
use serde_json::{Value, json};
use treeterm_core::fs::{FsClient, ReadOutcome};
use treeterm_proto::treeterm::FileEntry;

/// Files bigger than this are not shown until the user asks (Electron's read limit).
pub const MAX_VIEW_BYTES: u64 = 2 * 1024 * 1024;

/// A directory's entries, read on demand.
#[derive(Debug, Clone)]
pub enum Listing {
    Loading,
    Loaded(Vec<FileEntry>),
    Failed(String),
}

/// What the viewer shows.
#[derive(Debug, Clone)]
pub enum FileView {
    Nothing,
    Loading(String),
    Text { path: String, text: String },
    /// Shown as an image (by extension).
    Image { path: String, bytes: std::sync::Arc<Vec<u8>>, kind: ImageKind },
    /// Shown in a PDF viewer.
    Pdf { path: String, bytes: std::sync::Arc<Vec<u8>> },
    /// Not valid UTF-8 (images, archives, ...).
    Binary { path: String, size: u64 },
    TooLarge { path: String, size: u64 },
    Failed { path: String, error: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageKind {
    Png,
    Jpeg,
    Gif,
    Webp,
    Svg,
    Bmp,
}

/// How a file is shown, by extension (`isImagePath` / PDF detection in Electron).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presentation {
    Text,
    Image(ImageKind),
    Pdf,
}

fn presentation_of(path: &str) -> Presentation {
    let ext = path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    match ext.as_str() {
        "png" => Presentation::Image(ImageKind::Png),
        "jpg" | "jpeg" => Presentation::Image(ImageKind::Jpeg),
        "gif" => Presentation::Image(ImageKind::Gif),
        "webp" => Presentation::Image(ImageKind::Webp),
        "svg" => Presentation::Image(ImageKind::Svg),
        "bmp" => Presentation::Image(ImageKind::Bmp),
        "pdf" => Presentation::Pdf,
        _ => Presentation::Text,
    }
}

/// One visible tree row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub path: String,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileBrowserEvent {
    /// Selection or expansion changed: persist it in the tab state.
    StateChanged(Value),
}

impl EventEmitter<FileBrowserEvent> for FileBrowser {}

pub struct FileBrowser {
    fs: FsClient,
    root: String,
    listings: HashMap<String, Listing>,
    expanded: Vec<String>,
    selected: Option<String>,
    view: FileView,
    loads: HashMap<String, Task<()>>,
    _read: Option<Task<()>>,
}

/// Directories first, then names, case-insensitively (as the Electron tree sorts).
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

impl FileBrowser {
    /// `state` is the tab's persisted `FilesystemState`.
    pub fn new(fs: FsClient, root: String, state: &Value, cx: &mut Context<Self>) -> Self {
        let expanded: Vec<String> = state
            .get("expandedDirs")
            .and_then(Value::as_array)
            .map(|dirs| dirs.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let selected = state.get("selectedPath").and_then(Value::as_str).map(str::to_owned);
        let mut browser = Self {
            fs,
            root: root.clone(),
            listings: HashMap::new(),
            expanded: expanded.clone(),
            selected: None,
            view: FileView::Nothing,
            loads: HashMap::new(),
            _read: None,
        };
        browser.load(root, cx);
        for dir in expanded {
            browser.load(dir, cx);
        }
        if let Some(path) = selected {
            browser.open(&path, false, cx);
        }
        browser
    }

    pub fn root(&self) -> &str {
        &self.root
    }

    pub fn view(&self) -> &FileView {
        &self.view
    }

    pub fn selected(&self) -> Option<&str> {
        self.selected.as_deref()
    }

    /// Re-read every loaded directory (files changed on disk).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let dirs: Vec<String> = self.listings.keys().cloned().collect();
        for dir in dirs {
            self.load(dir, cx);
        }
    }

    fn load(&mut self, dir: String, cx: &mut Context<Self>) {
        self.listings.entry(dir.clone()).or_insert(Listing::Loading);
        let read = self.fs.read_directory(&self.root, &dir);
        let key = dir.clone();
        let task = cx.spawn(async move |this, cx| {
            let listing = match read.await {
                Ok(Ok(mut contents)) => {
                    sort_entries(&mut contents.entries);
                    Listing::Loaded(contents.entries)
                }
                Ok(Err(error)) => Listing::Failed(error.to_string()),
                Err(lost) => Listing::Failed(lost.to_string()),
            };
            match this.update(cx, |browser, cx| {
                browser.listings.insert(key.clone(), listing);
                browser.loads.remove(&key);
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        });
        self.loads.insert(dir, task);
        cx.notify();
    }

    /// Visible rows: the root's entries, with expanded directories' children under them.
    pub fn rows(&self) -> Vec<FileRow> {
        let mut rows = Vec::new();
        self.push_rows(&self.root, 0, &mut rows);
        rows
    }

    /// The root's own listing state, for "Loading…" / errors at the top level.
    pub fn root_listing(&self) -> Option<&Listing> {
        self.listings.get(&self.root)
    }

    fn push_rows(&self, dir: &str, depth: usize, rows: &mut Vec<FileRow>) {
        let Some(Listing::Loaded(entries)) = self.listings.get(dir) else {
            return;
        };
        for entry in entries {
            let expanded = entry.is_directory && self.expanded.contains(&entry.path);
            rows.push(FileRow {
                path: entry.path.clone(),
                name: entry.name.clone(),
                depth,
                is_dir: entry.is_directory,
                expanded,
            });
            if expanded {
                self.push_rows(&entry.path, depth + 1, rows);
            }
        }
    }

    pub fn toggle_dir(&mut self, path: &str, cx: &mut Context<Self>) {
        if let Some(index) = self.expanded.iter().position(|d| d == path) {
            self.expanded.remove(index);
        } else {
            self.expanded.push(path.to_owned());
            if !matches!(self.listings.get(path), Some(Listing::Loaded(_))) {
                self.load(path.to_owned(), cx);
            }
        }
        self.persist(cx);
        cx.notify();
    }

    /// Show a file. `allow_large` reads past [`MAX_VIEW_BYTES`] ("Open anyway").
    pub fn open(&mut self, path: &str, allow_large: bool, cx: &mut Context<Self>) {
        self.selected = Some(path.to_owned());
        self.view = FileView::Loading(path.to_owned());
        let budget = if allow_large { u64::MAX } else { MAX_VIEW_BYTES };
        let read = self.fs.read_file(&self.root, path, budget);
        let owned = path.to_owned();
        let presentation = presentation_of(path);
        self._read = Some(cx.spawn(async move |this, cx| {
            let view = match read.await {
                Ok(Ok(ReadOutcome::Read(file))) => match presentation {
                    Presentation::Image(kind) => FileView::Image {
                        path: owned.clone(),
                        bytes: std::sync::Arc::new(file.content),
                        kind,
                    },
                    Presentation::Pdf => FileView::Pdf {
                        path: owned.clone(),
                        bytes: std::sync::Arc::new(file.content),
                    },
                    Presentation::Text => match String::from_utf8(file.content) {
                        Ok(text) => FileView::Text { path: owned.clone(), text },
                        Err(bytes) => FileView::Binary {
                            path: owned.clone(),
                            size: bytes.into_bytes().len() as u64,
                        },
                    },
                },
                Ok(Ok(ReadOutcome::TooLarge { size })) => FileView::TooLarge { path: owned.clone(), size },
                Ok(Err(error)) => FileView::Failed { path: owned.clone(), error: error.to_string() },
                Err(lost) => FileView::Failed { path: owned.clone(), error: lost.to_string() },
            };
            match this.update(cx, |browser, cx| {
                // A later selection wins.
                if browser.selected.as_deref() == Some(owned.as_str()) {
                    browser.view = view;
                    cx.notify();
                }
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        self.persist(cx);
        cx.notify();
    }

    fn persist(&self, cx: &mut Context<Self>) {
        cx.emit(FileBrowserEvent::StateChanged(json!({
            "selectedPath": self.selected,
            "expandedDirs": self.expanded,
        })));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, dir: bool) -> FileEntry {
        FileEntry {
            name: name.to_owned(),
            path: format!("/w/{name}"),
            relative_path: name.to_owned(),
            is_directory: dir,
            size: None,
            modified_time: None,
        }
    }

    #[test]
    fn files_are_presented_by_extension() {
        assert_eq!(presentation_of("/w/a.PNG"), Presentation::Image(ImageKind::Png));
        assert_eq!(presentation_of("/w/doc.pdf"), Presentation::Pdf);
        assert_eq!(presentation_of("/w/main.rs"), Presentation::Text);
    }

    #[test]
    fn directories_sort_first_then_names_case_insensitively() {
        let mut entries = vec![entry("b.txt", false), entry("Src", true), entry("a.txt", false), entry("docs", true)];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "Src", "a.txt", "b.txt"]);
    }
}
