//! The diff beside the file list (`StackedDiffList` / `FileDiffSection` /
//! `PierreDiffViewer`): "N/M viewed" and "Collapse all" above GPUI Kit's `Diff`, whose file
//! headers are drawn here (collapse chevron, status, path, stats, Stage, refresh, Viewed),
//! with image files, binary files and files whose diff is not loaded yet, loading, failed or
//! too large shown under their header, and comments under their lines. A file's diff is read
//! as its section nears the viewport.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::diff::{
    Diff, DiffAnnotation, DiffChangeIndicator, DiffDecoration, DiffEvent, DiffFile, DiffHunkSeparator, DiffLinePosition,
    DiffLineRange, DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, ElementId, Pixels, Entity, MouseButton, SharedString, Subscription,
    WeakEntity, Window, div, px, rgb,
};
use treeterm_core::files::image_format;
use treeterm_core::git::{FileChangeStats, FileChangeStatus, Staging, UncommittedFile};
use treeterm_core::review::{ReviewComment, Side};
use treeterm_model::review::{CollapseAll, CommentDraft, DiffList, GitOp, ImageSides, ReviewMode, ReviewModel, StageTarget};
use treeterm_model::review_patch::{DiffSize, FileDiff, FileDiffs, Placeholder, ShownDiff, bare_header, shown_diff};

use super::ReviewView;
use crate::comment_ui::{CommentActions, Placement, comment_display, comment_input};
use crate::ui::{ReadingOrder, RemScope};
use super::parts::{change_stats, checkbox, disabled_checkbox, outline_button, plain_icon, spinner, spinning_icon, status_badge, title};
use crate::file_browser_view::{ImageDrawing, image_drawing, language_for};
use crate::theme::colors;
use treeterm_model::review_find::{DIFF_CONTEXT, MIN_COLLAPSED_LINES};

/// A changed file as the list and the diff headers show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedFile {
    pub path: String,
    pub status: FileChangeStatus,
    pub stats: FileChangeStats,
    /// Uncommitted files: which list they are in (their header's Stage / Unstage).
    pub staged: Option<Staging>,
}

impl ListedFile {
    pub fn committed(file: &treeterm_core::git::DiffFile) -> Self {
        Self { path: file.path.clone(), status: file.status, stats: file.stats, staged: None }
    }

    pub fn uncommitted(file: &UncommittedFile) -> Self {
        Self { path: file.entry.path.clone(), status: file.entry.status, stats: file.stats, staged: Some(file.entry.staged) }
    }
}

/// How the list to show differs from the cached one.
#[derive(Debug, PartialEq, Eq)]
enum ListChange {
    Same,
    /// Other files or another patch: the paths listed before.
    Files(HashSet<String>),
    New,
}

/// How `list`, whose patch and paths make `key`, differs from the cached list (with its
/// key and paths). Electron mounts a `StackedDiffList` per view, keyed by nothing else; in
/// Commits a commit's files replace the last commit's after none (`setCommitDiffFiles([])`).
fn list_change(cached: Option<(&DiffList, &str, &[String])>, list: &DiffList, key: &str) -> ListChange {
    match cached {
        Some((cached, cached_key, _)) if cached == list && cached_key == key => ListChange::Same,
        Some((cached, _, order)) if cached == list => ListChange::Files(order.iter().cloned().collect()),
        Some(_) | None => ListChange::New,
    }
}

/// The files "Load anyway" was clicked for (`forced`) that keep it as `list` shows `order`:
/// a section keeps its `forceRender` while it is mounted, so the cached list keeps the files
/// it still lists, and another list (another view or commit), or none cached, forgets them.
fn keep_forced(cached: Option<&DiffList>, list: &DiffList, order: &[ListedFile], forced: &mut HashSet<String>) {
    match cached {
        Some(cached) if cached == list => forced.retain(|path| order.iter().any(|f| f.path == *path)),
        Some(_) | None => forced.clear(),
    }
}

/// The key of `patch` (`diffs.patch()`) shown as `order` with the files loaded anyway
/// (`forced`), whose change rebuilds the files ([`list_change`]): a file whose diff is asked
/// for, read or fails draws another body (its bare header stays the same), and a "Load anyway"
/// draws its file's rows.
fn list_key(patch: &str, diffs: &FileDiffs, order: &[ListedFile], forced: &HashSet<String>) -> String {
    let mut forced: Vec<&str> = forced.iter().map(String::as_str).collect();
    forced.sort_unstable();
    let states: Vec<String> = diffs
        .iter()
        .map(|(path, diff)| match diff {
            FileDiff::NotLoaded => format!("{path}\u{1}not loaded"),
            FileDiff::Loading => format!("{path}\u{1}loading"),
            FileDiff::Loaded(_) => format!("{path}\u{1}loaded"),
            FileDiff::Failed(error) => format!("{path}\u{1}failed\u{1}{error}"),
        })
        .collect();
    format!(
        "{patch}\u{0}{}\u{0}\u{0}{}\u{0}\u{0}{}",
        order.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join("\u{0}"),
        forced.join("\u{0}"),
        states.join("\u{0}")
    )
}

/// The listed files of a patch, in list order (a path listed twice, once).
struct Parsed {
    files: Vec<DiffFile>,
    shown: Vec<String>,
    /// The files whose body the view draws, and what it draws: a header alone in the
    /// `DiffState` (`review_patch::Placeholder`).
    drawn: HashMap<String, Placeholder>,
}

/// The listed files, each section parsed on its own (`FileDiffSection`): one that does not
/// parse shows why in its own body. `forced`: the files "Load anyway" was clicked for. `Err`:
/// a header that does not parse (the whole diff cannot be shown).
fn parse_listed(diffs: &FileDiffs, order: &[ListedFile], forced: &HashSet<String>) -> Result<Parsed, String> {
    let header = |path: &str| -> Result<DiffFile, String> {
        let file = DiffFile::parse(&bare_header(path)).map_err(|e| e.to_string())?.into_iter().next();
        file.ok_or_else(|| format!("no header for {path}"))
    };
    let mut listed = Parsed { files: Vec::new(), shown: Vec::new(), drawn: HashMap::new() };
    let mut seen: HashSet<&str> = HashSet::new();
    for file in order {
        let path = file.path.as_str();
        let Some(diff) = diffs.get(path).filter(|_| seen.insert(path)) else {
            continue;
        };
        let (parsed, placeholder) = match shown_diff(path, diff, forced.contains(path)) {
            ShownDiff::Absent => continue,
            ShownDiff::Placeholder(placeholder) => (header(path)?, Some(placeholder)),
            ShownDiff::Section(section) => match DiffFile::parse(section) {
                Ok(files) => match files.into_iter().find(|parsed| parsed.path() == path) {
                    Some(parsed) if parsed.is_binary() => (parsed, Some(Placeholder::Binary)),
                    Some(parsed) => (parsed, None),
                    None => continue,
                },
                Err(error) => (header(path)?, Some(Placeholder::Failed(format!("Cannot show this diff: {error}")))),
            },
        };
        listed.files.push(parsed.with_language(language_for(path)));
        listed.shown.push(path.to_owned());
        if let Some(placeholder) = placeholder {
            listed.drawn.insert(path.to_owned(), placeholder);
        }
    }
    Ok(listed)
}

/// `sortFilesAsTree`: the files in the file tree's order.
pub fn tree_sorted(files: &[ListedFile]) -> Vec<ListedFile> {
    let entries: Vec<(&str, FileChangeStats)> = files.iter().map(|f| (f.path.as_str(), f.stats)).collect();
    treeterm_model::review_tree::tree_order(&treeterm_model::review_tree::build_tree(&entries))
        .into_iter()
        .map(|index| files[index].clone())
        .collect()
}

/// The parsed patch shown, for one patch and one filtered, tree-ordered file list.
pub struct DiffCache {
    list: DiffList,
    key: String,
    pub state: Entity<DiffState>,
    order: Vec<String>,
    /// Files whose body the view draws (`Placeholder`): always collapsed in the `DiffState`
    /// (no "binary file" notice); the view folds them itself (`ReviewView::closed_drawn`).
    drawn: HashMap<String, Placeholder>,
    /// Viewed files already collapsed (a file collapses when it becomes viewed).
    collapsed_viewed: HashSet<String>,
    /// An open file just checked off from its header's "Viewed", and how far below the top
    /// of the diff that header was: once the file collapses, the next file's header takes
    /// its place, so the next "Viewed" is under the pointer (`FileDiffSection`'s
    /// `pendingScrollAnchor`). Taken by the next render.
    viewed_anchor: Option<(String, Pixels)>,
    _events: Subscription,
}

impl DiffCache {
    /// The files in the diff's order.
    pub fn paths(&self) -> Vec<String> {
        self.order.clone()
    }

    /// The view whose list this is.
    pub fn mode(&self) -> ReviewMode {
        self.list.mode()
    }

    /// `path` shows a header alone, its body drawn by the view (no rows).
    pub fn has_placeholder(&self, path: &str) -> bool {
        self.drawn.contains_key(path)
    }
}

/// The comment being typed, and the draft it belongs to.
pub struct DraftInput {
    draft: CommentDraft,
    input: Entity<TextareaState>,
    text: String,
    _events: Subscription,
}

/// A press on a line number, from GPUI Kit's `SelectionStarted` (only a press sends it) to
/// the `SelectionEnded` of its release. The Kit's keyboard selections (Shift+Up/Down,
/// Cmd/Ctrl+A) end with no press.
enum LinePress {
    Idle,
    Pressed,
}

impl LinePress {
    /// Follows the diff's events. When a press ends, `clicked` reads the line number it
    /// clicked, if any (the diff's `clicked_line`): pressed and released on it, as a DOM
    /// `click` on it is, Shift or not (the Kit extends a selection with Shift, Electron does
    /// not). Returns the line a click comments on (`onLineNumberClick`). A press released
    /// elsewhere (another line number, the code, a header, outside the diff) and a keyboard
    /// selection comment nowhere.
    fn follow(
        &mut self,
        event: &DiffEvent,
        clicked: impl FnOnce() -> Option<DiffLinePosition>,
    ) -> Option<(String, Side, u32)> {
        match event {
            DiffEvent::SelectionStarted(_) => {
                *self = LinePress::Pressed;
                None
            }
            DiffEvent::SelectionEnded(_) => match std::mem::replace(self, LinePress::Idle) {
                LinePress::Pressed => clicked().map(|at| {
                    let side = match at.side() {
                        DiffSide::Original => Side::Original,
                        DiffSide::Modified => Side::Modified,
                    };
                    (at.path().to_string(), side, u32::try_from(at.line()).unwrap_or(u32::MAX))
                }),
                LinePress::Idle => None,
            },
            _ => None,
        }
    }
}

/// The comment a click on a line number starts (`onLineNumberClick`), when `event` ends
/// one. The clicked line does not stay selected; the find's chosen match `found` is
/// highlighted again, as Electron's `selectedLines` is that match alone.
fn line_click(
    press: &mut LinePress,
    state: &Entity<DiffState>,
    event: &DiffEvent,
    found: Option<&DiffLineRange>,
    cx: &mut App,
) -> Option<(String, Side, u32)> {
    let clicked = press.follow(event, || state.read(cx).clicked_line())?;
    let found = found.cloned();
    state.update(cx, |s, cx| s.set_selected_lines(found, cx));
    Some(clicked)
}

/// What an annotation of the diff shows.
enum Annotated {
    /// A file's header (and, for an image, both sides under it).
    Header(Header),
    /// The comments under one line, their text ordered after `order` (the diff's comments
    /// take document orders in reading order), and the comment box when the draft is there.
    Comments { comments: Vec<ReviewComment>, order: u64, draft: bool },
}

struct Header {
    /// The body the view draws under it, and whether the view has it folded (`None`: the
    /// `DiffState` draws its rows).
    body: Option<(Body, bool)>,
    file: ListedFile,
    /// Uncommitted files: the lists it is in, in list order (a button each).
    staging: Vec<Staging>,
    viewed: bool,
    /// The files above it (right-click "Viewed" marks them).
    above: Vec<String>,
    /// Its diff is being read: its refresh button spins, disabled.
    loading: bool,
}

/// What the view draws under a file's header (`FileDiffSection`'s body).
enum Body {
    /// `.file-diff-placeholder`: "Scroll to load diff".
    NotLoaded,
    /// `.file-diff-loading`.
    Loading,
    /// `.file-diff-error`: why its diff could not be read or shown.
    Failed(String),
    /// An image's sides, else `.file-diff-placeholder`.
    Binary(Option<ImageSides>),
    /// `.file-diff-too-large`, with "Load anyway".
    TooLarge(DiffSize),
}

