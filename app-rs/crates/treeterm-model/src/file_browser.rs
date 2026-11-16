//! The Files tab: a lazily loaded directory tree and the selected file. Port of the
//! non-view half of `FilesystemBrowser.tsx` / `FileTree.tsx` / `FileViewer.tsx` /
//! `SearchPanel.tsx`.
//!
//! Tab state stays compatible with Electron's `FilesystemState`: `selectedPath` (absolute)
//! and `expandedDirs` (absolute directory paths, in the order they were opened). A
//! `scrollToLine` another app wrote (Electron's "open in file browser") is honoured on load,
//! then removed once the viewer has moved there. The viewer's scroll offset is saved as
//! `scrollTop` when the tab is hidden (Electron unmounts the viewer then) and shown again in
//! the file once when the tab is built; selecting another file drops it. A tab without a state
//! is an "Invalid tab". The tab follows its state as other windows and Electron change it
//! ([`FileBrowser::follow_state`]), as Electron's components read it live.
//!
//! The search panel searches the open file ([`search::find_in_text`]) and the whole
//! worktree ([`search::repo_search`]); go to definition greps for a definition of the word
//! under the cursor ([`search::search_definition`]). Where the viewer should move is a
//! [`Reveal`] the view applies once the file's text is shown.
//!
//! Favourites are the workspace's `favouritePaths` (workspace-relative files and
//! directories) and those its ancestors list, which a child worktree inherits
//! ([`WorkspaceStore::all_favourite_paths`]); the tree stars them and lists the files they
//! name above the tree. Only the workspace's own can be unstarred here.
//!
//! Review comments on the shown file: [`comments`].

use std::collections::HashMap;
use std::ops::Range;
use std::sync::Arc;
use std::time::Duration;

use gpui_kit::{App, Context, Entity, EventEmitter, Subscription, Task, WeakEntity};
use serde_json::{Value, json};
use treeterm_core::daemon::ApiError;
use treeterm_core::exec::{ExecApi, ExecError};
use treeterm_core::files::{FavouriteFile, detect_monaco_language, normalize_favourite_path, resolve_favourite_files};
use treeterm_core::fs::{FsClient, FsError, ReadOutcome};
use treeterm_core::rt::TaskLost;
use treeterm_core::git::GitApi;
use treeterm_core::search::{self, DefinitionLocation, RepoHit, RepoSearchEvent};
use treeterm_proto::treeterm::FileEntry;

use crate::words;
use crate::workspace_store::WorkspaceStore;

#[path = "file_browser_comments.rs"]
pub mod comments;
pub use comments::{Anchor, LineDraft};

/// Text and images bigger than this are not shown until the user asks (Electron's
/// `MAX_READ_FILE_BYTES`; PDFs and models get [`treeterm_core::files::max_read_bytes_for`]).
pub const MAX_VIEW_BYTES: u64 = treeterm_core::files::MAX_READ_FILE_BYTES;

/// The tree's file name search waits for a pause in typing (`FileTree`'s 300ms debounce).
pub const NAME_SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);

pub use crate::definition::DefinitionLookup;
use crate::definition::{Begin, DefinitionProbe, Settled};
pub use crate::search_panel::{REPO_SEARCH_DEBOUNCE, RepoResults, SearchRow};
use crate::search_panel::{count_label, push_repo_section, spawn_repo_search};

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
    Image { path: String, bytes: std::sync::Arc<Vec<u8>>, format: treeterm_core::files::ImageFormat },
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

/// How a file is shown, by extension (`readLanguage` in Electron).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Presentation {
    Text,
    Image(treeterm_core::files::ImageFormat),
    Pdf,
    Model(treeterm_core::files::ModelFormat),
}

