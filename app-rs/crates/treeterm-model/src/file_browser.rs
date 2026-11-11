//! The Files tab: a lazily loaded directory tree and the selected file. Port of the
//! non-view half of `FilesystemBrowser.tsx` / `FileTree.tsx` / `FileViewer.tsx` /
//! `SearchPanel.tsx`.
//!
//! Tab state stays compatible with Electron's `FilesystemState`: `selectedPath` (absolute)
//! and `expandedDirs` (absolute directory paths, in the order they were opened). A
//! `scrollToLine` another app wrote (Electron's "open in file browser") is honoured on load.
//!
//! The search panel searches the open file ([`search::find_in_text`]) and the whole
//! worktree ([`search::repo_search`]); go to definition greps for a definition of the word
//! under the cursor ([`search::search_definition`]). Where the viewer should move is a
//! [`Reveal`] the view applies once the file's text is shown.
//!
//! Favourites are the workspace's `favouritePaths` (workspace-relative files and
//! directories); the tree stars them and lists the files they name above the tree.
//! Favourites a parent workspace lists are not inherited here (Electron's
//! `getWorkspaceFavouritePaths` walks the ancestors, which this tab cannot see).

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use serde_json::{Value, json};
use treeterm_core::exec::{ExecApi, ExecError};
use treeterm_core::files::{FavouriteFile, detect_monaco_language, normalize_favourite_path, resolve_favourite_files};
use treeterm_core::fs::{FsClient, ReadOutcome};
use treeterm_core::search::{
    self, Completion, DefinitionLocation, RepoHit, RepoSearchEvent, RepoSearchTool,
};
use treeterm_proto::treeterm::FileEntry;

use crate::workspace_store::WorkspaceStore;

/// Files bigger than this are not shown until the user asks (Electron's read limit).
pub const MAX_VIEW_BYTES: u64 = 2 * 1024 * 1024;

/// Typing restarts the repo search; it waits for a pause first (`REPO_SEARCH_DEBOUNCE_MS`).
pub const REPO_SEARCH_DEBOUNCE: Duration = Duration::from_millis(250);

/// Monaco languages Electron registers its grep definition provider for.
const DEFINITION_LANGUAGES: [&str; 16] = [
    "typescript", "javascript", "python", "rust", "go", "java", "kotlin", "ruby", "php", "c", "cpp",
    "css", "scss", "less", "html", "shell",
];

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
    /// Shown in the 3D model viewer.
    Model {
        path: String,
        bytes: std::sync::Arc<Vec<u8>>,
        format: treeterm_core::files::content_kind::ModelFormat,
    },
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
    Model(treeterm_core::files::content_kind::ModelFormat),
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
        _ => match treeterm_core::files::content_kind::model_format(path) {
            Ok(format) => Presentation::Model(format),
            Err(_not_a_model) => Presentation::Text,
        },
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

/// The worktree search (`RepoResults` in `SearchPanel.tsx`). Hits keep arriving while it runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepoResults {
    /// Nothing to search for.
    Idle,
    Searching { hits: Vec<RepoHit> },
    Done { hits: Vec<RepoHit>, tool: RepoSearchTool, completion: Completion },
    Failed { hits: Vec<RepoHit>, message: String },
}

/// The open search panel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchState {
    pub query: String,
    /// The selected match in the open file, by index into [`FileBrowser::file_matches`].
    pub active: Option<usize>,
    pub repo: RepoResults,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchPanel {
    Closed,
    Open(SearchState),
}

/// An occurrence of the query in the open file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileMatch {
    /// 1-based.
    pub line_number: usize,
    /// 1-based byte column.
    pub column: usize,
    /// Bytes of the file's text.
    pub range: Range<usize>,
    /// The whole line.
    pub preview: String,
}

/// A row of the search panel, in display order. Consecutive hits in one file share a
/// [`SearchRow::File`] heading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchRow {
    Section(&'static str),
    File(String),
    /// A match in the open file; `index` is its place in [`FileBrowser::file_matches`].
    InFile { index: usize, line_number: usize, preview: String, active: bool },
    InRepo(RepoHit),
    Note(String),
    Error(String),
}