impl ReviewView {
    /// The stacked diff of `diffs`, limited to `files` (already filtered), in tree order.
    /// `comments`: the Committed and Uncommitted views take comments, a commit's diff does not.
    pub(super) fn stacked(
        &mut self,
        diffs: &FileDiffs,
        files: &[ListedFile],
        viewed: &HashSet<String>,
        comments: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, border, bg_primary) = (c.text_secondary, c.text_primary, c.border, c.bg_primary);
        let order = tree_sorted(files);
        let patch = diffs.patch();
        let state = match self.diff_state(&patch, diffs, &order, viewed, window, cx) {
            Ok(state) => state,
            Err(text) => return Self::message(format!("Cannot show this diff: {text}"), rgb(super::parts::ERROR_RED).into(), 20.),
        };
        let shown_viewed = order.iter().filter(|f| viewed.contains(&f.path)).count();
        let all_viewed = !order.is_empty() && shown_viewed == order.len();
        let paths: Vec<String> = order.iter().map(|f| f.path.clone()).collect();
        let listed = paths.clone();
        // With no file listed (a commit's diff loading, or a filter leaving none), both are disabled.
        let enabled = !order.is_empty();
        // `.stacked-diff-list-header`; its bottom border is the first file's top border
        // (GPUI Kit borders every file header).
        let header = h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(12.))
            .px(px(12.))
            .py(px(4.))
            .text_size(px(12.))
            .line_height(px(14.))
            .text_color(secondary)
            .child(
                // The `<label>` keeps its pointer cursor and title over a disabled box.
                h_flex()
                    .id("check-all-viewed")
                    .gap(px(6.))
                    .items_center()
                    .cursor_pointer()
                    .tooltip(|window, cx| Tooltip::new("Mark every listed file as viewed").build(window, cx))
                    .child(if enabled { checkbox(all_viewed, cx) } else { disabled_checkbox() })
                    .child(format!("{shown_viewed}/{} viewed", order.len()))
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.model.update(cx, |m, cx| m.set_files_viewed(&paths, !all_viewed, cx));
                        }))
                    }),
            )
            .child(
                div()
                    .id("collapse-all")
                    .ml_auto()
                    .px(px(8.))
                    .py(px(2.))
                    .border_1()
                    .border_color(border)
                    .rounded(px(4.))
                    .bg(bg_primary)
                    .text_color(primary)
                    .child(self.collapse_all.label())
                    // `:disabled { opacity: 0.5; cursor: default }`.
                    .when(!enabled, |d| d.opacity(0.5))
                    .when(enabled, |d| d.cursor_pointer())
                    .when(enabled, |d| d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.collapse_all = view.collapse_all.toggled();
                        let collapsed = view.collapse_all == CollapseAll::Collapsed;
                        if let Some(diff) = &view.diff {
                            let (paths, drawn) = (diff.paths(), diff.drawn.clone());
                            diff.state.update(cx, |s, cx| {
                                for path in paths.iter().filter(|p| !drawn.contains_key(*p)) {
                                    s.set_file_collapsed(path, collapsed, cx);
                                }
                            });
                            if collapsed {
                                view.closed_drawn.extend(drawn.into_keys());
                            } else {
                                view.closed_drawn.clear();
                            }
                        }
                        cx.notify();
                    }))),
            );
        let diff = self.diff_element(&state, &patch, diffs, &order, viewed, comments, window, cx);
        let diff = self.with_diff_menu(diff, &state, cx);
        let find = self.find_panel(&state, &patch, &listed, window, cx);
        v_flex().size_full().child(header).child(div().flex_1().min_h_0().child(diff)).children(find).into_any_element()
    }

    /// The cached `DiffState`. A list of another view (or another commit) is a new one;
    /// when the patch (`diffs.patch()`) or the listed files change, the files are replaced in
    /// place, keeping the scroll, each file's collapse and its opened folds (`StackedDiffList`
    /// keys its sections by path); a file whose body moves between the view and the
    /// `DiffState` (its diff loading, read or failed, or loaded anyway) keeps its fold. A
    /// file's "Load anyway" lasts while the list shows it. A file listed for the
    /// first time starts collapsed when viewed, then follows the last "Collapse all" /
    /// "Expand all" (`FileDiffSection`). A file clicked or jumped to is scrolled to
    /// (`scrollToFile`), else a new list starts at its first file.
    #[allow(clippy::too_many_arguments)]
    fn diff_state(
        &mut self,
        patch: &str,
        diffs: &FileDiffs,
        order: &[ListedFile],
        viewed: &HashSet<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<Entity<DiffState>, String> {
        // The views draw a diff only where the model has a list (Commits: a picked commit).
        let Some(list) = self.model.read(cx).diff_list() else {
            return Err("no list of files to show".to_owned());
        };
        // "Load anyway" lasts while its file is listed (`forceRender`, a section's own state).
        keep_forced(self.diff.as_ref().map(|diff| &diff.list), &list, order, &mut self.forced);
        let key = list_key(patch, diffs, order, &self.forced);
        // Another view's list is dropped as it shows (`ReviewView::body`); another commit's
        // files are a new `DiffState` that keeps "Collapse all" (the list stays mounted).
        let change = list_change(self.diff.as_ref().map(|diff| (&diff.list, diff.key.as_str(), diff.order.as_slice())), &list, &key);
        let relisted = !matches!(change, ListChange::Same);
        let fresh = match change {
            ListChange::Same => Vec::new(),
            ListChange::Files(previous) => {
                let parsed = parse_listed(diffs, order, &self.forced)?;
                let fresh: Vec<String> = parsed.shown.iter().filter(|p| !previous.contains(*p)).cloned().collect();
                let collapsed = self.start_listed(&fresh, &parsed.drawn, viewed);
                let Some(diff) = &mut self.diff else {
                    unreachable!("matched above")
                };
                // A file whose body the view draws from now on takes its fold along, and one
                // the `DiffState` draws again gets it back.
                let (mut closed, mut opened): (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());
                for path in parsed.shown.iter().filter(|p| previous.contains(*p)) {
                    match (diff.drawn.contains_key(path), parsed.drawn.contains_key(path)) {
                        (false, true) if diff.state.read(cx).is_file_collapsed(path) => {
                            self.closed_drawn.insert(path.clone());
                        }
                        (false, true) => {
                            self.closed_drawn.remove(path);
                        }
                        (true, false) => {
                            let fold = if self.closed_drawn.remove(path) { &mut closed } else { &mut opened };
                            fold.push(path.clone());
                        }
                        (true, true) | (false, false) => {}
                    }
                }
                // Drawn files stay collapsed in the `DiffState` (the view folds them).
                let collapsed: Vec<SharedString> =
                    collapsed.into_iter().chain(closed).chain(parsed.drawn.keys().cloned()).map(SharedString::from).collect();
                let opened: Vec<SharedString> = opened.into_iter().map(SharedString::from).collect();
                diff.state.update(cx, |s, cx| s.update_files(parsed.files, collapsed, opened, cx));
                diff.key = key;
                diff.order = parsed.shown;
                diff.drawn = parsed.drawn;
                fresh
            }
            ListChange::New => {
                let parsed = parse_listed(diffs, order, &self.forced)?;
                let fresh = parsed.shown.clone();
                let collapsed = self.start_listed(&fresh, &parsed.drawn, viewed);
                let mode = if self.split { DiffMode::Split } else { DiffMode::Unified };
                let state = cx.new(|cx| DiffState::new(parsed.files, cx)
                        .with_mode(mode)
                        .with_context_lines(Some(DIFF_CONTEXT))
                        .with_min_collapsed_lines(MIN_COLLAPSED_LINES));
                let (top, hide_unchanged) = (parsed.shown.first().cloned(), self.hide_unchanged);
                state.update(cx, |s, cx| {
                    for path in collapsed.iter().chain(parsed.drawn.keys()) {
                        s.set_file_collapsed(path, true, cx);
                    }
                    if !hide_unchanged {
                        s.expand_unchanged(cx);
                    }
                    // Folding re-anchors on the first code row; start at the first header.
                    if let Some(top) = &top {
                        s.scroll_to_file(top, cx);
                    }
                });
                let mut press = LinePress::Idle;
                let events = cx.subscribe(&state, move |view, state, event: &DiffEvent, cx| {
                    if let DiffEvent::TopBandChanged(band) = event {
                        let band: Vec<String> = band.iter().map(|p| p.to_string()).collect();
                        view.model.update(cx, |m, cx| m.follow_scroll(&band, cx));
                    }
                    // Each file's diff is read as its section nears the viewport (`onRequestLoad`).
                    if let DiffEvent::NearFilesChanged(near) = event {
                        let near: Vec<String> = near.iter().map(|p| p.to_string()).collect();
                        view.model.update(cx, |m, cx| m.request_files(&near, cx));
                    }
                    // A click on a line number (`onLineNumberClick`) starts a comment there.
                    let found = view.find_selection.as_ref();
                    if let Some((path, side, line)) = line_click(&mut press, &state, event, found, cx) {
                        view.model.update(cx, |m, cx| m.start_comment(&path, side, line, cx));
                    }
                });
                self.diff = Some(DiffCache {
                    list,
                    key,
                    state,
                    order: parsed.shown,
                    drawn: parsed.drawn,
                    collapsed_viewed: HashSet::new(),
                    viewed_anchor: None,
                    _events: events,
                });
                fresh
            }
        };
        let Some(diff) = &mut self.diff else {
            unreachable!("made above")
        };
        // Files listed for the first time start collapsed when viewed: not again below.
        diff.collapsed_viewed.extend(fresh.iter().filter(|p| viewed.contains(*p)).cloned());
        if relisted {
            // The observer watches the files listed now (it re-subscribes when they change).
            self.model.update(cx, |m, _| m.reobserve_scroll());
        }
        // The file clicked or jumped to, once a list shows; one it does not list is dropped.
        if let Some(path) = self.model.update(cx, |m, _| m.take_scroll_request())
            && diff.order.contains(&path)
        {
            diff.state.update(cx, |s, cx| s.scroll_to_file(&path, cx));
        }
        let newly: Vec<String> =
            diff.order.iter().filter(|p| viewed.contains(*p) && !diff.collapsed_viewed.contains(*p)).cloned().collect();
        diff.collapsed_viewed.retain(|p| viewed.contains(p));
        let anchor = diff.viewed_anchor.take();
        if !newly.is_empty() {
            diff.collapsed_viewed.extend(newly.iter().cloned());
            let drawn: HashSet<String> = diff.drawn.keys().cloned().collect();
            diff.state.update(cx, |s, cx| {
                for path in newly.iter().filter(|p| !drawn.contains(*p)) {
                    s.set_file_collapsed(path, true, cx);
                }
            });
            if let Some((path, top)) = anchor.filter(|(path, _)| newly.contains(path))
                && let Some(next) = diff.order.iter().skip_while(|p| **p != path).nth(1).cloned()
            {
                // A text file's header row keeps its height, so the next header goes there
                // now. A body the view draws folds with its header, a row measured open: that
                // header stays put until it is laid out folded.
                let now = if drawn.contains(&path) { path } else { next.clone() };
                diff.state.update(cx, |s, cx| s.scroll_file_header_to(&now, top, cx));
                let state = diff.state.clone();
                window.on_next_frame(move |_, cx| state.update(cx, |s, cx| s.scroll_file_header_to(&next, top, cx)));
            }
            self.closed_drawn.extend(newly.into_iter().filter(|p| drawn.contains(p)));
        }
        Ok(diff.state.clone())
    }

    /// Files listed for the first time: those whose body the view draws fold in the view, and
    /// the others that start collapsed are returned (`CollapseAll::initial_collapsed`).
    fn start_listed(&mut self, fresh: &[String], drawn: &HashMap<String, Placeholder>, viewed: &HashSet<String>) -> Vec<String> {
        let mut collapsed = Vec::new();
        for path in fresh {
            let closed = self.collapse_all.initial_collapsed(viewed.contains(path));
            if drawn.contains_key(path) {
                if closed {
                    self.closed_drawn.insert(path.clone());
                } else {
                    self.closed_drawn.remove(path);
                }
            } else if closed {
                collapsed.push(path.clone());
            }
        }
        collapsed
    }

    /// The draft's comment box, made when the draft changes.
    fn draft_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<(Entity<TextareaState>, bool)> {
        let Some(draft) = self.model.read(cx).draft().cloned() else {
            self.draft_input = None;
            return None;
        };
        if let Some(shown) = &self.draft_input
            && shown.draft == draft
        {
            return Some((shown.input.clone(), !shown.text.trim().is_empty()));
        }
        let input = cx.new(|cx| {
            TextareaState::new(window, cx).placeholder("Add your comment... (Cmd/Ctrl+Enter to submit, Esc to cancel)")
        });
        // Cmd/Ctrl+Enter submits in `comment_input`, before the textarea sees it.
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::Change => {
                if let Some(draft) = &mut view.draft_input {
                    draft.text = input.read(cx).value().to_string();
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.draft_input = Some(DraftInput { draft, input: input.clone(), text: String::new(), _events: events });
        Some((input, false))
    }

    #[allow(clippy::too_many_arguments)]
    fn diff_element(
        &mut self,
        state: &Entity<DiffState>,
        patch: &str,
        diffs: &FileDiffs,
        order: &[ListedFile],
        viewed: &HashSet<String>,
        with_comments: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut annotations: Vec<DiffAnnotation> = Vec::new();
        let mut items: HashMap<ElementId, Annotated> = HashMap::new();
        let drawn = self.diff.as_ref().map(|diff| diff.drawn.clone()).unwrap_or_default();
        let model = self.model.read(cx);
        for (index, staging) in file_headers(order) {
            let file = &order[index];
            let id = ElementId::from(SharedString::from(format!("header-{}", file.path)));
            annotations.push(DiffAnnotation::file(id.clone(), file.path.clone()));
            let body = drawn.get(&file.path).map(|placeholder| {
                let body = match placeholder {
                    Placeholder::NotLoaded => Body::NotLoaded,
                    Placeholder::Loading => Body::Loading,
                    Placeholder::Failed(error) => Body::Failed(error.clone()),
                    Placeholder::Image | Placeholder::Binary => Body::Binary(model.image(&file.path).cloned()),
                    Placeholder::TooLarge(size) => Body::TooLarge(*size),
                };
                (body, self.closed_drawn.contains(&file.path))
            });
            items.insert(
                id,
                Annotated::Header(Header {
                    body,
                    file: file.clone(),
                    staging,
                    viewed: viewed.contains(&file.path),
                    above: order[..index].iter().map(|f| f.path.clone()).collect(),
                    loading: diffs.is_loading(&file.path),
                }),
            );
        }
        let draft = if with_comments { model.draft().cloned() } else { None };
        let adding = model.adding().clone();
        let comments = if with_comments { model.comments(cx).unwrap_or_default() } else { Vec::new() };
        // A file whose body the view draws has no rows to comment under (Electron's comments
        // are in the `PierreDiffViewer` it does not mount).
        let shown: HashSet<&str> = order.iter().map(|f| f.path.as_str()).filter(|path| !drawn.contains_key(*path)).collect();
        // `PierreDiffViewer` groups a line's comments, with the comment box after them.
        let mut groups: Vec<((String, Side, u32), Vec<ReviewComment>)> = Vec::new();
        for comment in comments.into_iter().filter(|c| shown.contains(c.file_path.as_str())) {
            let key = (comment.file_path.clone(), comment.side, comment.line_number);
            match groups.iter_mut().find(|(k, _)| *k == key) {
                Some((_, list)) => list.push(comment),
                None => groups.push((key, vec![comment])),
            }
        }
        if let Some(draft) = &draft {
            let key = (draft.file_path.clone(), draft.side, draft.line_number);
            if !groups.iter().any(|(k, _)| *k == key) && !drawn.contains_key(&draft.file_path) {
                groups.push((key, Vec::new()));
            }
        }
        // Their text takes the view's document orders in reading order, so that a drag over
        // several copies them in that order.
        let keys: Vec<(String, Side, u32)> = groups.iter().map(|(key, _)| key.clone()).collect();
        let files: Vec<String> = order.iter().map(|file| file.path.clone()).collect();
        let mut orders = vec![0; groups.len()];
        let mut next = self.first_order;
        for ix in treeterm_model::review_find::reading_order(diffs, &files, &keys, self.layout().style) {
            orders[ix] = next;
            next += groups[ix].1.len() as u64;
        }
        for (((path, side, line), list), text_order) in groups.into_iter().zip(orders) {
            let is_draft = draft.as_ref().is_some_and(|d| d.file_path == path && d.side == side && d.line_number == line);
            let id = ElementId::from(SharedString::from(format!("comments-{path}-{}-{line}", side.as_str())));
            let side = match side {
                Side::Original => DiffSide::Original,
                Side::Modified => DiffSide::Modified,
            };
            annotations.push(DiffAnnotation::line(id.clone(), DiffLinePosition::new(path, side, line as usize)));
            items.insert(id, Annotated::Comments { comments: list, order: text_order, draft: is_draft });
        }
        let draft_box = if draft.is_some() { self.draft_box(window, cx) } else {
            self.draft_input = None;
            None
        };
        let items = Rc::new(items);
        let this = cx.weak_entity();
        let model = self.model.clone();
        let state_for_headers = state.clone();
        let sticky = (items.clone(), this.clone(), model.clone(), state.clone());
        let draft_label = draft.map(|d| format!("Comment on line {} ({})", d.line_number, d.side.as_str()));
        // Pierre tints changed rows (`--diffs-deletion/addition-color`) on its near-black body.
        let mut decorations = Vec::new();
        for (path, lines) in treeterm_model::review_tree::changed_lines(patch).into_iter().filter(|(path, _)| !drawn.contains_key(path)) {
            // The file's body first: the last decoration covering a line wins.
            for (side, last) in [(DiffSide::Original, lines.original_lines), (DiffSide::Modified, lines.modified_lines)] {
                if last > 0 {
                    let range = DiffLineRange::new(path.clone(), side, 1, last as usize);
                    decorations.push(DiffDecoration::new(range).with_background(rgb(DIFF_BG)));
                }
            }
            for (side, runs, tint) in [(DiffSide::Original, &lines.deleted, DELETED_ROW), (DiffSide::Modified, &lines.added, ADDED_ROW)] {
                for run in runs {
                    let range = DiffLineRange::new(path.clone(), side, *run.start() as usize, *run.end() as usize);
                    decorations.push(DiffDecoration::new(range).with_background(rgb(tint)));
                }
            }
        }
        let diff = Diff::new(state)
            .header_visible(false)
            .line_number(true)
            .syntax_highlight(true)
            // Pierre's `treeterm-dark` theme, not the code editors' `vs-dark`.
            .highlight_theme(super::dark_plus::theme())
            .soft_wrap(true)
            .change_background(false)
            .change_indicator(DiffChangeIndicator::None)
            .hunk_separator(DiffHunkSeparator::LineInfo)
            .bg(colors(cx).bg_primary)
            .border_0()
            .decorations(decorations)
            .annotations(annotations)
            // `.file-diff-header` is `position: sticky; top: 0` in its file's section.
            .render_sticky_header(move |file, _, cx| {
                let (items, view, model, state) = &sticky;
                let id = ElementId::from(SharedString::from(format!("header-{}", file.path())));
                match items.get(&id) {
                    Some(Annotated::Header(header)) => sticky_header(header, state, view, model, cx),
                    Some(Annotated::Comments { .. }) | None => div().into_any_element(),
                }
            })
            .render_annotation(move |annotation, window, cx| {
                let Some(item) = items.get(annotation.id()) else {
                    return div().into_any_element();
                };
                match item {
                    Annotated::Header(header) => file_header(header, &state_for_headers, &this, &model, window, cx),
                    Annotated::Comments { comments, order, draft } => {
                        let input = draft_box.clone().filter(|_| *draft);
                        comment_group(comments, *order, input, draft_label.clone(), &adding, &model, window, cx)
                    }
                }
            })
            .into_any_element();
        // `--diffs-font-size: 12px` (the theme's mono size is 13px): the diff is laid out at
        // a rem that makes its code 12px.
        RemScope::new(px(16. * 12. / 13.), diff).into_any_element()
    }
}

/// One header per path, at its first listing, with the lists the path is in (in list
/// order). A partially staged file is listed twice: `StackedDiffList` gives it an "Unstage"
/// section and a "Stage" section, both showing its staged hunks. GPUI Kit keys files by
/// path, so the diff shows it once, under one header with both buttons.
fn file_headers(order: &[ListedFile]) -> Vec<(usize, Vec<Staging>)> {
    let mut headers: Vec<(usize, Vec<Staging>)> = Vec::new();
    let mut at: HashMap<&str, usize> = HashMap::new();
    for (index, file) in order.iter().enumerate() {
        match at.get(file.path.as_str()) {
            Some(&header) => headers[header].1.extend(file.staged),
            None => {
                at.insert(&file.path, headers.len());
                headers.push((index, file.staged.into_iter().collect()));
            }
        }
    }
    headers
}

/// Pierre's body and its changed-row tints, as Chromium draws them.
const DIFF_BG: u32 = 0x070707;
const DELETED_ROW: u32 = 0x351b17;
const ADDED_ROW: u32 = 0x192919;

/// A file's header in the diff (an annotation of its first row), with the body the view
/// draws under it when open: its diff loading or failed, an image file's sides or a binary
/// file's placeholder.
fn file_header(
    header: &Header,
    state: &Entity<DiffState>,
    view: &WeakEntity<ReviewView>,
    model: &Entity<ReviewModel>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (bg_primary, secondary) = (colors(cx).bg_primary, colors(cx).text_secondary);
    // GPUI Kit pads (0.75rem) and borders every annotation, painting the borders over its
    // content: the header fills the padding and the borders are its own. (Pierre's 8px above
    // the code would sit above the bottom border, so it is left out.)
    let pad = window.rem_size() * 0.75;
    // `.file-diff-placeholder`.
    let placeholder = |text: &'static str| {
        div().px(px(12.)).py(px(16.)).bg(bg_primary).text_size(px(12.)).italic().text_color(secondary).child(text).into_any_element()
    };
    let body = match &header.body {
        Some((_, true)) | None => None,
        Some((Body::NotLoaded, false)) => Some(placeholder("Scroll to load diff")),
        Some((Body::Loading, false)) => Some(div().bg(bg_primary).child(loading_diff(cx)).into_any_element()),
        Some((Body::Failed(error), false)) => Some(div().bg(bg_primary).child(diff_error(error.clone())).into_any_element()),
        Some((Body::Binary(Some(sides)), false)) => Some(image_diff(&header.file.path, sides.clone(), cx)),
        Some((Body::Binary(None), false)) => Some(placeholder("Binary file \u{2014} diff not shown")),
        Some((Body::TooLarge(size), false)) => Some(too_large(size, &header.file.path, view, cx)),
    };
    v_flex()
        .mx(-pad)
        .my(-pad)
        .on_mouse_down(MouseButton::Right, keep_right_button)
        .child(header_row(header, state, view, model, cx))
        .children(body)
        .into_any_element()
}

/// `.file-diff-loading`: a spinning `Loader2` and "Loading diff...".
fn loading_diff(cx: &App) -> AnyElement {
    let secondary = colors(cx).text_secondary;
    h_flex()
        .gap(px(6.))
        .items_center()
        .px(px(12.))
        .py(px(16.))
        .text_size(px(12.))
        .text_color(secondary)
        .child(spinner(16., secondary))
        .child("Loading diff...")
        .into_any_element()
}

/// `.file-diff-too-large`: how large the diff is, and "Load anyway", which draws its rows.
fn too_large(size: &DiffSize, path: &str, view: &WeakEntity<ReviewView>, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, bg_secondary, hover, border, bg_primary) =
        (c.text_secondary, c.text_primary, c.bg_secondary, c.bg_hover, c.border, c.bg_primary);
    // `.file-diff-load-anyway-btn`.
    let load = {
        let (view, path) = (view.clone(), path.to_owned());
        div()
            .id(SharedString::from(format!("load-anyway-{path}")))
            .px(px(10.))
            .py(px(4.))
            .text_size(px(12.))
            // Chromium's `normal` line height for the UI font.
            .line_height(px(14.))
            .text_color(primary)
            .bg(bg_secondary)
            .border_1()
            .border_color(border)
            .rounded(px(4.))
            .cursor_pointer()
            .hover(move |d| d.bg(hover))
            .child("Load anyway")
            .on_click(move |_, _, cx| {
                // Not the header's fold.
                cx.stop_propagation();
                let path = path.clone();
                match view.update(cx, |view, cx| {
                    view.forced.insert(path);
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: the tab closed
                }
            })
    };
    v_flex()
        .min_h(px(40.))
        .items_start()
        .gap(px(8.))
        .px(px(12.))
        .py(px(16.))
        .bg(bg_primary)
        .text_size(px(12.))
        .text_color(secondary)
        .child(div().w_full().child(size.notice()))
        .child(load)
        .into_any_element()
}

/// `.file-diff-error` (`#f85149`) in a `.file-diff-body` (`min-height: 40px`).
fn diff_error(error: String) -> AnyElement {
    div().min_h(px(40.)).p(px(12.)).text_size(px(12.)).text_color(rgb(0xf85149)).child(error).into_any_element()
}

/// The header pinned over the top of the diff while its file scrolls under it: the row
/// alone, between the borders GPUI Kit draws around it in the list.
fn sticky_header(
    header: &Header,
    state: &Entity<DiffState>,
    view: &WeakEntity<ReviewView>,
    model: &Entity<ReviewModel>,
    cx: &mut App,
) -> AnyElement {
    v_flex()
        .border_t_1()
        .border_b_1()
        .border_color(colors(cx).border)
        .on_mouse_down(MouseButton::Right, keep_right_button)
        .child(header_row(header, state, view, model, cx))
        .into_any_element()
}

/// A file header, with its image, and a line's comments sit outside Pierre's viewer in
/// Electron, so a right-click there opens no Search / Copy menu (`contextTarget`): the diff's
/// menu does not see it. The comment box's own menu and "Viewed" come first.
fn keep_right_button(_: &gpui_kit::MouseDownEvent, _: &mut Window, cx: &mut App) {
    cx.stop_propagation();
}

/// `.file-diff-header`: chevron, status, path, stats, Stage / Unstage, refresh, Viewed.
fn header_row(
    header: &Header,
    state: &Entity<DiffState>,
    view: &WeakEntity<ReviewView>,
    model: &Entity<ReviewModel>,
    cx: &mut App,
) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let (secondary, primary, accent, hover, tertiary, border) =
        (c.text_secondary, c.text_primary, c.accent, c.bg_hover, c.bg_tertiary, c.border);
    let file = &header.file;
    let collapsed = match &header.body {
        Some((_, closed)) => *closed,
        None => state.read(cx).is_file_collapsed(&file.path),
    };
    let loading = header.loading;
    let staging_busy = model.read(cx).staging() == &GitOp::Running;
    let toggle = {
        let (state, path, view, drawn) = (state.clone(), file.path.clone(), view.clone(), header.body.is_some());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            if drawn {
                let path = path.clone();
                match view.update(cx, |view, cx| {
                    if !view.closed_drawn.remove(&path) {
                        view.closed_drawn.insert(path);
                    }
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: the tab closed
                }
            } else {
                state.update(cx, |s, cx| s.set_file_collapsed(&path, !collapsed, cx));
            }
        }
    };
    let stage: Vec<_> = header
        .staging
        .iter()
        .map(|&staged| {
            let (model, target) = (model.clone(), StageTarget::File(file.path.clone()));
            let (id, label) = match staged {
                Staging::Staged => (format!("header-unstage-{}", file.path), "Unstage"),
                Staging::Unstaged => (format!("header-stage-{}", file.path), "Stage"),
            };
            outline_button(SharedString::from(id), 11., (1., 6.), 3., !staging_busy, cx).child(label).on_click(move |_, _, cx| {
                // Disabled while staging runs, it still keeps the click from the header.
                cx.stop_propagation();
                if !staging_busy {
                    let target = target.clone();
                    model.update(cx, |m, cx| match staged {
                        Staging::Staged => m.unstage(target, cx),
                        Staging::Unstaged => m.stage(target, cx),
                    });
                }
            })
        })
        .collect();
    // `.file-diff-refresh-btn`: reads this file's diff again, disabled while it is read.
    let refresh = {
        let (model, path) = (model.clone(), file.path.clone());
        div()
            .id(SharedString::from(format!("header-refresh-{}", file.path)))
            .flex()
            .flex_shrink_0()
            .items_center()
            .ml_auto()
            .px(px(6.))
            .py(px(2.))
            .border_1()
            .border_color(border)
            .rounded(px(3.))
            .text_color(secondary)
            .tooltip(|window, cx| Tooltip::new("Refresh this file's diff").build(window, cx))
            .when(loading, |d| d.opacity(0.5))
            .when(!loading, |d| d.cursor_pointer().hover(move |d| d.bg(hover).text_color(primary)))
            .child(if loading {
                spinning_icon(IconName::RefreshCw, 12., secondary)
            } else {
                plain_icon(IconName::RefreshCw, 12.).into_any_element()
            })
            .on_click(move |_, _, cx| {
                // A disabled button still keeps the click from the header.
                cx.stop_propagation();
                if !loading {
                    model.update(cx, |m, cx| m.refresh_file(&path, cx));
                }
            })
    };
    let viewed = {
        let (model, path, above, view, state) =
            (model.clone(), file.path.clone(), header.above.clone(), view.clone(), state.clone());
        let mark_above = model.clone();
        let is_viewed = header.viewed;
        h_flex()
            .id(SharedString::from(format!("header-viewed-{}", file.path)))
            .flex_shrink_0()
            .gap(px(4.))
            .items_center()
            .px(px(6.))
            .py(px(1.))
            .text_size(px(11.))
            .text_color(if is_viewed { accent } else { secondary })
            .cursor_pointer()
            .child(checkbox(is_viewed, cx))
            .child(
                div()
                    .id(SharedString::from(format!("header-viewed-label-{}", file.path)))
                    .child("Viewed")
                    // Told only where the right-click below works: not on the first file.
                    .when(!above.is_empty(), |d| d.tooltip(title("Right-click to mark all above as viewed"))),
            )
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                // Checking off an open file: where its header (in the list, or pinned) is now.
                let anchor = if is_viewed || collapsed { None } else { state.read(cx).file_header_top(&path) };
                model.update(cx, |m, cx| m.toggle_viewed(&path, cx));
                // The view collapses it when it sees it viewed.
                match view.update(cx, |view, cx| {
                    if let Some(diff) = &mut view.diff {
                        diff.viewed_anchor = anchor.map(|top| (path.clone(), top));
                    }
                    cx.notify();
                }) {
                    Ok(()) | Err(_) => {} // Err: the tab closed
                }
            })
            .when(!above.is_empty(), |d| {
                d.on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    cx.stop_propagation();
                    mark_above.update(cx, |m, cx| m.set_files_viewed(&above, true, cx));
                })
            })
    };
    h_flex()
        .id(SharedString::from(format!("header-{}", file.path)))
        .items_center()
        .gap(px(6.))
        .px(px(12.))
        .py(px(6.))
        .bg(tertiary)
        .text_size(px(12.))
        .line_height(px(14.))
        .cursor_pointer()
        .hover(move |d| d.bg(hover))
        .child(div().flex().flex_shrink_0().items_center().text_color(secondary).child(plain_icon(
            if collapsed { IconName::ChevronRight } else { IconName::ChevronDown },
            14.,
        )))
        .child(status_badge(file.status))
        .child(
            div()
                .id(SharedString::from(format!("header-path-{}", file.path)))
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(primary)
                .child(file.path.clone())
                // `title={file.path}`: the whole path when it is cut short.
                .tooltip(title(file.path.clone())),
        )
        .child(
            h_flex()
                .flex_shrink_0()
                .gap(px(6.))
                .text_size(px(11.))
                .children(change_stats(SharedString::from(format!("header-stats-{}", file.path)), file.stats, cx)),
        )
        .children(stage)
        .child(refresh)
        .child(viewed)
        .on_click(toggle)
}