fn presentation_of(path: &str) -> Presentation {
    use treeterm_core::files::{image_format, is_pdf_path, model_format};
    if let Ok(format) = image_format(path) {
        Presentation::Image(format)
    } else if let Ok(format) = model_format(path) {
        Presentation::Model(format)
    } else if is_pdf_path(path) {
        Presentation::Pdf
    } else {
        Presentation::Text
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

/// The favourites section (`FavouriteFilesSection`): the files the favourite paths name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Favourites {
    /// The workspace has no favourite paths.
    Empty,
    Loading,
    Ready(Vec<FavouriteFile>),
    Failed(String),
}

/// The tree's "Search files..." results (`FileTree`'s `SearchState`).
#[derive(Debug, Clone, PartialEq)]
pub enum NameResults {
    /// Nothing searched for (yet): no entries.
    Idle,
    Searching,
    Found(Vec<FileEntry>),
    Failed(String),
}

/// The tree's file name search: while its query is not blank the tree lists the results
/// instead of the directories. Results stay those of the last search until the next one
/// starts, after the typing pause.
#[derive(Debug, Clone, PartialEq)]
pub struct NameSearch {
    pub query: String,
    pub results: NameResults,
}

impl NameSearch {
    /// Whether the tree shows results rather than directories.
    pub fn is_active(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// A tree entry's star.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Star {
    Off,
    /// One of the workspace's own favourites: clicking removes it.
    On,
    /// An ancestor's favourite, or inside a favourite directory: shown starred, not
    /// removable on its own ("Inherited or included by a favourite folder").
    Covered,
}

/// `isFavouritePath`: `path` is a favourite or inside a favourite directory.
pub fn is_favourite_path(path: &str, favourites: &[String]) -> bool {
    let path = normalize_favourite_path(path);
    favourites.iter().map(|favourite| normalize_favourite_path(favourite)).any(|favourite| {
        !favourite.is_empty() && (path == favourite || path.starts_with(&format!("{favourite}/")))
    })
}

/// The zoom factors Chromium's PDF viewer steps through (`kPresetZoomFactors`).
pub const PDF_ZOOM_FACTORS: [f64; 17] =
    [0.25, 1. / 3., 0.5, 2. / 3., 0.75, 0.8, 0.9, 1., 1.1, 1.25, 1.5, 1.75, 2., 2.5, 3., 4., 5.];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
}

/// The PDF zoom after one step from `current`: the next preset factor that way, staying at
/// the smallest or largest one.
pub fn pdf_zoom_step(current: f64, direction: ZoomDirection) -> f64 {
    // Within a hair of a preset counts as on it (1/3 and 2/3 are not exact).
    const EPSILON: f64 = 1e-6;
    let next = match direction {
        ZoomDirection::In => PDF_ZOOM_FACTORS.iter().find(|&&factor| factor > current + EPSILON),
        ZoomDirection::Out => PDF_ZOOM_FACTORS.iter().rev().find(|&&factor| factor < current - EPSILON),
    };
    match (next, direction) {
        (Some(&factor), _) => factor,
        (None, ZoomDirection::In) => PDF_ZOOM_FACTORS[PDF_ZOOM_FACTORS.len() - 1],
        (None, ZoomDirection::Out) => PDF_ZOOM_FACTORS[0],
    }
}

/// Where the browser is shown, which decides where a repo search hit opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Host {
    /// The Files tab: the hit opens here.
    FilesTab,
    /// A viewer inside another tab (Review's favourites): the hit opens in a new Files tab
    /// (`openInFileBrowser`).
    Embedded,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileBrowserEvent {
    /// Selection, expansion or scroll offset changed: persist it in the tab state.
    StateChanged(Value),
    /// This key of the tab state was used up or no longer applies: remove it
    /// (`scrollToLine: undefined`).
    StateRemoved(&'static str),
}

impl EventEmitter<FileBrowserEvent> for FileBrowser {}

/// Whether the tab has a state to show: `FilesystemBrowser` shows only "Invalid tab" for one
/// without.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Validity {
    Valid,
    Invalid,
}

/// The tab state's `scrollTop`, still to be shown in `path` (`FileViewer` applies
/// `initialScrollTop` once per file, after it has loaded).
#[derive(Debug, Clone, PartialEq)]
struct ScrollRestore {
    path: String,
    /// Pixels from the top.
    top: f32,
}

pub struct FileBrowser {
    fs: FsClient,
    exec: Arc<dyn ExecApi>,
    /// `HEAD` and `git blame`, for review comments.
    git: GitApi,
    host: Host,
    validity: Validity,
    root: String,
    listings: HashMap<String, Listing>,
    expanded: Vec<String>,
    selected: Option<String>,
    view: FileView,
    /// Reads started so far: `view` comes from the latest, so a view of a file it keeps can
    /// tell when the same file was read again.
    reads: u64,
    search: SearchPanel,
    definition: DefinitionLookup,
    reveal: Option<Reveal>,
    /// The tab state's `scrollToLine` as last read, until used: another one arriving is
    /// revealed.
    scroll_to_line: Option<usize>,
    /// The reveal (its `seq`) of the `scrollToLine` the tab was built with, until the viewer
    /// has moved there and the key is removed (`onScrollToLineUsed`).
    state_reveal: Option<u64>,
    /// The saved offset to show when the file it is for is shown as text.
    scroll_restore: Option<ScrollRestore>,
    /// The workspace whose favourite paths the tree stars.
    workspace: WeakEntity<WorkspaceStore>,
    /// The favourite paths `favourites` was resolved from, the ancestors' included.
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
    /// Saving a copy of the shown file to this computer (`FileDownloadButton`). Every read
    /// starts it at Idle, as Electron's button is mounted afresh with each read.
    download: DownloadState,
    /// The download `download` follows, while its read is the one shown.
    shown_download: ShownDownload,
    /// Downloads started so far (each gets the next number as its id).
    downloads_started: u64,
    /// Running downloads, by id. Each keeps going until it is done, unseen when another
    /// download is shown (its failure is logged).
    downloads: HashMap<u64, Task<()>>,
    name_search: NameSearch,
    _name_search: Option<Task<()>>,
    comments: comments::Comments,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DownloadState {
    Idle,
    Downloading,
    Failed(String),
}

/// The download the Download button follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShownDownload {
    None,
    /// Download `id`, the latest started from read `read` ([`FileBrowser::reads`]).
    Latest { read: u64, id: u64 },
}

/// A filesystem call's outcome, a lost background task counting as the call failing.
pub(crate) fn fs_outcome<T>(result: Result<Result<T, FsError>, TaskLost>) -> Result<T, FsError> {
    result.unwrap_or_else(|lost| Err(ApiError::from(lost).into()))
}

/// What a failed filesystem call says, in the words of Electron's renderer: the daemon's
/// own reason, or `fallback` when it gave none (`result.error || fallback`); a call that
/// failed outright (a rejected IPC call in Electron) is `prefix` then the error.
pub(crate) fn fs_error_text(error: &FsError, fallback: &str, prefix: &str) -> String {
    match error {
        FsError::Daemon(reason) if reason.is_empty() => fallback.to_owned(),
        FsError::Daemon(reason) => reason.clone(),
        FsError::Api(error) => format!("{prefix}{error}"),
    }
}

/// Directories first, then names byte by byte (capitals before lowercase): the daemon's
/// order, which the Electron tree shows as it comes.
fn sort_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| b.is_directory.cmp(&a.is_directory).then_with(|| a.name.cmp(&b.name)));
}