/// Where the viewer should move: select `len` bytes from `column` of `line_number` in
/// `path`, once that file's text is shown.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reveal {
    pub path: String,
    /// 1-based.
    pub line_number: usize,
    /// 1-based byte column.
    pub column: usize,
    pub len: usize,
    /// Bumped on every request, so asking for the same place again moves there again.
    pub seq: u64,
}

/// Go to definition (`definitionProvider` in `monaco-config.ts`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DefinitionLookup {
    Idle,
    Searching { symbol: String },
    /// Several candidates: the user picks one.
    Choose { symbol: String, locations: Vec<DefinitionLocation> },
    NotFound { symbol: String },
    /// No definition search for this file's language (Electron registers none).
    Unavailable { language: &'static str },
    Failed { symbol: String, error: String },
}

/// The favourites section (`FavouriteFilesSection`): the files the favourite paths name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Favourites {
    /// The workspace has no favourite paths.
    Empty,
    Loading,
    Ready(Vec<FavouriteFile>),
    Failed(String),
}

/// A tree entry's star.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Star {
    Off,
    /// A favourite itself: clicking removes it.
    On,
    /// Inside a favourite directory: shown starred, not removable on its own.
    Covered,
}

/// `isFavouritePath`: `path` is a favourite or inside a favourite directory.
pub fn is_favourite_path(path: &str, favourites: &[String]) -> bool {
    let path = normalize_favourite_path(path);
    favourites.iter().map(|favourite| normalize_favourite_path(favourite)).any(|favourite| {
        !favourite.is_empty() && (path == favourite || path.starts_with(&format!("{favourite}/")))
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileBrowserEvent {
    /// Selection or expansion changed: persist it in the tab state.
    StateChanged(Value),
}

impl EventEmitter<FileBrowserEvent> for FileBrowser {}

pub struct FileBrowser {
    fs: FsClient,
    exec: Arc<dyn ExecApi>,
    root: String,
    listings: HashMap<String, Listing>,
    expanded: Vec<String>,
    selected: Option<String>,
    view: FileView,
    search: SearchPanel,
    definition: DefinitionLookup,
    reveal: Option<Reveal>,
    /// The workspace whose favourite paths the tree stars.
    workspace: WeakEntity<WorkspaceStore>,
    /// The favourite paths `favourites` was resolved from.
    favourite_paths: Vec<String>,
    favourites: Favourites,
    /// Reveals requested so far (each gets a new `seq`).
    reveals: u64,
    loads: HashMap<String, Task<()>>,
    _read: Option<Task<()>>,
    /// The running repo search; dropping it kills the command.
    _search: Option<Task<()>>,
    _definition: Option<Task<()>>,
    _favourites: Option<Task<()>>,
    _workspace: Subscription,
    /// Saving a copy of a file to this computer (`FileDownloadButton`).
    download: DownloadState,
    _download: Option<Task<()>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Idle,
    Downloading,
    Failed(String),
}

/// Directories first, then names, case-insensitively (as the Electron tree sorts).
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        b.is_directory
            .cmp(&a.is_directory)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

/// Each line of `text` with the byte offset it starts at.
fn lines_with_offsets(text: &str) -> Vec<(usize, &str)> {
    let mut offset = 0;
    text.split('\n')
        .map(|line| {
            let start = offset;
            offset += line.len() + 1;
            (start, line)
        })
        .collect()
}

/// The bytes `len` long from 1-based `column` of 1-based `line_number`, kept within the
/// line; `None` past the last line.
pub fn line_range(text: &str, line_number: usize, column: usize, len: usize) -> Option<Range<usize>> {
    let (start, line) = *lines_with_offsets(text).get(line_number.checked_sub(1)?)?;
    let from = column.saturating_sub(1).min(line.len());
    let to = (from + len).min(line.len());
    Some(start + from..start + to)
}

/// `countLabel`: the search panel's match counter.
fn count_label(query: &str, active: Option<usize>, total: usize, pending: bool) -> String {
    if query.is_empty() {
        return String::new();
    }
    if pending && total == 0 {
        return "Searching…".to_owned();
    }
    let count = match active {
        Some(index) => format!("{}/{total}", index + 1),
        None => format!("{total} found"),
    };
    if pending { format!("{count}…") } else { count }
}

/// Consecutive hits of one file under one heading (`groupByFile`).
fn push_repo_rows(rows: &mut Vec<SearchRow>, hits: impl IntoIterator<Item = RepoHit>) -> usize {
    let mut shown = 0;
    let mut last_file: Option<String> = None;
    for hit in hits {
        if last_file.as_deref() != Some(hit.file_path.as_str()) {
            rows.push(SearchRow::File(hit.file_path.clone()));
            last_file = Some(hit.file_path.clone());
        }
        rows.push(SearchRow::InRepo(hit));
        shown += 1;
    }
    shown
}

impl FileBrowser {
    /// `state` is the tab's persisted `FilesystemState`; `favourite_paths` the workspace's
    /// (it may be mid-update, so it is not read here).
    pub fn new(
        fs: FsClient,
        exec: Arc<dyn ExecApi>,
        root: String,
        state: &Value,
        workspace: &Entity<WorkspaceStore>,
        favourite_paths: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let expanded: Vec<String> = state
            .get("expandedDirs")
            .and_then(Value::as_array)
            .map(|dirs| dirs.iter().filter_map(Value::as_str).map(str::to_owned).collect())
            .unwrap_or_default();
        let selected = state.get("selectedPath").and_then(Value::as_str).map(str::to_owned);
        let scroll_to_line = state.get("scrollToLine").and_then(Value::as_u64);
        let observe = cx.observe(workspace, |browser, workspace, cx| {
            let paths = workspace.read(cx).favourite_paths().to_vec();
            browser.sync_favourites(paths, cx);
        });
        let mut browser = Self {
            fs,
            exec,
            root: root.clone(),
            listings: HashMap::new(),
            expanded: expanded.clone(),
            selected: None,
            view: FileView::Nothing,
            search: SearchPanel::Closed,
            definition: DefinitionLookup::Idle,
            reveal: None,
            workspace: workspace.downgrade(),
            favourite_paths,
            favourites: Favourites::Empty,
            reveals: 0,
            loads: HashMap::new(),
            _read: None,
            download: DownloadState::Idle,
            _download: None,
            _search: None,
            _definition: None,
            _favourites: None,
            _workspace: observe,
        };
        browser.resolve_favourites(cx);
        browser.load(root, cx);
        for dir in expanded {
            browser.load(dir, cx);
        }
        if let Some(path) = selected {
            browser.open(&path, false, cx);
            if let Some(line) = scroll_to_line {
                browser.reveal_at(&path, line as usize, 1, 0);
            }
        }
        browser
    }

    pub fn download_state(&self) -> &DownloadState {
        &self.download
    }

    /// Save `path` (on the daemon's host) to `destination` on this computer.
    pub fn download(&mut self, path: &str, destination: std::path::PathBuf, cx: &mut Context<Self>) {
        self.download = DownloadState::Downloading;
        let read = self.fs.read_file(&self.root, path, u64::MAX);
        let executor = cx.background_executor().clone();
        self._download = Some(cx.spawn(async move |this, cx| {
            let result = match read.await {
                Ok(Ok(ReadOutcome::Read(file))) => {
                    let content = file.content;
                    executor
                        .spawn(async move { std::fs::write(&destination, content).map_err(|e| format!("{}: {e}", destination.display())) })
                        .await
                }
                Ok(Ok(ReadOutcome::TooLarge { size })) => Err(format!("File too large ({size} bytes)")),
                Ok(Err(error)) => Err(error.to_string()),
                Err(lost) => Err(lost.to_string()),
            };
            match this.update(cx, |browser, cx| {
                browser.download = match result {
                    Ok(()) => DownloadState::Idle,
                    Err(error) => DownloadState::Failed(error),
                };
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        cx.notify();
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

    pub fn search(&self) -> &SearchPanel {
        &self.search
    }

    pub fn definition(&self) -> &DefinitionLookup {
        &self.definition
    }

    pub fn reveal(&self) -> Option<&Reveal> {
        self.reveal.as_ref()
    }

    /// The bytes to select for the pending [`Reveal`], once its file's text is shown.
    pub fn reveal_range(&self) -> Option<(u64, Range<usize>)> {
        let reveal = self.reveal.as_ref()?;
        let FileView::Text { path, text } = &self.view else {
            return None;
        };
        if *path != reveal.path {
            return None;
        }
        let range = line_range(text, reveal.line_number, reveal.column, reveal.len)?;
        Some((reveal.seq, range))
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
        self.reveal = None;
        if let SearchPanel::Open(search) = &mut self.search {
            search.active = None;
        }
        let presentation = presentation_of(path);
        // PDFs and models are routinely tens of megabytes (`fileLimits.ts`).
        let budget = match (allow_large, presentation) {
            (true, _) => u64::MAX,
            (false, Presentation::Pdf | Presentation::Model(_)) => treeterm_core::files::max_read_bytes_for(path),
            (false, Presentation::Text | Presentation::Image(_)) => MAX_VIEW_BYTES,
        };
        let read = self.fs.read_file(&self.root, path, budget);
        let owned = path.to_owned();
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
                    Presentation::Model(format) => FileView::Model {
                        path: owned.clone(),
                        bytes: std::sync::Arc::new(file.content),
                        format,
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

    /// Open a workspace-relative location (a search hit or a definition) and move to it.
    pub fn open_location(&mut self, relative_path: &str, line_number: usize, column: usize, len: usize, cx: &mut Context<Self>) {
        let path = search::to_workspace_absolute(&self.root, relative_path);
        if self.selected.as_deref() != Some(path.as_str()) {
            self.open(&path, false, cx);
        }
        self.reveal_at(&path, line_number, column, len);
        cx.notify();
    }

    fn reveal_at(&mut self, path: &str, line_number: usize, column: usize, len: usize) {
        self.reveals += 1;
        self.reveal = Some(Reveal { path: path.to_owned(), line_number, column, len, seq: self.reveals });
    }

    fn persist(&self, cx: &mut Context<Self>) {
        cx.emit(FileBrowserEvent::StateChanged(json!({
            "selectedPath": self.selected,
            "expandedDirs": self.expanded,
        })));
    }

    // ---- favourites --------------------------------------------------------------------

    pub fn favourites(&self) -> &Favourites {
        &self.favourites
    }

    /// The workspace changed: re-resolve the favourites section if its favourite paths did.
    fn sync_favourites(&mut self, paths: Vec<String>, cx: &mut Context<Self>) {
        if paths != self.favourite_paths {
            self.favourite_paths = paths;
            self.resolve_favourites(cx);
        }
    }

    /// Read the files the favourite paths name (also "Retry" after a failure).
    pub fn resolve_favourites(&mut self, cx: &mut Context<Self>) {
        if self.favourite_paths.is_empty() {
            self.favourites = Favourites::Empty;
            self._favourites = None;
            cx.notify();
            return;
        }
        self.favourites = Favourites::Loading;
        let (fs, root, paths) = (self.fs.clone(), self.root.clone(), self.favourite_paths.clone());
        self._favourites = Some(cx.spawn(async move |this, cx| {
            let favourites = match resolve_favourite_files(&root, &paths, &fs).await {
                Ok(files) => Favourites::Ready(files),
                Err(error) => Favourites::Failed(error.to_string()),
            };
            match this.update(cx, |browser, cx| {
                browser.favourites = favourites;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// The star of a tree entry (absolute path).
    pub fn star(&self, path: &str, cx: &App) -> Star {
        let Some(workspace) = self.workspace.upgrade() else {
            return Star::Off;
        };
        let favourites = workspace.read(cx).favourite_paths();
        let relative = normalize_favourite_path(search::to_workspace_relative(&self.root, path));
        if favourites.iter().any(|favourite| normalize_favourite_path(favourite) == relative) {
            Star::On
        } else if is_favourite_path(&relative, favourites) {
            Star::Covered
        } else {
            Star::Off
        }
    }

    /// Star or unstar a tree entry (absolute path).
    pub fn toggle_favourite(&mut self, path: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let relative = search::to_workspace_relative(&self.root, path).to_owned();
        match self.star(path, cx) {
            Star::Off => workspace.update(cx, |w, cx| w.add_favourite_path(&relative, cx)),
            Star::On => workspace.update(cx, |w, cx| w.remove_favourite_path(&relative, cx)),
            Star::Covered => {}
        }
    }

    // ---- search panel ------------------------------------------------------------------

    /// The open file relative to the workspace, as search hits name files.
    fn selected_relative(&self) -> Option<&str> {
        self.selected.as_deref().map(|path| search::to_workspace_relative(&self.root, path))
    }

    /// Open the panel (Cmd/Ctrl+F, or the context menu's "Search"). A single-line
    /// `selection` becomes the query; otherwise, with a `cursor` (the context menu), the word
    /// under it. With neither, an open panel keeps its query.
    pub fn open_search(&mut self, selection: &str, cursor: Option<usize>, cx: &mut Context<Self>) {
        let seed = if !selection.is_empty() && !selection.contains('\n') {
            Some(selection.to_owned())
        } else {
            match (&self.view, cursor) {
                (FileView::Text { text, .. }, Some(offset)) => {
                    Some(search::word_at(text, offset)).filter(|word| !word.is_empty()).map(str::to_owned)
                }
                (_, None)
                | (
                    FileView::Nothing
                    | FileView::Loading(_)
                    | FileView::Image { .. }
                    | FileView::Pdf { .. }
                    | FileView::Model { .. }
                    | FileView::Binary { .. }
                    | FileView::TooLarge { .. }
                    | FileView::Failed { .. },
                    Some(_),
                ) => None,
            }
        };
        match (&self.search, seed) {
            (SearchPanel::Open(_), None) => {}
            (SearchPanel::Open(search), Some(query)) if search.query == query => {}
            (SearchPanel::Closed, None) => {
                self.search = SearchPanel::Open(SearchState {
                    query: String::new(),
                    active: None,
                    repo: RepoResults::Idle,
                });
            }
            (SearchPanel::Open(_) | SearchPanel::Closed, Some(query)) => {
                self.search = SearchPanel::Open(SearchState { query, active: None, repo: RepoResults::Idle });
                self.start_repo_search(cx);
            }
        }
        cx.notify();
    }

    /// Close the panel; a running repo search is cancelled.
    pub fn close_search(&mut self, cx: &mut Context<Self>) {
        self.search = SearchPanel::Closed;
        self._search = None;
        cx.notify();
    }

    /// The query changed: matches start over and the repo search restarts after a pause.
    pub fn set_query(&mut self, query: &str, cx: &mut Context<Self>) {
        let SearchPanel::Open(search) = &mut self.search else {
            return;
        };
        if search.query == query {
            return;
        }
        search.query = query.to_owned();
        search.active = None;
        self.start_repo_search(cx);
        cx.notify();
    }

    fn start_repo_search(&mut self, cx: &mut Context<Self>) {
        let SearchPanel::Open(search) = &mut self.search else {
            return;
        };
        if search.query.is_empty() {
            search.repo = RepoResults::Idle;
            self._search = None;
            return;
        }
        search.repo = RepoResults::Searching { hits: Vec::new() };
        // The command starts on the first poll, after the pause.
        let events = search::repo_search(self.exec.clone(), &self.root, &search.query);
        let pause = cx.background_executor().timer(REPO_SEARCH_DEBOUNCE);
        self._search = Some(cx.spawn(async move |this, cx| {
            pause.await;
            let mut events = std::pin::pin!(events);
            while let Some(event) = events.next().await {
                if this.update(cx, |browser, cx| browser.apply_repo_event(event, cx)).is_err() {
                    return; // tab closed
                }
            }
        }));
    }

    fn apply_repo_event(&mut self, event: RepoSearchEvent, cx: &mut Context<Self>) {
        let SearchPanel::Open(search) = &mut self.search else {
            return;
        };
        let RepoResults::Searching { hits } = &mut search.repo else {
            return; // the search ended; a replaced one was dropped with its task
        };
        match event {
            RepoSearchEvent::Hits(more) => hits.extend(more),
            RepoSearchEvent::Done { tool, completion } => {
                search.repo = RepoResults::Done { hits: std::mem::take(hits), tool, completion };
            }
            RepoSearchEvent::Failed(error) => {
                search.repo = RepoResults::Failed { hits: std::mem::take(hits), message: error.to_string() };
            }
        }
        cx.notify();
    }

    /// Every occurrence of the query in the open file; none while the panel is closed or the
    /// file is not text.
    pub fn file_matches(&self) -> Vec<FileMatch> {
        let (SearchPanel::Open(search), FileView::Text { text, .. }) = (&self.search, &self.view) else {
            return Vec::new();
        };
        let hits = search::find_in_text(text, &search.query);
        if hits.is_empty() {
            return Vec::new();
        }
        let lines = lines_with_offsets(text);
        hits.into_iter()
            .filter_map(|hit| {
                let (start, line) = *lines.get(hit.line_number - 1)?;
                let from = (hit.column - 1).min(line.len());
                let to = (from + search.query.len()).min(line.len());
                Some(FileMatch {
                    line_number: hit.line_number,
                    column: hit.column,
                    range: start + from..start + to,
                    preview: line.to_owned(),
                })
            })
            .collect()
    }

    /// Whether the open file is still loading, so its matches may still come.
    fn file_pending(&self) -> bool {
        match &self.view {
            FileView::Loading(_) => true,
            FileView::Nothing
            | FileView::Text { .. }
            | FileView::Image { .. }
            | FileView::Pdf { .. }
            | FileView::Model { .. }
            | FileView::Binary { .. }
            | FileView::TooLarge { .. }
            | FileView::Failed { .. } => false,
        }
    }

    /// The panel's counter: `3/12`, `12 found`, `Searching…`.
    pub fn match_count_label(&self) -> String {
        let SearchPanel::Open(search) = &self.search else {
            return String::new();
        };
        count_label(&search.query, search.active, self.file_matches().len(), self.file_pending())
    }

    /// The panel's results: matches in the open file, then in the rest of the repo.
    pub fn search_rows(&self) -> Vec<SearchRow> {
        let SearchPanel::Open(search) = &self.search else {
            return Vec::new();
        };
        if search.query.is_empty() {
            return Vec::new();
        }
        let mut rows = Vec::new();
        let current = self.selected_relative();
        if let Some(current) = current {
            rows.push(SearchRow::Section("In this file"));
            let matches = self.file_matches();
            if matches.is_empty() && !self.file_pending() {
                rows.push(SearchRow::Note("No matches".to_owned()));
            }
            if !matches.is_empty() {
                rows.push(SearchRow::File(current.to_owned()));
            }
            for (index, found) in matches.into_iter().enumerate() {
                rows.push(SearchRow::InFile {
                    index,
                    line_number: found.line_number,
                    preview: found.preview,
                    active: search.active == Some(index),
                });
            }
        }
        rows.push(SearchRow::Section("In repo"));
        // The open file's matches are listed above.
        let elsewhere = |hits: &[RepoHit]| -> Vec<RepoHit> {
            hits.iter().filter(|hit| Some(hit.file_path.as_str()) != current).cloned().collect()
        };
        match &search.repo {
            RepoResults::Idle => {}
            RepoResults::Searching { hits } => {
                push_repo_rows(&mut rows, elsewhere(hits));
                rows.push(SearchRow::Note("Searching repo…".to_owned()));
            }
            RepoResults::Done { hits, tool, completion } => {
                let shown = push_repo_rows(&mut rows, elsewhere(hits));
                let found = if shown == 0 { "No matches".to_owned() } else { format!("{shown} found") };
                let truncated = match completion {
                    Completion::Complete => "",
                    Completion::Truncated => " (stopped at the result limit)",
                };
                rows.push(SearchRow::Note(format!("{found}{truncated} · {}", tool.as_str())));
            }
            RepoResults::Failed { hits, message } => {
                push_repo_rows(&mut rows, elsewhere(hits));
                rows.push(SearchRow::Error(format!("Search failed: {message}")));
            }
        }
        rows
    }

    /// Select a match in the open file and move the viewer to it.
    pub fn select_match(&mut self, index: usize, cx: &mut Context<Self>) {
        let matches = self.file_matches();
        let (Some(found), Some(path)) = (matches.get(index), self.selected.clone()) else {
            return;
        };
        let SearchPanel::Open(search) = &mut self.search else {
            return;
        };
        search.active = Some(index);
        let len = found.range.len();
        self.reveal_at(&path, found.line_number, found.column, len);
        cx.notify();
    }

    /// Enter / Shift+Enter: the next or previous match, wrapping around.
    pub fn step_match(&mut self, forward: bool, cx: &mut Context<Self>) {
        let total = self.file_matches().len();
        let SearchPanel::Open(search) = &self.search else {
            return;
        };
        if total == 0 {
            return;
        }
        let next = match (search.active, forward) {
            (None, true) => 0,
            (None, false) => total - 1,
            (Some(index), true) => (index + 1) % total,
            (Some(index), false) => (index + total - 1) % total,
        };
        self.select_match(next, cx);
    }

    /// Open a repo hit, selecting the match.
    pub fn open_repo_hit(&mut self, hit: &RepoHit, cx: &mut Context<Self>) {
        let len = match &self.search {
            SearchPanel::Open(search) => search.query.len(),
            SearchPanel::Closed => 0,
        };
        self.open_location(&hit.file_path, hit.line_number as usize, hit.column as usize, len, cx);
    }

    // ---- go to definition --------------------------------------------------------------

    /// Look up the definition of the word at byte `offset` of the open file: one result
    /// opens, several are offered to choose from.
    pub fn go_to_definition(&mut self, offset: usize, cx: &mut Context<Self>) {
        let FileView::Text { path, text } = &self.view else {
            return;
        };
        let symbol = search::word_at(text, offset).to_owned();
        let language = detect_monaco_language(path);
        self._definition = None;
        if symbol.is_empty() {
            self.definition = DefinitionLookup::Idle;
            cx.notify();
            return;
        }
        if !DEFINITION_LANGUAGES.contains(&language) {
            self.definition = DefinitionLookup::Unavailable { language };
            cx.notify();
            return;
        }
        self.definition = DefinitionLookup::Searching { symbol: symbol.clone() };
        let (exec, root) = (self.exec.clone(), self.root.clone());
        self._definition = Some(cx.spawn(async move |this, cx| {
            let result = search::search_definition(exec.as_ref(), &root, &symbol, language).await;
            match this.update(cx, |browser, cx| browser.on_definitions(symbol, result, cx)) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    fn on_definitions(&mut self, symbol: String, result: Result<Vec<DefinitionLocation>, ExecError>, cx: &mut Context<Self>) {
        self.definition = match result {
            Err(error) => DefinitionLookup::Failed { symbol, error: error.to_string() },
            Ok(locations) => match <[DefinitionLocation; 1]>::try_from(locations) {
                Ok([location]) => {
                    self.open_location(&location.file_path, location.line_number as usize, 1, 0, cx);
                    DefinitionLookup::Idle
                }
                Err(locations) if locations.is_empty() => DefinitionLookup::NotFound { symbol },
                Err(locations) => DefinitionLookup::Choose { symbol, locations },
            },
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
        self.open_location(&location.file_path, location.line_number as usize, 1, 0, cx);
    }

    pub fn dismiss_definition(&mut self, cx: &mut Context<Self>) {
        self.definition = DefinitionLookup::Idle;
        self._definition = None;
        cx.notify();
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

    fn hit(file: &str, line: u64) -> RepoHit {
        RepoHit { file_path: file.to_owned(), line_number: line, column: 1, preview: format!("{file}:{line}") }
    }

    #[test]
    fn files_are_presented_by_extension() {
        assert_eq!(presentation_of("/w/a.PNG"), Presentation::Image(ImageKind::Png));
        assert_eq!(presentation_of("/w/doc.pdf"), Presentation::Pdf);
        assert_eq!(
            presentation_of("/w/parts/Bracket.STL"),
            Presentation::Model(treeterm_core::files::content_kind::ModelFormat::Stl)
        );
        assert_eq!(presentation_of("/w/main.rs"), Presentation::Text);
    }

    #[test]
    fn directories_sort_first_then_names_case_insensitively() {
        let mut entries = vec![entry("b.txt", false), entry("Src", true), entry("a.txt", false), entry("docs", true)];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["docs", "Src", "a.txt", "b.txt"]);
    }

    #[test]
    fn line_ranges_stay_within_their_line() {
        let text = "one\ntwo words\nthree";
        assert_eq!(line_range(text, 1, 1, 3), Some(0..3));
        assert_eq!(line_range(text, 2, 5, 5), Some(8..13));
        assert_eq!(line_range(text, 2, 5, 50), Some(8..13));
        assert_eq!(line_range(text, 3, 1, 0), Some(14..14));
        assert_eq!(line_range(text, 4, 1, 0), None);
        assert_eq!(line_range(text, 0, 1, 0), None);
    }

    #[test]
    fn the_counter_follows_electrons_count_label() {
        assert_eq!(count_label("", None, 3, false), "");
        assert_eq!(count_label("x", None, 0, true), "Searching…");
        assert_eq!(count_label("x", None, 3, false), "3 found");
        assert_eq!(count_label("x", Some(1), 3, false), "2/3");
        assert_eq!(count_label("x", Some(0), 3, true), "1/3…");
    }

    #[test]
    fn repo_hits_are_grouped_by_consecutive_file() {
        let mut rows = Vec::new();
        let shown = push_repo_rows(&mut rows, vec![hit("a.rs", 1), hit("a.rs", 4), hit("b.rs", 2), hit("a.rs", 9)]);
        assert_eq!(shown, 4);
        assert_eq!(
            rows,
            vec![
                SearchRow::File("a.rs".into()),
                SearchRow::InRepo(hit("a.rs", 1)),
                SearchRow::InRepo(hit("a.rs", 4)),
                SearchRow::File("b.rs".into()),
                SearchRow::InRepo(hit("b.rs", 2)),
                SearchRow::File("a.rs".into()),
                SearchRow::InRepo(hit("a.rs", 9)),
            ]
        );
    }

    #[test]
    fn favourite_paths_cover_their_directories() {
        let favourites = vec!["src".to_owned(), "./docs/".to_owned()];
        assert!(is_favourite_path("src", &favourites));
        assert!(is_favourite_path("src/lib.rs", &favourites));
        assert!(is_favourite_path("/docs/a.md", &favourites));
        assert!(!is_favourite_path("srcs/lib.rs", &favourites));
        assert!(!is_favourite_path("README.md", &favourites));
        assert!(!is_favourite_path("anything", &["".to_owned()]));
    }

    #[test]
    fn lines_know_where_they_start() {
        assert_eq!(lines_with_offsets("ab\n\ncd"), vec![(0, "ab"), (3, ""), (4, "cd")]);
    }
}