/// The squares of `.image-diff-side img`'s checkerboard, `repeating-conic-gradient(#3a3a3a 0%
/// 25%, #2a2a2a 0% 50%) 50% / 16px 16px`: 16px tiles centred on the image, each a 2×2 of 8px
/// squares, lighter above right of its centre and below left.
const CHECKER_SQUARE: f32 = 8.;
const CHECKER_DARK: u32 = 0x2a2a2a;
const CHECKER_LIGHT: u32 = 0x3a3a3a;

/// The light squares of the checkerboard filling `bounds`, cut to it (the dark ones are its
/// background).
fn checker_squares(bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
    let centre = bounds.center();
    // The squares (counted from the centre) that `from..to` crosses on one axis.
    let squares = |from: Pixels, to: Pixels, centre: Pixels| {
        let index = |at: Pixels| f32::from(at - centre) / CHECKER_SQUARE;
        index(from).floor() as i32..index(to).ceil() as i32
    };
    let (columns, rows) =
        (squares(bounds.left(), bounds.right(), centre.x), squares(bounds.top(), bounds.bottom(), centre.y));
    let at = |index: i32, centre: Pixels| centre + px(index as f32 * CHECKER_SQUARE);
    rows.flat_map(|row| columns.clone().map(move |column| (column, row)))
        // Above right of the centre is (0, -1): the light squares are those with an odd sum.
        .filter(|(column, row)| (column + row).rem_euclid(2) == 1)
        .map(|(column, row)| {
            let top_left = gpui_kit::point(at(column, centre.x).max(bounds.left()), at(row, centre.y).max(bounds.top()));
            let bottom_right =
                gpui_kit::point(at(column + 1, centre.x).min(bounds.right()), at(row + 1, centre.y).min(bounds.bottom()));
            Bounds::from_corners(top_left, bottom_right)
        })
        .collect()
}