/// A read file shown as text, decoded as UTF-8 (invalid bytes replaced, as a browser
/// decodes them), or as binary when it holds a NUL (`isBinaryContent`).
fn text_view(path: String, content: Vec<u8>) -> FileView {
    let size = content.len() as u64;
    let text = match String::from_utf8(content) {
        Ok(text) => text,
        Err(invalid) => String::from_utf8_lossy(invalid.as_bytes()).into_owned(),
    };
    if treeterm_core::files::is_binary_content(&text) {
        FileView::Binary { path, size }
    } else {
        FileView::Text { path, text }
    }
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

/// A `FilesystemState`'s `expandedDirs`.
fn expanded_dirs_of(state: &Value) -> Vec<String> {
    state
        .get("expandedDirs")
        .and_then(Value::as_array)
        .map(|dirs| dirs.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default()
}

/// A `FilesystemState`'s `scrollToLine` (1-based).
fn scroll_to_line_of(state: &Value) -> Option<usize> {
    state.get("scrollToLine").and_then(Value::as_u64).map(|line| line as usize)
}

/// A tab state's `scrollTop` (Monaco's, in pixels) when there is somewhere to scroll to: 0
/// or none leaves the viewer at the top (`initialScrollTop` is falsy).
pub(crate) fn scroll_top_of(state: &Value) -> Option<f32> {
    state.get("scrollTop").and_then(Value::as_f64).filter(|top| *top > 0.).map(|top| top as f32)
}

/// A scroll offset as Monaco reports `scrollTop`: whole pixels.
pub(crate) fn scroll_top_value(top: f32) -> Value {
    json!(top.max(0.).round() as u64)
}

impl FileBrowser {
    /// `state` is the tab's persisted `FilesystemState`; `favourite_paths` and `in_repo`
    /// (`isGitRepo`) the workspace's (it may be mid-update, so it is not read here).
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        fs: FsClient,
        exec: Arc<dyn ExecApi>,
        git: GitApi,
        in_repo: bool,
        root: String,
        state: &Value,
        workspace: &Entity<WorkspaceStore>,
        favourite_paths: Vec<String>,
        cx: &mut Context<Self>,
    ) -> Self {
        let observe = cx.observe(workspace, |browser, workspace, cx| {
            let (paths, in_repo) = {
                let workspace = workspace.read(cx);
                (workspace.all_favourite_paths(), workspace.file().is_git_repo)
            };
            browser.sync_favourites(paths, cx);
            browser.follow_repo(in_repo, cx);
        });
        let mut browser = Self {
            fs,
            exec,
            git,
            host: Host::FilesTab,
            // `FilesystemBrowser` checks only that there is a state (`!state`).
            validity: if state.is_object() { Validity::Valid } else { Validity::Invalid },
            root: root.clone(),
            listings: HashMap::new(),
            expanded: Vec::new(),
            selected: None,
            view: FileView::Nothing,
            reads: 0,
            search: SearchPanel::Closed,
            definition: DefinitionLookup::Idle,
            reveal: None,
            scroll_to_line: scroll_to_line_of(state),
            state_reveal: None,
            scroll_restore: None,
            workspace: workspace.downgrade(),
            favourite_paths,
            favourites: Favourites::Empty,
            reveals: 0,
            loads: HashMap::new(),
            _read: None,
            download: DownloadState::Idle,
            shown_download: ShownDownload::None,
            downloads_started: 0,
            downloads: HashMap::new(),
            _search: None,
            _definition: None,
            _favourites: None,
            _workspace: observe,
            name_search: NameSearch { query: String::new(), results: NameResults::Idle },
            _name_search: None,
            comments: comments::Comments::new(),
        };
        // An invalid tab shows nothing, so it reads nothing.
        if browser.validity == Validity::Invalid {
            return browser;
        }
        browser.refresh_anchor(in_repo, cx);
        browser.resolve_favourites(cx);
        browser.load(root, cx);
        browser.expand(expanded_dirs_of(state), cx);
        if let Some(path) = state.get("selectedPath").and_then(Value::as_str) {
            browser.show(path, false, cx);
            browser.restore_scroll_top(path, state);
            if let Some(line) = browser.scroll_to_line {
                browser.reveal_at(path, line, 1, 0);
                browser.state_reveal = Some(browser.reveals);
            }
        }
        browser
    }

    /// Whether the tab has no state, so it shows only "Invalid tab".
    pub fn is_invalid(&self) -> bool {
        self.validity == Validity::Invalid
    }

    /// Show the state's `scrollTop` in `path` once its text is shown, unless the state asks
    /// for a line (`!scrollToLine && initialScrollTop`).
    fn restore_scroll_top(&mut self, path: &str, state: &Value) {
        self.scroll_restore = scroll_top_of(state)
            .filter(|_| scroll_to_line_of(state).is_none())
            .map(|top| ScrollRestore { path: path.to_owned(), top });
    }

    /// The saved offset still to be shown in `path`, whose text is shown.
    pub fn scroll_restore(&self, path: &str) -> Option<f32> {
        self.scroll_restore.as_ref().filter(|restore| restore.path == path).map(|restore| restore.top)
    }

    /// [`Self::scroll_restore`], taken once: the viewer scrolls there now.
    pub fn take_scroll_restore(&mut self, path: &str) -> Option<f32> {
        let top = self.scroll_restore(path)?;
        self.scroll_restore = None;
        Some(top)
    }

    /// The viewer was hidden (Electron unmounts it, `handleScrollPositionChange`) with the
    /// selected file's text scrolled `top` pixels down: save that as `scrollTop`. Not while
    /// the saved offset is still to be shown, which the viewer has not scrolled to yet: it
    /// stays saved (`lastScrollTopRef` starts at `initialScrollTop`).
    pub fn save_scroll_top(&mut self, top: f32, cx: &mut Context<Self>) {
        if self.scroll_restore.is_some() {
            return;
        }
        cx.emit(FileBrowserEvent::StateChanged(json!({ "scrollTop": scroll_top_value(top) })));
    }

    /// The viewer moved to reveal `seq`: if that was the tab state's `scrollToLine`, it is
    /// used up and removed (`onScrollToLineUsed`), so the next build does not move there again.
    pub fn reveal_applied(&mut self, seq: u64, cx: &mut Context<Self>) {
        if self.state_reveal == Some(seq) {
            self.drop_state_reveal(cx);
        }
    }

    /// Remove the `scrollToLine` the tab was built with; the same line asked for again is new
    /// again ([`Self::follow_state`]).
    fn drop_state_reveal(&mut self, cx: &mut Context<Self>) {
        self.state_reveal = None;
        self.scroll_to_line = None;
        cx.emit(FileBrowserEvent::StateRemoved("scrollToLine"));
    }

    /// The tab's state as another window or Electron left it (`FilesystemBrowser` reads it
    /// live): its expanded directories show, the newly expanded ones loaded; another selected
    /// file shows, the comment being written going with the one shown before; and a new
    /// `scrollToLine` is revealed in the file then removed (`handleScrollToLineUsed`). What
    /// the state already says is not written back.
    pub fn follow_state(&mut self, state: &Value, cx: &mut Context<Self>) {
        let expanded = expanded_dirs_of(state);
        if expanded != self.expanded {
            self.expand(expanded, cx);
        }
        // Nothing deselects a file once one was picked (`setSelectedPath` gets a path).
        if let Some(path) = state.get("selectedPath").and_then(Value::as_str)
            && self.selected.as_deref() != Some(path)
        {
            self.show(path, false, cx);
            // Another file shows the offset the state has for it, once (`scrollRestoredFor`
            // resets with the path).
            self.restore_scroll_top(path, state);
        }
        let line = scroll_to_line_of(state);
        if line != self.scroll_to_line {
            self.scroll_to_line = line;
            if let (Some(line), Some(path)) = (line, self.selected.clone()) {
                self.reveal_at(&path, line, 1, 0);
                // Used up: the same line asked for again is new again.
                self.scroll_to_line = None;
                cx.emit(FileBrowserEvent::StateRemoved("scrollToLine"));
                cx.notify();
            }
        }
    }

    /// Show `dirs` expanded, loading those not loaded or loading yet (a `DirectoryLoader` per
    /// expanded directory).
    fn expand(&mut self, dirs: Vec<String>, cx: &mut Context<Self>) {
        for dir in &dirs {
            if !matches!(self.listings.get(dir), Some(Listing::Loaded(_) | Listing::Loading)) {
                self.load(dir.clone(), cx);
            }
        }
        self.expanded = dirs;
        cx.notify();
    }

    /// Shown inside another tab ([`Host::Embedded`]).
    pub fn embedded(mut self) -> Self {
        self.host = Host::Embedded;
        self
    }

    pub fn host(&self) -> Host {
        self.host
    }

    pub fn download_state(&self) -> &DownloadState {
        &self.download
    }

    /// Save `path` (on the daemon's host) to `destination` on this computer. `read` is the read
    /// ([`Self::reads`]) whose Download button was clicked. The save panel does not block the
    /// tab, so another file may be shown by now: the download then runs unseen, leaving that
    /// file's button alone.
    pub fn download(&mut self, path: &str, destination: std::path::PathBuf, read: u64, cx: &mut Context<Self>) {
        self.downloads_started += 1;
        let id = self.downloads_started;
        if read == self.reads {
            self.shown_download = ShownDownload::Latest { read, id };
            self.download = DownloadState::Downloading;
            cx.notify();
        }
        let file = self.fs.read_file(&self.root, path, u64::MAX);
        let executor = cx.background_executor().clone();
        let task = cx.spawn(async move |this, cx| {
            let result = match file.await {
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
                browser.downloads.remove(&id);
                let shown = browser.shown_download == ShownDownload::Latest { read: browser.reads, id };
                match (shown, result) {
                    (true, result) => {
                        browser.download = match result {
                            Ok(()) => DownloadState::Idle,
                            Err(error) => DownloadState::Failed(error),
                        };
                        cx.notify();
                    }
                    // Another file is shown, with its own button, or a later download of
                    // this file is on the button.
                    (false, Ok(())) => {}
                    (false, Err(error)) => tracing::error!(%error, "download failed"),
                }
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        });
        self.downloads.insert(id, task);
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

    /// How many reads have started; [`Self::view`] comes from the latest.
    pub fn reads(&self) -> u64 {
        self.reads
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

    /// Read a directory. Loaded entries stay shown until the new ones arrive; anything else
    /// (a failure too, as Electron's `loadDirectory` does) shows "Loading..." meanwhile.
    fn load(&mut self, dir: String, cx: &mut Context<Self>) {
        match self.listings.get(&dir) {
            Some(Listing::Loaded(_)) => {}
            Some(Listing::Loading | Listing::Failed(_)) | None => {
                self.listings.insert(dir.clone(), Listing::Loading);
            }
        }
        let read = self.fs.read_directory(&self.root, &dir);
        let key = dir.clone();
        let task = cx.spawn(async move |this, cx| {
            let listing = match fs_outcome(read.await) {
                Ok(mut contents) => {
                    sort_entries(&mut contents.entries);
                    Listing::Loaded(contents.entries)
                }
                Err(error) => Listing::Failed(fs_error_text(&error, "Failed to load", "Error: ")),
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

    /// A directory's listing state, for "Loading…" / errors under an expanded directory.
    pub fn listing(&self, dir: &str) -> Option<&Listing> {
        self.listings.get(dir)
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

    /// The user picked a file (a tree row, a favourite, a search result): show it. The file
    /// shown already stays as it is, unread, as Electron reads only when the path changes.
    pub fn select_file(&mut self, path: &str, cx: &mut Context<Self>) {
        if self.selected.as_deref() != Some(path) {
            self.open(path, false, cx);
        }
    }

    /// Read a file and show it, even the one shown (Review's Refresh). `allow_large` reads
    /// past [`MAX_VIEW_BYTES`] ("Open anyway").
    pub fn open(&mut self, path: &str, allow_large: bool, cx: &mut Context<Self>) {
        let another = self.selected.as_deref() != Some(path);
        self.show(path, allow_large, cx);
        self.persist(cx);
        // `setSelectedPath`: the offset saved for the file shown before is not this one's.
        if another {
            cx.emit(FileBrowserEvent::StateRemoved("scrollTop"));
        }
    }

    /// [`Self::open`] without saving the selection in the tab state.
    fn show(&mut self, path: &str, allow_large: bool, cx: &mut Context<Self>) {
        self.drop_draft_unless(path);
        if self.scroll_restore.as_ref().is_some_and(|restore| restore.path != path) {
            self.scroll_restore = None;
        }
        self.selected = Some(path.to_owned());
        self.view = FileView::Loading(path.to_owned());
        self.reads += 1;
        self.download = DownloadState::Idle;
        // The line the tab was built to show goes with the reveal, before the viewer got to it.
        if self.state_reveal.is_some() {
            self.drop_state_reveal(cx);
        }
        self.reveal = None;
        if let SearchPanel::Open(search) = &mut self.search {
            search.active = None;
        }
        let presentation = presentation_of(path);
        // PDFs and models are routinely tens of megabytes (`fileLimits.ts`).
        let budget = if allow_large { u64::MAX } else { treeterm_core::files::max_read_bytes_for(path) };
        let read = self.fs.read_file(&self.root, path, budget);
        let owned = path.to_owned();
        self._read = Some(cx.spawn(async move |this, cx| {
            let view = match fs_outcome(read.await) {
                Ok(ReadOutcome::Read(file)) => match presentation {
                    Presentation::Image(format) => FileView::Image {
                        path: owned.clone(),
                        bytes: std::sync::Arc::new(file.content),
                        format,
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
                    Presentation::Text => text_view(owned.clone(), file.content),
                },
                Ok(ReadOutcome::TooLarge { size }) => FileView::TooLarge { path: owned.clone(), size },
                Err(error) => FileView::Failed {
                    path: owned.clone(),
                    error: fs_error_text(&error, "Failed to load file", "Error loading file: "),
                },
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
        cx.notify();
    }

    /// Open a workspace-relative location (a search hit or a definition) and move to it.
    pub fn open_location(&mut self, relative_path: &str, line_number: usize, column: usize, len: usize, cx: &mut Context<Self>) {
        let path = search::to_workspace_absolute(&self.root, relative_path);
        self.select_file(&path, cx);
        self.reveal_at(&path, line_number, column, len);
        cx.notify();
    }

    /// Move to a place in `path` once it is shown; the saved offset is not shown then
    /// (`FileViewer` restores `initialScrollTop` only without a `scrollToLine`).
    fn reveal_at(&mut self, path: &str, line_number: usize, column: usize, len: usize) {
        self.scroll_restore = None;
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

    /// The workspace changed: re-resolve the favourites section if its favourite paths (the
    /// ancestors' included) did.
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

    /// The star of a tree entry (absolute path): on for a path the workspace itself lists as
    /// it is stored, covered for one an ancestor lists or a favourite directory holds.
    pub fn star(&self, path: &str, cx: &App) -> Star {
        let Some(workspace) = self.workspace.upgrade() else {
            return Star::Off;
        };
        let workspace = workspace.read(cx);
        let relative = normalize_favourite_path(search::to_workspace_relative(&self.root, path));
        if workspace.favourite_paths().contains(&relative) {
            Star::On
        } else if is_favourite_path(&relative, &workspace.all_favourite_paths()) {
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
    /// under it as Monaco reads it in the file's language. With neither, an open panel keeps
    /// its query.
    pub fn open_search(&mut self, selection: &str, cursor: Option<usize>, cx: &mut Context<Self>) {
        let seed = if !selection.is_empty() && !selection.contains('\n') {
            Some(selection.to_owned())
        } else {
            cursor.and_then(|offset| self.word_at(offset))
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

    /// The word at byte `cursor` of the shown text, by the file's language
    /// ([`words::word_at`]); none when there is no word there or the file is not text.
    fn word_at(&self, cursor: usize) -> Option<String> {
        match &self.view {
            FileView::Text { path, text } => {
                let word = words::word_at(text, cursor, detect_monaco_language(path));
                Some(word).filter(|word| !word.is_empty()).map(str::to_owned)
            }
            FileView::Nothing
            | FileView::Loading(_)
            | FileView::Image { .. }
            | FileView::Pdf { .. }
            | FileView::Model { .. }
            | FileView::Binary { .. }
            | FileView::TooLarge { .. }
            | FileView::Failed { .. } => None,
        }
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
        let query = search.query.clone();
        self._search = Some(spawn_repo_search(self.exec.clone(), &self.root, &query, cx, Self::apply_repo_event));
    }

    fn apply_repo_event(&mut self, event: RepoSearchEvent, cx: &mut Context<Self>) {
        if let SearchPanel::Open(search) = &mut self.search {
            search.repo.apply(event);
            cx.notify();
        }
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
        // The open file's matches are listed above.
        push_repo_section(&mut rows, &search.repo, |hit| Some(hit.file_path.as_str()) != current);
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

    /// Open a repo hit, selecting the match; an embedded viewer opens it in a new Files tab
    /// at its line instead.
    pub fn open_repo_hit(&mut self, hit: &RepoHit, cx: &mut Context<Self>) {
        let len = match &self.search {
            SearchPanel::Open(search) => search.query.len(),
            SearchPanel::Closed => 0,
        };
        match self.host {
            Host::FilesTab => self.open_location(&hit.file_path, hit.line_number as usize, hit.column as usize, len, cx),
            Host::Embedded => {
                let Some(workspace) = self.workspace.upgrade() else {
                    return;
                };
                let state = json!({
                    "selectedPath": search::to_workspace_absolute(&self.root, &hit.file_path),
                    "scrollToLine": hit.line_number,
                });
                if workspace.update(cx, |w, cx| w.add_tab_with("filesystem", state, cx)).is_none() {
                    tracing::error!("the file browser is not registered");
                }
            }
        }
    }

    // ---- file name search ---------------------------------------------------------------

    pub fn name_search(&self) -> &NameSearch {
        &self.name_search
    }

    /// The tree's search box changed: search file names after the typing pause. A blank
    /// query clears the results.
    pub fn set_name_query(&mut self, query: &str, cx: &mut Context<Self>) {
        if self.name_search.query == query {
            return;
        }
        self.name_search.query = query.to_owned();
        let pause = cx.background_executor().timer(NAME_SEARCH_DEBOUNCE);
        let query = query.to_owned();
        self._name_search = Some(cx.spawn(async move |this, cx| {
            pause.await;
            let search = match this.update(cx, |browser, cx| {
                cx.notify();
                if query.trim().is_empty() {
                    browser.name_search.results = NameResults::Idle;
                    return None;
                }
                browser.name_search.results = NameResults::Searching;
                Some(browser.fs.search_files(&browser.root, &query))
            }) {
                Ok(Some(search)) => search,
                Ok(None) | Err(_) => return, // blank, or the tab closed
            };
            let results = match fs_outcome(search.await) {
                Ok(entries) => NameResults::Found(entries),
                Err(error) => NameResults::Failed(fs_error_text(&error, "Search failed", "Search error: ")),
            };
            match this.update(cx, |browser, cx| {
                browser.name_search.results = results;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// The search box's clear button.
    pub fn clear_name_search(&mut self, cx: &mut Context<Self>) {
        self.name_search = NameSearch { query: String::new(), results: NameResults::Idle };
        self._name_search = None;
        cx.notify();
    }

    /// A search result was clicked: a file is selected; a directory expands (or collapses)
    /// and is selected, as Electron does.
    pub fn open_name_result(&mut self, entry: &FileEntry, cx: &mut Context<Self>) {
        if entry.is_directory {
            self.toggle_dir(&entry.path, cx);
        }
        self.select_file(&entry.path, cx);
    }

    // ---- go to definition --------------------------------------------------------------

    /// Look up the definition of the word at byte `offset` of the open file: one result
    /// opens, several are offered to choose from.
    pub fn go_to_definition(&mut self, offset: usize, cx: &mut Context<Self>) {
        let FileView::Text { path, text } = &self.view else {
            return;
        };
        self._definition = None;
        self.definition = match crate::definition::begin(path, text, offset) {
            Begin::Idle => DefinitionLookup::Idle,
            Begin::Unavailable { language } => DefinitionLookup::Unavailable { language },
            Begin::Search(query) => {
                let symbol = query.symbol.clone();
                let search = crate::definition::run(self.exec.clone(), self.root.clone(), query);
                let searched = symbol.clone();
                self._definition = Some(cx.spawn(async move |this, cx| {
                    let result = search.await;
                    match this.update(cx, |browser, cx| browser.on_definitions(searched, result, cx)) {
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

    /// The Cmd/Ctrl+hover underline for the shown file, while it is text.
    pub fn definition_probe(&self) -> Option<DefinitionProbe> {
        let FileView::Text { path, .. } = &self.view else {
            return None;
        };
        Some(DefinitionProbe::new(self.exec.clone(), self.root.clone(), path.clone()))
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

    /// Monaco's definition navigation: within the open file the viewer moves there; another
    /// file opens in an Editor tab at the line (`monacoNavigationBridge.openFileAtLine`).
    fn go_to_location(&mut self, location: &DefinitionLocation, cx: &mut Context<Self>) {
        let line = location.line_number as usize;
        if self.selected_relative() == Some(location.file_path.as_str()) {
            self.open_location(&location.file_path, line, 1, 0, cx);
            return;
        }
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let path = search::to_workspace_absolute(&self.root, &location.file_path);
        workspace.update(cx, |w, cx| w.open_in_editor_at(&path, line, cx));
    }

    pub fn dismiss_definition(&mut self, cx: &mut Context<Self>) {
        self.definition = DefinitionLookup::Idle;
        self._definition = None;
        cx.notify();
    }

    /// Open a link Cmd/Ctrl+clicked in the shown file ([`WorkspaceStore::open_link`]).
    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        workspace.update(cx, |w, cx| w.open_link(url, cx));
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
        use treeterm_core::files::ImageFormat;
        assert_eq!(presentation_of("/w/a.PNG"), Presentation::Image(ImageFormat::Png));
        assert_eq!(presentation_of("/w/a.svg"), Presentation::Image(ImageFormat::Svg));
        assert_eq!(presentation_of("/w/favicon.ICO"), Presentation::Image(ImageFormat::Ico));
        assert_eq!(presentation_of("/w/photo.avif"), Presentation::Image(ImageFormat::Avif));
        assert_eq!(presentation_of("/w/doc.pdf"), Presentation::Pdf);
        assert_eq!(
            presentation_of("/w/parts/Bracket.STL"),
            Presentation::Model(treeterm_core::files::ModelFormat::Stl)
        );
        assert_eq!(presentation_of("/w/main.rs"), Presentation::Text);
        assert_eq!(presentation_of("/w/Makefile"), Presentation::Text);
    }

    #[test]
    fn pdf_zoom_steps_through_chromiums_presets_and_stops_at_the_ends() {
        let step = pdf_zoom_step;
        assert_eq!(step(1., ZoomDirection::In), 1.1);
        assert_eq!(step(1., ZoomDirection::Out), 0.9);
        assert_eq!(step(0.5, ZoomDirection::Out), 1. / 3.);
        assert_eq!(step(1. / 3., ZoomDirection::Out), 0.25);
        assert_eq!(step(1. / 3., ZoomDirection::In), 0.5);
        // Clamped at both ends.
        assert_eq!(step(0.25, ZoomDirection::Out), 0.25);
        assert_eq!(step(5., ZoomDirection::In), 5.);
        assert_eq!(step(9., ZoomDirection::In), 5.);
        // From a factor between presets, to the nearest one that way.
        assert_eq!(step(1.3, ZoomDirection::In), 1.5);
        assert_eq!(step(1.3, ZoomDirection::Out), 1.25);
    }

    #[test]
    fn directories_sort_first_then_names_as_the_daemon_orders_them() {
        let mut entries =
            vec![entry("b.txt", false), entry("Src", true), entry("a.txt", false), entry("docs", true), entry("README.md", false)];
        sort_entries(&mut entries);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["Src", "docs", "README.md", "a.txt", "b.txt"]);
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
    fn text_with_a_nul_is_binary_and_invalid_utf8_is_replaced() {
        assert!(matches!(text_view("/w/a".into(), b"ab\0c".to_vec()), FileView::Binary { size: 4, .. }));
        match text_view("/w/b".into(), vec![b'o', b'k', 0xff]) {
            FileView::Text { text, .. } => assert_eq!(text, "ok\u{fffd}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_blank_name_query_is_not_a_search() {
        let search = |query: &str| NameSearch { query: query.into(), results: NameResults::Idle };
        assert!(!search("").is_active());
        assert!(!search("  ").is_active());
        assert!(search(" a").is_active());
    }

    #[test]
    fn failures_read_as_electron_words_them() {
        let text = |error: FsError| fs_error_text(&error, "Failed to load", "Error: ");
        assert_eq!(text(FsError::Daemon("Permission denied".into())), "Permission denied");
        assert_eq!(text(FsError::Daemon(String::new())), "Failed to load");
        assert_eq!(
            text(FsError::Api(ApiError::Protocol("ReadDirectory succeeded without contents"))),
            "Error: daemon protocol violation: ReadDirectory succeeded without contents"
        );
        assert_eq!(
            text(fs_outcome::<()>(Err(TaskLost)).unwrap_err()),
            "Error: background task ended without a result (panicked or runtime shut down)"
        );
        assert!(matches!(fs_outcome(Ok(Ok(7))), Ok(7)));
        assert!(matches!(fs_outcome::<()>(Ok(Err(FsError::Daemon("no".into())))), Err(FsError::Daemon(reason)) if reason == "no"));
    }

    #[test]
    fn lines_know_where_they_start() {
        assert_eq!(lines_with_offsets("ab\n\ncd"), vec![(0, "ab"), (3, ""), (4, "cd")]);
    }
}