/// The checkerboard behind a diff image, so that its transparent pixels show.
fn checkerboard() -> impl IntoElement {
    gpui_kit::canvas(
        |_, _, _| {},
        |bounds, _, window, _| {
            for square in checker_squares(bounds) {
                window.paint_quad(gpui_kit::fill(square, rgb(CHECKER_LIGHT)));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// `ImageDiffViewer`: before and after side by side on a checkerboard.
fn image_diff(path: &str, sides: ImageSides, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, bg) = (c.text_secondary, c.bg_primary);
    let side = |label: &'static str, bytes: Option<std::sync::Arc<Vec<u8>>>, drawing: ImageDrawing| {
        // The `<img alt>`, which Chromium shows when it cannot decode the image.
        let broken = crate::ui::broken_image(format!("{label}: {path}"), cx);
        v_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .child(div().self_start().text_size(px(11.)).text_color(secondary).child(label.to_uppercase()))
            .child(match bytes {
                Some(bytes) => {
                    let source = match drawing {
                        ImageDrawing::Gpui(format) => {
                            gpui_kit::ImageSource::from(std::sync::Arc::new(gpui_kit::Image::from_bytes(format, (*bytes).clone())))
                        }
                        // GPUI has no AVIF decoder. The Files viewer has WebKit draw one, which
                        // does not fit a diff scrolled in a list, so the app decodes it.
                        ImageDrawing::Webview => crate::avif::image_source(bytes),
                    };
                    let image = gpui_kit::img(source)
                        .max_w_full()
                        .max_h(px(480.))
                        .object_fit(gpui_kit::ObjectFit::Contain)
                        .with_fallback(broken);
                    div().relative().max_w_full().bg(rgb(CHECKER_DARK)).child(checkerboard()).child(image).into_any_element()
                }
                None => div().p(px(24.)).text_size(px(12.)).italic().text_color(secondary).child("No file").into_any_element(),
            })
    };
    let failed = diff_error;
    let body = match sides {
        ImageSides::Loading => loading_diff(cx),
        ImageSides::Failed(error) => failed(error),
        ImageSides::Loaded { before, after } => match image_format(path) {
            Ok(format) => h_flex()
                .items_start()
                .gap(px(12.))
                .p(px(12.))
                .child(side("Before", before, image_drawing(format)))
                .child(side("After", after, image_drawing(format)))
                .into_any_element(),
            Err(error) => failed(error.to_string()),
        },
    };
    div().bg(bg).child(body).into_any_element()
}

/// `.pierre-diff-annotation`: a line's comments (`CommentDisplay`), their text ordered
/// after `order`, and the comment box (`CommentInput`).
#[allow(clippy::too_many_arguments)]
fn comment_group(
    comments: &[ReviewComment],
    order: u64,
    input: Option<(Entity<TextareaState>, bool)>,
    draft_label: Option<String>,
    adding: &GitOp,
    model: &Entity<ReviewModel>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let (delete, cancel, submit) = (model.clone(), model.clone(), model.clone());
    let actions = CommentActions {
        delete: Rc::new(move |id, cx| delete.update(cx, |m, cx| m.delete_comment(id, cx))),
        cancel: Rc::new(move |cx| cancel.update(cx, |m, cx| m.cancel_comment(cx))),
        submit: Rc::new(move |text, cx| submit.update(cx, |m, cx| m.submit_comment(text, cx))),
    };
    // Over GPUI Kit's annotation padding; its top border is `.pierre-diff-annotation`'s.
    let pad = window.rem_size() * 0.75;
    // Pierre's annotations inherit its monospace font; buttons keep the UI font.
    let mut group = v_flex()
        .mx(-pad)
        .my(-pad)
        .px(px(8.))
        .py(px(4.))
        .on_mouse_down(MouseButton::Right, keep_right_button)
        .bg(colors(cx).bg_secondary)
        .font_family(super::parts::MONO);
    let mut text = ReadingOrder::after(order);
    for comment in comments {
        group = group.child(comment_display(comment, &Placement::Diff, &actions, &mut text, cx));
    }
    match input {
        Some((input, has_text)) => group
            .child(comment_input(&input, has_text, draft_label.unwrap_or_default(), adding, &actions, window, cx))
            .into_any_element(),
        None => group.into_any_element(),
    }
}

/// An ISO-8601 time (`git log %aI`: `2026-11-11T17:57:00+08:00`) as milliseconds since the
/// epoch; `None` when it is not one.
pub fn parse_iso(text: &str) -> Option<i64> {
    let (date, rest) = text.split_once('T')?;
    let mut ymd = date.split('-');
    let (year, month, day): (i64, i64, i64) = (ymd.next()?.parse().ok()?, ymd.next()?.parse().ok()?, ymd.next()?.parse().ok()?);
    let (time, offset) = match rest.find(['+', '-', 'Z']) {
        Some(at) => (&rest[..at], &rest[at..]),
        None => (rest, ""),
    };
    let mut hms = time.split(':');
    let (hour, minute, second): (i64, i64, i64) =
        (hms.next()?.parse().ok()?, hms.next()?.parse().ok()?, hms.next().unwrap_or("0").split('.').next()?.parse().ok()?);
    let offset_minutes = match offset.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let (h, m) = offset[1..].split_once(':').unwrap_or((&offset[1..], "0"));
            let minutes = h.parse::<i64>().ok()? * 60 + m.parse::<i64>().ok()?;
            if sign == '-' { -minutes } else { minutes }
        }
        Some(_) | None => 0,
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 86_400 + hour * 3600 + minute * 60 + second) - offset_minutes * 60) * 1000)
}

/// `new Date(iso).toLocaleDateString()` in Electron's locale (en-US: `11/11/2026`), local time.
pub fn locale_date(iso: &str) -> String {
    match parse_iso(iso) {
        Some(millis) => treeterm_core::clock::locale_date(millis),
        None => iso.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_times_with_offsets_become_epoch_milliseconds() {
        assert_eq!(parse_iso("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_iso("1970-01-01T08:00:00+08:00"), Some(0));
        assert_eq!(parse_iso("2000-03-01T00:00:01-01:30"), Some(951_868_801_000 + 5_400_000));
        assert_eq!(parse_iso("not a date"), None);
    }

    fn square(left: f32, top: f32, right: f32, bottom: f32) -> Bounds<Pixels> {
        Bounds::from_corners(gpui_kit::point(px(left), px(top)), gpui_kit::point(px(right), px(bottom)))
    }

    #[test]
    fn the_checkerboard_is_lighter_above_right_and_below_left_of_its_centre() {
        // 32×32: 4×4 whole squares, half of them light.
        let squares = checker_squares(square(100., 50., 132., 82.));
        assert_eq!(squares.len(), 8, "{squares:?}");
        assert!(squares.contains(&square(116., 58., 124., 66.)), "above right of the centre");
        assert!(squares.contains(&square(108., 66., 116., 74.)), "below left of the centre");
        assert!(!squares.contains(&square(108., 58., 116., 66.)), "above left of the centre");
        assert!(!squares.contains(&square(116., 66., 124., 74.)), "below right of the centre");
        assert!(!squares.contains(&square(100., 50., 108., 58.)), "the corner, as above left");
        assert!(squares.contains(&square(108., 50., 116., 58.)), "beside the corner");
    }

    #[test]
    fn checkerboard_squares_are_cut_at_the_edges() {
        // 20×20, centred on (10, 10): the squares around the middle 16px are cut to 2px.
        let squares = checker_squares(square(0., 0., 20., 20.));
        assert_eq!(
            squares,
            vec![
                square(2., 0., 10., 2.),
                square(18., 0., 20., 2.),
                square(0., 2., 2., 10.),
                square(10., 2., 18., 10.),
                square(2., 10., 10., 18.),
                square(18., 10., 20., 18.),
                square(0., 18., 2., 20.),
                square(10., 18., 18., 20.),
            ]
        );
        assert!(checker_squares(square(5., 5., 5., 9.)).is_empty(), "nothing to fill");
    }

    #[test]
    fn commit_dates_are_locale_dates() {
        let date = locale_date("2026-01-05T12:00:00Z");
        assert_eq!(date, treeterm_core::clock::locale_date(1_767_614_400_000));
        assert_eq!(locale_date("garbage"), "garbage");
    }

    /// A 400 x 300 diff of three files, each far taller than the viewport.
    struct Host {
        state: Entity<DiffState>,
    }

    impl gpui_kit::Render for Host {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).h(px(300.)).child(Diff::new(&self.state).size_full())
        }
    }

    fn draw(cx: &mut gpui_kit::VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }

    /// `name` with its 40 lines replaced by `new 0`, `new 1`, …, each prefixed by `prefix`.
    fn rewritten(name: &str, prefix: &str) -> String {
        let mut patch = format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,40 +1,40 @@\n");
        patch.extend((0..40).map(|line| format!("-old {line}\n")));
        patch.extend((0..40).map(|line| format!("+{prefix}new {line}\n")));
        patch
    }

    /// `name` whose 41st line becomes `change` after 40 unchanged lines (the Kit folds them).
    fn folded(name: &str, change: &str) -> String {
        let mut patch = format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,41 +1,41 @@\n");
        patch.extend((0..40).map(|line| format!(" same {line}\n")));
        patch.push_str(&format!("-old\n+{change}\n"));
        patch
    }

    fn parsed(patch: &str) -> Vec<DiffFile> {
        DiffFile::parse(patch).expect("a valid patch")
    }

    fn three_files(cx: &mut gpui_kit::TestAppContext) -> (Entity<DiffState>, &mut gpui_kit::VisualTestContext) {
        let patch: String = ["a.txt", "b.txt", "c.txt"].iter().map(|name| rewritten(name, "")).collect();
        shown(&patch, cx)
    }

    fn shown<'a>(patch: &str, cx: &'a mut gpui_kit::TestAppContext) -> (Entity<DiffState>, &'a mut gpui_kit::VisualTestContext) {
        cx.update(gpui_kit::init);
        let files = parsed(patch);
        let state = cx.new(|cx| DiffState::new(files, cx));
        let (_, cx) = cx.add_window_view({
            let state = state.clone();
            move |_, _| Host { state }
        });
        // The body records its size on the first paint and lays out again.
        for _ in 0..3 {
            draw(cx);
        }
        (state, cx)
    }

    fn header_top(state: &Entity<DiffState>, path: &str, cx: &mut gpui_kit::VisualTestContext) -> Option<Pixels> {
        cx.update(|_, cx| state.read(cx).file_header_top(path))
    }

    #[gpui_kit::test]
    fn the_next_header_takes_the_place_of_a_collapsed_one(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = three_files(cx);
        assert_eq!(header_top(&state, "a.txt", cx), Some(px(0.)));
        assert_eq!(header_top(&state, "b.txt", cx), None, "a screen and more below");

        // The rows above it are estimated until laid out: again once they are on screen.
        for _ in 0..2 {
            state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(100.), cx));
            draw(cx);
        }
        assert_eq!(header_top(&state, "b.txt", cx), Some(px(100.)));

        // Checking off b.txt from its header, 100px down.
        state.update(cx, |s, cx| {
            s.set_file_collapsed("b.txt", true, cx);
            s.scroll_file_header_to("c.txt", px(100.), cx);
        });
        draw(cx);
        assert_eq!(header_top(&state, "c.txt", cx), Some(px(100.)));
    }

    fn lines(side: DiffSide, start: usize, end: usize) -> DiffLineRange {
        DiffLineRange::new("file.txt", side, start, end)
    }

    fn at(side: DiffSide, line: usize) -> Option<DiffLinePosition> {
        Some(DiffLinePosition::new("file.txt", side, line))
    }

    #[test]
    fn only_a_press_that_clicks_a_line_number_comments() {
        let comment = |side, line| Some(("file.txt".to_owned(), side, line));
        let unread = || -> Option<DiffLinePosition> { panic!("only the end of a press reads the line clicked") };
        let one = lines(DiffSide::Modified, 2, 2);
        let mut press = LinePress::Idle;
        assert_eq!(press.follow(&DiffEvent::SelectionStarted(one.clone()), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionChanged(Some(one.clone())), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(one.clone()), || at(DiffSide::Modified, 2)), comment(Side::Modified, 2));
        // Shift+Up/Down and Cmd/Ctrl+A end a selection with no press, also after a click.
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(one.clone()), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(lines(DiffSide::Modified, 1, 4)), unread), None);

        // A press released elsewhere (another line number, the code, a header, outside the
        // diff) clicked no line number, also with its line still the only one selected.
        assert_eq!(press.follow(&DiffEvent::SelectionStarted(one.clone()), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(one.clone()), || None), None);
        assert_eq!(press.follow(&DiffEvent::SelectionStarted(one), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(lines(DiffSide::Modified, 2, 4)), || None), None);

        // A Shift-click selects from the anchor to the line clicked, below or above it, and
        // comments there; on a removed line too, also across sides (Unified).
        let extended = lines(DiffSide::Modified, 1, 3);
        assert_eq!(press.follow(&DiffEvent::SelectionStarted(extended.clone()), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(extended), || at(DiffSide::Modified, 1)), comment(Side::Modified, 1));
        let across = lines(DiffSide::Original, 3, 3).with_end_side(DiffSide::Modified);
        assert_eq!(press.follow(&DiffEvent::SelectionStarted(across.clone()), unread), None);
        assert_eq!(press.follow(&DiffEvent::SelectionEnded(across), || at(DiffSide::Original, 3)), comment(Side::Original, 3));
    }

    /// A one-file diff that comments where `line_click` says, with `found` the find's chosen
    /// match.
    struct Commenting {
        state: Entity<DiffState>,
        comments: Vec<(String, Side, u32)>,
        found: Option<DiffLineRange>,
        _events: Subscription,
    }

    impl gpui_kit::Render for Commenting {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(400.)).h(px(300.)).child(Diff::new(&self.state).size_full())
        }
    }

    #[gpui_kit::test]
    fn a_click_on_a_line_number_comments_and_keyboard_and_drag_selections_do_not(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};

        cx.update(gpui_kit::init);
        let patch = "diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1,4 +1,4 @@\n one\n-two\n+TWO\n three\n four\n";
        let files = DiffFile::parse(patch).expect("a valid patch");
        let state = cx.new(|cx| DiffState::new(files, cx));
        let (host, cx) = cx.add_window_view({
            let state = state.clone();
            move |_, cx| {
                let mut press = LinePress::Idle;
                let events = cx.subscribe(&state, move |host: &mut Commenting, state, event: &DiffEvent, cx| {
                    if let Some(comment) = line_click(&mut press, &state, event, host.found.as_ref(), cx) {
                        host.comments.push(comment);
                    }
                });
                Commenting { state, comments: Vec::new(), found: None, _events: events }
            }
        });
        for _ in 0..3 {
            draw(cx);
        }
        let number = |line: usize, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, _| window.find_by_label(&format!("Select Modified line {line}")).bounds().center())
        };
        let comments = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| host.read(cx).comments.clone());
        let selected = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| state.read(cx).selected_lines());

        let at = number(2, cx);
        cx.simulate_click(at, gpui_kit::Modifiers::none());
        draw(cx);
        assert_eq!(comments(cx), [("file.txt".to_owned(), Side::Modified, 2)]);
        assert_eq!(selected(cx), None, "the clicked line does not stay selected");

        // The click focused the diff: Shift+Down and select-all select lines, nothing more.
        cx.simulate_keystrokes("shift-down");
        draw(cx);
        assert_eq!(selected(cx), Some(lines(DiffSide::Modified, 1, 1)));
        cx.dispatch_action(gpui_kit::component::input::SelectAll);
        draw(cx);
        assert_eq!(selected(cx), Some(lines(DiffSide::Modified, 1, 4)));
        assert_eq!(comments(cx).len(), 1);

        // A drag over line numbers selects them.
        let (from, to) = (number(1, cx), number(3, cx));
        cx.update(|window, cx| window.drag(from, to, cx));
        draw(cx);
        assert_eq!(selected(cx), Some(lines(DiffSide::Modified, 1, 3)));
        assert_eq!(comments(cx).len(), 1);
        // A press on a line number released off the line numbers, in the code or outside the
        // diff, selects that line and comments nowhere: no click lands on a line number.
        let from = number(2, cx);
        for to in [gpui_kit::point(from.x + px(200.), from.y), gpui_kit::point(px(600.), from.y)] {
            cx.update(|window, cx| window.drag(from, to, cx));
            draw(cx);
            assert_eq!(selected(cx), Some(lines(DiffSide::Modified, 2, 2)), "released at {to:?}");
            assert_eq!(comments(cx).len(), 1, "released at {to:?}");
        }

        let at = number(3, cx);
        cx.simulate_click(at, gpui_kit::Modifiers::none());
        draw(cx);
        assert_eq!(comments(cx)[1..], [("file.txt".to_owned(), Side::Modified, 3)]);
        assert_eq!(selected(cx), None);

        // Find chose a match on line 1. A Shift-click on line 4, which the Kit takes to extend
        // the match's selection, comments there too; the match stays highlighted.
        let found = lines(DiffSide::Modified, 1, 1);
        host.update(cx, |host, _| host.found = Some(found.clone()));
        state.update(cx, |s, cx| s.set_selected_lines(Some(found.clone()), cx));
        let at = number(4, cx);
        cx.simulate_click(at, gpui_kit::Modifiers::shift());
        draw(cx);
        assert_eq!(comments(cx)[2..], [("file.txt".to_owned(), Side::Modified, 4)]);
        assert_eq!(selected(cx), Some(found.clone()));
        // And after a plain click.
        let at = number(2, cx);
        cx.simulate_click(at, gpui_kit::Modifiers::none());
        draw(cx);
        assert_eq!(comments(cx)[3..], [("file.txt".to_owned(), Side::Modified, 2)]);
        assert_eq!(selected(cx), Some(found.clone()));

        // A press dragged to another line number and back, released on the one it pressed, is
        // a click there.
        let (at, away) = (number(3, cx), number(4, cx));
        let (left, none) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
        cx.simulate_mouse_down(at, left, none);
        cx.simulate_mouse_move(away, Some(left), none);
        draw(cx);
        assert_eq!(selected(cx), Some(lines(DiffSide::Modified, 3, 4)));
        cx.simulate_mouse_move(at, Some(left), none);
        cx.simulate_mouse_up(at, left, none);
        draw(cx);
        assert_eq!(comments(cx)[4..], [("file.txt".to_owned(), Side::Modified, 3)]);
        assert_eq!(selected(cx), Some(found));
    }

    fn listed(path: &str, staged: Option<Staging>) -> ListedFile {
        ListedFile { path: path.to_owned(), status: FileChangeStatus::Modified, stats: FileChangeStats::Unknown, staged }
    }

    #[test]
    fn a_partially_staged_file_has_one_header_with_both_buttons() {
        let order = [
            listed("a.txt", Some(Staging::Unstaged)),
            listed("b.txt", Some(Staging::Staged)),
            listed("b.txt", Some(Staging::Unstaged)),
            listed("c.txt", Some(Staging::Staged)),
        ];
        assert_eq!(
            file_headers(&order),
            [(0, vec![Staging::Unstaged]), (1, vec![Staging::Staged, Staging::Unstaged]), (3, vec![Staging::Staged])]
        );
        // Committed files have no button.
        assert_eq!(file_headers(&[listed("a.txt", None), listed("b.txt", None)]), [(0, vec![]), (1, vec![])]);
        assert_eq!(file_headers(&[]), []);
    }

    #[gpui_kit::test]
    fn a_header_scrolled_past_is_pinned_at_the_top(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = three_files(cx);
        state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(-60.), cx));
        draw(cx);
        assert_eq!(header_top(&state, "b.txt", cx), Some(px(0.)));
        assert_eq!(header_top(&state, "a.txt", cx), None);
        assert_eq!(header_top(&state, "c.txt", cx), None);
        assert_eq!(header_top(&state, "missing.txt", cx), None);
    }

    /// `file.txt` with its second line `two` changed to `fresh words` (shown unified).
    fn one_change(cx: &mut gpui_kit::TestAppContext) -> (Entity<DiffState>, &mut gpui_kit::VisualTestContext) {
        shown("diff --git a/file.txt b/file.txt\n--- a/file.txt\n+++ b/file.txt\n@@ -1,2 +1,2 @@\n one\n-two\n+fresh words\n", cx)
    }

    type TextAt = Option<(DiffLinePosition, SharedString, usize)>;

    /// The points along the row of `file.txt`'s new line `line`, a pixel apart from its line
    /// number to the right edge, and the text under each.
    fn along(state: &Entity<DiffState>, line: usize, cx: &mut gpui_kit::VisualTestContext) -> Vec<(gpui_kit::Point<Pixels>, TextAt)> {
        use gpui_kit::test::TestQueryExt as _;
        let number = cx.update(|window, _| window.find_by_label(&format!("Select Modified line {line}")).bounds());
        let (left, y) = (f32::from(number.left()) as usize, number.center().y);
        (left..400)
            .map(|x| {
                let at = gpui_kit::point(px(x as f32), y);
                (at, cx.update(|_, cx| state.read(cx).text_at(at)))
            })
            .collect()
    }

    #[gpui_kit::test]
    fn the_text_under_a_point_is_a_character_of_its_line_and_none_beside_it(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = one_change(cx);
        let row = along(&state, 2, cx);
        assert_eq!(row[0].1, None, "the line number is no text");
        // Nor is the row's space after the line's text (well over 100px of it).
        let end = row.iter().rposition(|(_, hit)| hit.is_some()).expect("painted");
        assert!(row.len() - end > 100, "the text ends {} px before the edge", row.len() - end);
        let line = DiffLinePosition::new("file.txt", DiffSide::Modified, 2);
        let mut offsets: Vec<usize> = row
            .iter()
            .filter_map(|(_, hit)| hit.clone())
            .map(|(position, text, offset)| {
                assert_eq!((position, text.as_ref()), (line.clone(), "fresh words"));
                offset
            })
            .collect();
        // Left to right, each character in turn.
        offsets.dedup();
        assert_eq!(offsets, (0.."fresh words".len()).collect::<Vec<_>>());
    }

    #[gpui_kit::test]
    fn a_right_click_offers_the_selection_else_the_word_on_a_files_rows(cx: &mut gpui_kit::TestAppContext) {
        use treeterm_model::review_find::DiffMenu;
        let (state, cx) = one_change(cx);
        let row = along(&state, 2, cx);
        let on_words = row.iter().find(|(_, hit)| hit.as_ref().is_some_and(|(_, _, offset)| *offset == 8)).expect("painted").0;
        let end = row.iter().rposition(|(_, hit)| hit.is_some()).expect("painted");
        let after = row[end + 20].0;
        assert_eq!(row[end + 20].1, None);
        let below = gpui_kit::point(px(200.), px(295.));
        let menu = |at, found: Option<&DiffLineRange>, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| super::super::find::diff_menu_at(&state, at, found, cx))
        };
        let offer = |copy: &str, search: &str| Some((SharedString::from("file.txt"), DiffMenu { copy: copy.into(), search: search.into() }));
        let selected = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| state.read(cx).selected_text(cx));
        // Selects `range`, and gives the diff's selection back (as the find keeps it).
        let select = |range: DiffLineRange, cx: &mut gpui_kit::VisualTestContext| {
            state.update(cx, |s, cx| s.set_selected_lines(Some(range), cx));
            cx.update(|_, cx| state.read(cx).selected_lines()).expect("selected")
        };

        // Nothing selected: the word under the pointer, else no menu.
        assert_eq!(menu(on_words, None, cx), offer("words", "words"));
        assert_eq!(menu(after, None, cx), None);
        assert_eq!(menu(below, None, cx), None);

        // Lines selected: copied wherever the file's rows are right-clicked; the word is searched.
        let two_lines = select(DiffLineRange::new("file.txt", DiffSide::Modified, 1, 2), cx);
        let lines = selected(cx);
        assert!(lines.trim().contains('\n'), "{lines:?}");
        assert_eq!(menu(after, None, cx), offer(&lines, ""));
        assert_eq!(menu(on_words, None, cx), offer(&lines, "words"));
        assert_eq!(menu(below, None, cx), None, "below the files");

        // One line selected: searched too.
        let one_line = select(DiffLineRange::new("file.txt", DiffSide::Modified, 2, 2), cx);
        let line = selected(cx);
        assert_eq!(menu(on_words, None, cx), offer(&line, "fresh words"));

        // The line the find's chosen match selected only highlights it: the word under the
        // pointer, for both, else no menu.
        assert_eq!(menu(on_words, Some(&one_line), cx), offer("words", "words"));
        assert_eq!(menu(after, Some(&one_line), cx), None);
        // Lines selected since are the user's.
        select(two_lines, cx);
        assert_eq!(menu(on_words, Some(&one_line), cx), offer(&lines, "words"));
    }

    #[gpui_kit::test]
    fn the_file_under_a_point_is_the_one_whose_row_is_there(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = three_files(cx);
        // b.txt's header 100px down: a.txt's last rows above it.
        for _ in 0..2 {
            state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(100.), cx));
            draw(cx);
        }
        let file_at = |y: f32, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|_, cx| state.read(cx).file_at(gpui_kit::point(px(200.), px(y))).map(|p| p.to_string()))
        };
        // The rows start under the diff's 1px border.
        assert_eq!(file_at(0., cx), None, "on the border");
        assert_eq!(file_at(1., cx).as_deref(), Some("a.txt"));
        assert_eq!(file_at(100., cx).as_deref(), Some("a.txt"));
        assert_eq!(file_at(101., cx).as_deref(), Some("b.txt"));
        assert_eq!(file_at(298., cx).as_deref(), Some("b.txt"));
        assert_eq!(file_at(299., cx), None, "below the rows");
        assert_eq!(cx.update(|_, cx| state.read(cx).file_at(gpui_kit::point(px(400.), px(50.)))), None, "beside them");
    }

    #[gpui_kit::test]
    fn a_line_scrolled_to_a_third_of_the_way_down_sits_there(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestQueryExt as _;
        let (state, cx) = three_files(cx);
        let line = DiffLinePosition::new("b.txt", DiffSide::Modified, 20);
        let top = |cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, _| f32::from(window.find_by_label("Select Modified line 20").bounds().top()))
        };
        state.update(cx, |s, cx| s.scroll_to_line(line.clone(), cx));
        draw(cx);
        let at_top = top(cx);
        // The rows above it are estimated until laid out: again once they are.
        for _ in 0..2 {
            state.update(cx, |s, cx| s.scroll_to_line_at(line.clone(), 1. / 3., cx));
            draw(cx);
        }
        assert!((top(cx) - at_top - 100.).abs() <= 1., "{} below the top", top(cx) - at_top);

        // At the start of the list it stops there.
        state.update(cx, |s, cx| s.scroll_to_line_at(DiffLinePosition::new("a.txt", DiffSide::Modified, 1), 1. / 3., cx));
        draw(cx);
        assert_eq!(header_top(&state, "a.txt", cx), Some(px(0.)));
    }

    fn band(state: &Entity<DiffState>, cx: &mut gpui_kit::VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| state.read(cx).top_band().iter().map(|p| p.to_string()).collect())
    }

    /// The bands `state` reports from now on.
    fn reports(state: &Entity<DiffState>, cx: &mut gpui_kit::VisualTestContext) -> (Rc<std::cell::RefCell<Vec<Vec<String>>>>, Subscription) {
        let reported = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = reported.clone();
        let subscription = cx.update(|_, cx| {
            cx.subscribe(state, move |_, event: &DiffEvent, _| {
                if let DiffEvent::TopBandChanged(band) = event {
                    seen.borrow_mut().push(band.iter().map(|p| p.to_string()).collect());
                }
            })
        });
        (reported, subscription)
    }

    #[gpui_kit::test]
    fn the_files_in_the_top_tenth_of_the_diff_are_reported(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = three_files(cx);
        assert_eq!(band(&state, cx), ["a.txt"]);
        let (reported, _subscription) = reports(&state, cx);

        // b.txt's header 10px down: the top 30px hold a.txt's last rows and b.txt's header.
        for _ in 0..2 {
            state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(10.), cx));
            draw(cx);
        }
        assert_eq!(header_top(&state, "b.txt", cx), Some(px(10.)));
        assert_eq!(band(&state, cx), ["a.txt", "b.txt"]);
        assert_eq!(reported.borrow().last().cloned(), Some(vec!["a.txt".to_owned(), "b.txt".to_owned()]));

        // Scrolled into b.txt: b.txt alone. A scroll within it reports nothing more.
        state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(-60.), cx));
        draw(cx);
        assert_eq!(band(&state, cx), ["b.txt"]);
        let count = reported.borrow().len();
        state.update(cx, |s, cx| s.scroll_file_header_to("b.txt", px(-120.), cx));
        draw(cx);
        draw(cx);
        assert_eq!(reported.borrow().len(), count);
        assert_eq!(reported.borrow().last().cloned(), Some(vec!["b.txt".to_owned()]));

        // A jump counts too.
        state.update(cx, |s, cx| s.scroll_to_file("c.txt", cx));
        draw(cx);
        assert_eq!(band(&state, cx), ["c.txt"]);
        assert_eq!(reported.borrow().last().cloned(), Some(vec!["c.txt".to_owned()]));
    }

    fn near(state: &Entity<DiffState>, cx: &mut gpui_kit::VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| state.read(cx).near_files().iter().map(|p| p.to_string()).collect())
    }

    #[gpui_kit::test]
    fn the_files_within_1500px_of_the_viewport_are_reported_near(cx: &mut gpui_kit::TestAppContext) {
        // Five files, each far taller than the 300px viewport.
        let patch: String = ["a.txt", "b.txt", "c.txt", "d.txt", "e.txt"].iter().map(|name| rewritten(name, "")).collect();
        let (state, cx) = shown(&patch, cx);
        let reported = Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = reported.clone();
        let _subscription = cx.update(|_, cx| {
            cx.subscribe(&state, move |_, event: &DiffEvent, _| {
                if let DiffEvent::NearFilesChanged(near) = event {
                    seen.borrow_mut().push(near.iter().map(|p| p.to_string()).collect::<Vec<_>>());
                }
            })
        });
        // At the top: a.txt, and b.txt whose header is within 1500px below; not c.txt.
        assert_eq!(near(&state, cx), ["a.txt", "b.txt"]);

        // In c.txt: b.txt's last rows above it, d.txt's header below it.
        state.update(cx, |s, cx| s.scroll_to_file("c.txt", cx));
        draw(cx);
        draw(cx);
        assert_eq!(near(&state, cx), ["b.txt", "c.txt", "d.txt"]);
        assert_eq!(reported.borrow().last().cloned(), Some(vec!["b.txt".to_owned(), "c.txt".to_owned(), "d.txt".to_owned()]));

        // A collapsed file counts by its header: with b.txt and c.txt collapsed, a.txt's
        // rows, those headers and d.txt's from the top.
        state.update(cx, |s, cx| {
            s.set_file_collapsed("b.txt", true, cx);
            s.set_file_collapsed("c.txt", true, cx);
            s.scroll_to_file("a.txt", cx);
        });
        draw(cx);
        draw(cx);
        assert_eq!(near(&state, cx), ["a.txt", "b.txt", "c.txt", "d.txt"]);

        // Other files are reported afresh.
        let count = reported.borrow().len();
        state.update(cx, |s, cx| s.update_files(parsed(&patch), [], [], cx));
        draw(cx);
        draw(cx);
        assert!(reported.borrow().len() > count);
    }

    #[gpui_kit::test]
    fn replaced_files_keep_the_place_collapse_and_selection(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = three_files(cx);
        // a.txt collapsed by hand, c.txt's header 100px down (b.txt's rows above it), and
        // lines of b.txt selected.
        let selected = DiffLineRange::new("b.txt", DiffSide::Modified, 2, 3);
        state.update(cx, |s, cx| {
            s.set_file_collapsed("a.txt", true, cx);
            s.set_selected_lines(Some(selected.clone()), cx);
        });
        for _ in 0..2 {
            state.update(cx, |s, cx| s.scroll_file_header_to("c.txt", px(100.), cx));
            draw(cx);
        }
        assert_eq!(header_top(&state, "c.txt", cx), Some(px(100.)));
        let (reported, _subscription) = reports(&state, cx);

        // a.txt changes, d.txt comes in collapsed and e.txt open.
        let patch: String = [rewritten("a.txt", "x"), rewritten("b.txt", ""), rewritten("c.txt", ""), rewritten("d.txt", ""), rewritten("e.txt", "")].concat();
        state.update(cx, |s, cx| s.update_files(parsed(&patch), [SharedString::from("d.txt")], [], cx));
        draw(cx);
        draw(cx);
        assert_eq!(header_top(&state, "c.txt", cx), Some(px(100.)), "the same row of b.txt at the top");
        let collapsed = |path: &str, cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| state.read(cx).is_file_collapsed(path));
        assert!(collapsed("a.txt", cx));
        assert!(collapsed("d.txt", cx));
        assert!(!collapsed("e.txt", cx));
        assert_eq!(cx.update(|_, cx| state.read(cx).selected_lines()), Some(selected), "b.txt did not change");
        // The band is reported afresh, though it is the same.
        assert_eq!(*reported.borrow(), [vec!["b.txt".to_owned()]]);

        // b.txt changes: its selection goes.
        let patch: String = [rewritten("a.txt", "x"), rewritten("b.txt", "x"), rewritten("c.txt", "")].concat();
        state.update(cx, |s, cx| s.update_files(parsed(&patch), [], [], cx));
        draw(cx);
        assert_eq!(cx.update(|_, cx| state.read(cx).selected_lines()), None);
        assert!(collapsed("a.txt", cx));

        // b.txt goes: the next file still listed comes to the top.
        let patch: String = [rewritten("a.txt", "x"), rewritten("c.txt", "")].concat();
        state.update(cx, |s, cx| s.update_files(parsed(&patch), [], [], cx));
        draw(cx);
        assert_eq!(header_top(&state, "c.txt", cx), Some(px(0.)));
        // And with none after it, the last one before it.
        state.update(cx, |s, cx| s.update_files(parsed(&rewritten("a.txt", "x")), [], [], cx));
        draw(cx);
        assert_eq!(header_top(&state, "a.txt", cx), Some(px(0.)));
    }

    /// `name` with its two lines rewritten to `<new> one` and `<new> two`.
    fn small(name: &str, new: &str) -> String {
        format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,2 +1,2 @@\n-old one\n-old two\n+{new} one\n+{new} two\n")
    }

    /// a.txt rewritten to `a`, b.txt to `b` ([`small`]).
    fn two_small(a: &str, b: &str) -> String {
        [small("a.txt", a), small("b.txt", b)].concat()
    }

    /// The middle of the left edge of the character `offset` of `path`'s new line `line`.
    fn char_at(state: &Entity<DiffState>, path: &str, line: usize, offset: usize, cx: &mut gpui_kit::VisualTestContext) -> gpui_kit::Point<Pixels> {
        let want = DiffLinePosition::new(path, DiffSide::Modified, line);
        let over: Vec<(usize, usize)> = cx.update(|_, cx| {
            let state = state.read(cx);
            (0..300)
                .flat_map(|y| (0..400).map(move |x| (x, y)))
                .filter(|(x, y)| {
                    let at = gpui_kit::point(px(*x as f32), px(*y as f32));
                    state.text_at(at).is_some_and(|(position, _, at)| position == want && at == offset)
                })
                .collect()
        });
        let left = over.iter().map(|(x, _)| *x).min().unwrap_or_else(|| panic!("{path}:{line} has no character {offset} painted"));
        let ys: Vec<usize> = over.iter().filter(|(x, _)| *x == left).map(|(_, y)| *y).collect();
        gpui_kit::point(px(left as f32), px(ys[ys.len() / 2] as f32))
    }

    /// [`shown`] in a window as the app opens one (under GPUI Kit's root), where text can be
    /// selected.
    fn selectable<'a>(patch: &str, cx: &'a mut gpui_kit::TestAppContext) -> (Entity<DiffState>, &'a mut gpui_kit::VisualTestContext) {
        let files = parsed(patch);
        let (window, host) = cx.update(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(gpui_kit::WindowOptions::default(), cx, move |_, cx| {
                let state = cx.new(|cx| DiffState::new(files, cx));
                cx.new(|_| Host { state })
            })
            .expect("open a window")
        });
        let state = host.read_with(cx, |host, _| host.state.clone());
        let cx = gpui_kit::VisualTestContext::from_window(window, cx).into_mut();
        cx.update(|window, _| window.activate_window());
        for _ in 0..3 {
            draw(cx);
        }
        (state, cx)
    }

    #[gpui_kit::test]
    fn text_selected_in_a_file_left_as_it_was_stays_selected(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = selectable(&two_small("new", "new"), cx);
        let selected = |cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| state.read(cx).selected_text(cx));
        let (left, none) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
        // A drag over b.txt's text, still held, while a.txt's diff is read again: it goes on.
        let (from, to) = (char_at(&state, "b.txt", 1, 4, cx), char_at(&state, "b.txt", 1, 6, cx));
        cx.simulate_mouse_down(from, left, none);
        draw(cx);
        cx.simulate_mouse_move(to, Some(left), none);
        draw(cx);
        assert_eq!(selected(cx), "on");
        state.update(cx, |s, cx| s.update_files(parsed(&two_small("newer", "new")), [], [], cx));
        draw(cx);
        assert_eq!(selected(cx), "on");
        let to = char_at(&state, "b.txt", 2, 3, cx);
        cx.simulate_mouse_move(to, Some(left), none);
        cx.simulate_mouse_up(to, left, none);
        draw(cx);
        assert_eq!(selected(cx), "one\nnew");

        // It stays as a.txt changes again, and goes when b.txt does.
        state.update(cx, |s, cx| s.update_files(parsed(&two_small("newest", "new")), [], [], cx));
        draw(cx);
        assert_eq!(selected(cx), "one\nnew");
        state.update(cx, |s, cx| s.update_files(parsed(&two_small("newest", "newer")), [], [], cx));
        draw(cx);
        assert_eq!(selected(cx), "");

        // Text selected in b.txt goes too when a file before it goes (its place changes).
        let (from, to) = (char_at(&state, "b.txt", 1, 6, cx), char_at(&state, "b.txt", 2, 5, cx));
        cx.simulate_mouse_down(from, left, none);
        draw(cx);
        cx.simulate_mouse_move(to, Some(left), none);
        cx.simulate_mouse_up(to, left, none);
        draw(cx);
        assert_eq!(selected(cx), "one\nnewer");
        state.update(cx, |s, cx| s.update_files(parsed(&small("b.txt", "newer")), [], [], cx));
        draw(cx);
        assert_eq!(selected(cx), "");
    }

    #[gpui_kit::test]
    fn a_line_number_drag_goes_on_while_another_files_diff_changes(cx: &mut gpui_kit::TestAppContext) {
        use gpui_kit::test::TestQueryExt as _;
        let (state, cx) = shown(&two_small("new", "new"), cx);
        // b.txt's line number `line`: the lower of the two.
        let number = |line: usize, cx: &mut gpui_kit::VisualTestContext| {
            cx.update(|window, _| {
                let found = window.find_all_by_label(&format!("Select Modified line {line}"));
                found.iter().map(|n| n.bounds().center()).max_by(|a, b| f32::from(a.y).total_cmp(&f32::from(b.y))).expect("drawn")
            })
        };
        let (left, none) = (gpui_kit::MouseButton::Left, gpui_kit::Modifiers::none());
        let from = number(1, cx);
        cx.simulate_mouse_down(from, left, none);
        draw(cx);
        state.update(cx, |s, cx| s.update_files(parsed(&two_small("newer", "new")), [], [], cx));
        draw(cx);
        let to = number(2, cx);
        cx.simulate_mouse_move(to, Some(left), none);
        cx.simulate_mouse_up(to, left, none);
        draw(cx);
        let selected = cx.update(|_, cx| state.read(cx).selected_lines());
        assert_eq!(selected, Some(DiffLineRange::new("b.txt", DiffSide::Modified, 1, 2)));
    }

    #[gpui_kit::test]
    fn files_opened_as_their_rows_arrive_keep_the_place(cx: &mut gpui_kit::TestAppContext) {
        let small = |name: &str| format!("diff --git a/{name} b/{name}\n--- a/{name}\n+++ b/{name}\n@@ -1,2 +1,2 @@\n-x\n-y\n+X\n+Y\n");
        let patch: String = ["a.txt", "b.txt", "c.txt"].iter().map(|name| small(name)).collect();
        // Bare headers, collapsed (their bodies drawn by the owner), the first at the top.
        let headers: String = ["a.txt", "b.txt", "c.txt"].iter().map(|name| bare_header(name)).collect();
        let (state, cx) = shown(&headers, cx);
        state.update(cx, |s, cx| {
            for name in ["a.txt", "b.txt", "c.txt"] {
                s.set_file_collapsed(name, true, cx);
            }
        });
        draw(cx);
        // The rows of a.txt and b.txt arrive, opened; c.txt stays a collapsed header.
        state.update(cx, |s, cx| s.update_files(parsed(&patch), [], ["a.txt", "b.txt"].map(SharedString::from), cx));
        draw(cx);
        draw(cx);
        let collapsed = |path: &str, cx: &mut gpui_kit::VisualTestContext| cx.update(|_, cx| state.read(cx).is_file_collapsed(path));
        assert!(!collapsed("a.txt", cx) && !collapsed("b.txt", cx) && collapsed("c.txt", cx));
        let kept = header_top(&state, "c.txt", cx);
        assert!(kept.is_some_and(|top| top > px(0.)), "{kept:?}");
        // a.txt's header is still at the top: as the same files laid out afresh from the start.
        state.update(cx, |s, cx| {
            s.set_files(parsed(&patch), cx);
            s.scroll_to_file("a.txt", cx);
        });
        draw(cx);
        draw(cx);
        assert_eq!(header_top(&state, "c.txt", cx), kept);
    }

    #[gpui_kit::test]
    fn an_unchanged_file_keeps_its_opened_folds(cx: &mut gpui_kit::TestAppContext) {
        let (state, cx) = shown(&[folded("a.txt", "new"), folded("b.txt", "new")].concat(), cx);
        assert!(header_top(&state, "b.txt", cx).is_some(), "a.txt folds its unchanged lines");
        // Opening a.txt's fold pushes b.txt off the screen.
        state.update(cx, |s, cx| s.scroll_to_line(DiffLinePosition::new("a.txt", DiffSide::Modified, 1), cx));
        state.update(cx, |s, cx| s.scroll_to_file("a.txt", cx));
        draw(cx);
        assert_eq!(header_top(&state, "b.txt", cx), None);

        // b.txt changes: a.txt's fold stays open.
        state.update(cx, |s, cx| s.update_files(parsed(&[folded("a.txt", "new"), folded("b.txt", "newer")].concat()), [], [], cx));
        draw(cx);
        assert_eq!(header_top(&state, "a.txt", cx), Some(px(0.)));
        assert_eq!(header_top(&state, "b.txt", cx), None);
        // a.txt changes: folded again.
        state.update(cx, |s, cx| s.update_files(parsed(&[folded("a.txt", "newer"), folded("b.txt", "newer")].concat()), [], [], cx));
        draw(cx);
        assert!(header_top(&state, "b.txt", cx).is_some());

        // Every unchanged line shown: a file that changes shows all of its own too.
        state.update(cx, |s, cx| s.expand_unchanged(cx));
        state.update(cx, |s, cx| s.scroll_to_file("a.txt", cx));
        draw(cx);
        assert_eq!(header_top(&state, "b.txt", cx), None);
        state.update(cx, |s, cx| s.update_files(parsed(&[folded("a.txt", "newest"), folded("b.txt", "newer")].concat()), [], [], cx));
        draw(cx);
        assert_eq!(header_top(&state, "b.txt", cx), None);
    }

    /// The diffs of `paths` as one command read them from `patch`.
    fn read(paths: &[&str], patch: &str) -> FileDiffs {
        let mut diffs = FileDiffs::not_loaded(paths.iter().copied());
        diffs.set_read(paths.iter().copied(), &Ok(patch.to_owned()));
        diffs
    }

    fn drawn(parsed: &Parsed) -> Vec<(&str, &Placeholder)> {
        let mut drawn: Vec<(&str, &Placeholder)> = parsed.drawn.iter().map(|(path, p)| (path.as_str(), p)).collect();
        drawn.sort_by_key(|(path, _)| *path);
        drawn
    }

    /// Whether each shown file has rows in the `DiffState`.
    fn rows(parsed: &Parsed) -> Vec<(String, bool)> {
        parsed.files.iter().map(|f| (f.path().to_string(), f.additions() + f.deletions() > 0)).collect()
    }

    #[test]
    fn the_listed_files_are_parsed_in_list_order_once() {
        let patch: String = [rewritten("a.txt", ""), rewritten("b.txt", ""), rewritten("c.txt", "")].concat();
        let order = [listed("c.txt", None), listed("a.txt", Some(Staging::Staged)), listed("a.txt", Some(Staging::Unstaged))];
        let none = HashSet::new();
        let got = parse_listed(&read(&["a.txt", "b.txt", "c.txt"], &patch), &order, &none).expect("headers");
        // b.txt is not listed (say, filtered out); a.txt is listed twice, shown once.
        assert_eq!(got.shown, ["c.txt", "a.txt"]);
        assert_eq!(got.files.iter().map(|f| f.path().to_string()).collect::<Vec<_>>(), ["c.txt", "a.txt"]);
        assert!(got.drawn.is_empty());
        // A file git printed no section for is not shown.
        let got = parse_listed(&read(&["a.txt", "gone.txt"], &patch), &[listed("gone.txt", None), listed("a.txt", None)], &none);
        assert_eq!(got.expect("headers").shown, ["a.txt"]);

        // A file whose diff is not loaded yet, loading or failed has a header, its body drawn
        // by the view.
        let mut diffs = FileDiffs::not_loaded(["c.txt", "b.txt", "d.txt", "a.txt"]);
        diffs.set("c.txt", FileDiff::Loading);
        diffs.set("b.txt", FileDiff::Failed("fatal: bad object".to_owned()));
        diffs.set("a.txt", FileDiff::Loaded(rewritten("a.txt", "")));
        let order = [listed("a.txt", None), listed("b.txt", None), listed("c.txt", None), listed("d.txt", None)];
        let got = parse_listed(&diffs, &order, &none).expect("headers");
        assert_eq!(got.shown, ["a.txt", "b.txt", "c.txt", "d.txt"]);
        assert_eq!(
            drawn(&got),
            [
                ("b.txt", &Placeholder::Failed("fatal: bad object".to_owned())),
                ("c.txt", &Placeholder::Loading),
                ("d.txt", &Placeholder::NotLoaded),
            ]
        );
        assert_eq!(
            rows(&got),
            [("a.txt", true), ("b.txt", false), ("c.txt", false), ("d.txt", false)].map(|(path, rows)| (path.to_owned(), rows))
        );

        let tree: Vec<String> = tree_sorted(&[listed("z.txt", None), listed("dir/b.txt", None), listed("a.txt", None)])
            .into_iter()
            .map(|f| f.path)
            .collect();
        assert_eq!(tree, ["dir/b.txt", "a.txt", "z.txt"], "folders first, as the tree");
    }

    #[test]
    fn each_section_shows_its_rows_or_a_body_of_its_own() {
        let bad = "diff --git a/bad.txt b/bad.txt\n--- a/bad.txt\n+++ b/bad.txt\n@@ nonsense\n";
        let binary = "diff --git a/data.bin b/data.bin\nindex 1..2 100644\nBinary files a/data.bin and b/data.bin differ\n";
        let png = binary.replace("data.bin", "logo.png");
        // An SVG git diffs as text.
        let svg = rewritten("icon.svg", "");
        let mut big = "diff --git a/big.txt b/big.txt\n--- a/big.txt\n+++ b/big.txt\n@@ -0,0 +1,20000 @@\n".to_owned();
        big.extend((0..20_000).map(|line| format!("+line {line}\n")));
        let patch = [rewritten("a.txt", ""), bad.to_owned(), binary.to_owned(), png, svg, big, rewritten("z.txt", "")].concat();
        let paths = ["a.txt", "bad.txt", "data.bin", "logo.png", "icon.svg", "big.txt", "z.txt"];
        let order: Vec<ListedFile> = paths.iter().map(|path| listed(path, None)).collect();
        let diffs = read(&paths, &patch);
        let got = parse_listed(&diffs, &order, &HashSet::new()).expect("headers");
        assert_eq!(got.shown, paths);
        // The section that does not parse shows why; the others still show.
        let Some(Placeholder::Failed(error)) = got.drawn.get("bad.txt") else {
            panic!("bad.txt failed: {:?}", got.drawn.get("bad.txt"));
        };
        assert!(error.starts_with("Cannot show this diff: patch line 4: "), "{error}");
        let bytes = (0..20_000).map(|line| format!("line {line}\n").len()).sum::<usize>();
        assert_eq!(
            drawn(&got),
            [
                ("bad.txt", &Placeholder::Failed(error.clone())),
                ("big.txt", &Placeholder::TooLarge(DiffSize { lines: 20_001, bytes, oversized: true })),
                ("data.bin", &Placeholder::Binary),
                ("icon.svg", &Placeholder::Image),
                ("logo.png", &Placeholder::Image),
            ]
        );
        let with_rows: Vec<String> = rows(&got).into_iter().filter(|(_, rows)| *rows).map(|(path, _)| path).collect();
        assert_eq!(with_rows, ["a.txt", "z.txt"], "files drawn by the view have no rows");

        // "Load anyway": its rows.
        let got = parse_listed(&diffs, &order, &HashSet::from(["big.txt".to_owned()])).expect("headers");
        assert!(!got.drawn.contains_key("big.txt"));
        assert!(rows(&got).contains(&("big.txt".to_owned(), true)));
    }

    #[test]
    fn a_list_is_kept_until_another_view_or_commit_shows() {
        let order = ["a.txt".to_owned(), "b.txt".to_owned()];
        let committed = Some((&DiffList::Committed, "key", &order[..]));
        // Nothing cached, or another list: a new one.
        assert_eq!(list_change(None, &DiffList::Committed, "key"), ListChange::New);
        assert_eq!(list_change(committed, &DiffList::Uncommitted, "key"), ListChange::New);
        let first = DiffList::Commit("1".to_owned());
        assert_eq!(list_change(Some((&first, "key", &order[..])), &DiffList::Commit("2".to_owned()), "key"), ListChange::New);
        // The same list: kept as it is, or its files replaced (those listed before given).
        assert_eq!(list_change(committed, &DiffList::Committed, "key"), ListChange::Same);
        assert_eq!(
            list_change(committed, &DiffList::Committed, "other"),
            ListChange::Files(HashSet::from(["a.txt".to_owned(), "b.txt".to_owned()]))
        );
        assert_eq!(list_change(Some((&first, "key", &order[..])), &first, "other"), ListChange::Files(order.into_iter().collect()));
    }

    fn path_set(paths: &[&str]) -> HashSet<String> {
        paths.iter().map(|path| (*path).to_owned()).collect()
    }

    #[test]
    fn load_anyway_lasts_while_the_same_list_shows_the_file() {
        let order = [listed("a.txt", None), listed("big.txt", None), listed("huge.txt", None)];
        // The same list: the files it still lists keep it, one it no longer lists forgets it.
        for list in [DiffList::Committed, DiffList::Uncommitted, DiffList::Commit("1".to_owned())] {
            let mut forced = path_set(&["big.txt", "gone.txt"]);
            keep_forced(Some(&list), &list, &order, &mut forced);
            assert_eq!(forced, path_set(&["big.txt"]), "{list:?}");
        }
        let mut forced = path_set(&["big.txt", "huge.txt"]);
        keep_forced(Some(&DiffList::Committed), &DiffList::Committed, &order[1..], &mut forced);
        assert_eq!(forced, path_set(&["big.txt", "huge.txt"]), "a file listed again keeps it");
        keep_forced(Some(&DiffList::Committed), &DiffList::Committed, &order[..1], &mut forced);
        assert!(forced.is_empty(), "files no longer listed forget it");

        // Another view, another commit, or no list shown before: every file forgets it.
        let first = DiffList::Commit("1".to_owned());
        for (cached, list) in [
            (Some(&DiffList::Committed), DiffList::Uncommitted),
            (Some(&DiffList::Uncommitted), DiffList::Committed),
            (Some(&DiffList::Committed), first.clone()),
            (Some(&first), DiffList::Commit("2".to_owned())),
            (None, DiffList::Committed),
            (None, first.clone()),
        ] {
            let mut forced = path_set(&["a.txt", "big.txt"]);
            keep_forced(cached, &list, &order, &mut forced);
            assert!(forced.is_empty(), "{cached:?} then {list:?}: {forced:?}");
        }
    }

    #[test]
    fn load_anyway_rebuilds_the_files_once() {
        let order = [listed("a.txt", None), listed("big.txt", None)];
        let shown: Vec<String> = order.iter().map(|f| f.path.clone()).collect();
        let diffs = FileDiffs::not_loaded(["a.txt", "big.txt"]);
        let none = HashSet::new();
        let key = list_key("patch", &diffs, &order, &none);
        let cached = Some((&DiffList::Committed, key.as_str(), &shown[..]));
        assert_eq!(list_change(cached, &DiffList::Committed, &list_key("patch", &diffs, &order, &none)), ListChange::Same);
        // A click on "Load anyway": the files are replaced (their rows drawn), not a new list.
        let forced = path_set(&["big.txt"]);
        let loaded = list_key("patch", &diffs, &order, &forced);
        assert_ne!(loaded, key);
        assert_eq!(list_change(cached, &DiffList::Committed, &loaded), ListChange::Files(path_set(&["a.txt", "big.txt"])));
        // Then the list stays as it is.
        let cached = Some((&DiffList::Committed, loaded.as_str(), &shown[..]));
        assert_eq!(list_change(cached, &DiffList::Committed, &list_key("patch", &diffs, &order, &forced)), ListChange::Same);
        // The choice forgotten (`keep_forced`): the files are replaced again, the notice drawn.
        assert_eq!(list_change(cached, &DiffList::Committed, &key), ListChange::Files(path_set(&["a.txt", "big.txt"])));

        // Whatever order the files were loaded in (each set iterates in its own order).
        let one = path_set(&["a.txt", "big.txt", "c.txt", "d.txt", "e.txt", "f.txt"]);
        let other = path_set(&["f.txt", "e.txt", "d.txt", "c.txt", "big.txt", "a.txt"]);
        assert_eq!(list_key("patch", &diffs, &order, &one), list_key("patch", &diffs, &order, &other));
        // The patch and the listed files are part of the key too.
        assert_ne!(list_key("other", &diffs, &order, &forced), loaded);
        assert_ne!(list_key("patch", &diffs, &order[..1], &forced), loaded);
    }

    #[test]
    fn a_file_asked_for_read_or_failed_rebuilds_the_files() {
        let order = [listed("a.txt", None)];
        let none = HashSet::new();
        // Each body drawn under the same bare header, then the file's rows: a key each.
        let mut keys = Vec::new();
        for diff in [
            FileDiff::NotLoaded,
            FileDiff::Loading,
            FileDiff::Failed("fatal: one".to_owned()),
            FileDiff::Failed("fatal: two".to_owned()),
            FileDiff::Loaded(String::new()),
        ] {
            let mut diffs = FileDiffs::not_loaded(["a.txt"]);
            diffs.set("a.txt", diff);
            keys.push(list_key(&bare_header("a.txt"), &diffs, &order, &none));
        }
        let distinct: HashSet<&String> = keys.iter().collect();
        assert_eq!(distinct.len(), keys.len(), "{keys:?}");
    }
}
