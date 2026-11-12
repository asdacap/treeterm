//! The diff beside the file list (`StackedDiffList` / `FileDiffSection` /
//! `PierreDiffViewer`): "N/M viewed" and "Collapse all" above GPUI Kit's `Diff`, whose file
//! headers are drawn here (collapse chevron, status, path, stats, Stage, refresh, Viewed),
//! with image files shown under their header and comments under their lines.

use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::diff::{
    Diff, DiffAnnotation, DiffChangeIndicator, DiffDecoration, DiffEvent, DiffFile, DiffHunkSeparator, DiffLinePosition,
    DiffLineRange, DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, Bounds, ClickEvent, Context, Element, ElementId, GlobalElementId, InspectorElementId, LayoutId, Pixels, Entity, MouseButton, SharedString, Subscription,
    WeakEntity, Window, div, px, rgb,
};
use treeterm_core::git::{FileChangeStats, FileChangeStatus, Staging, UncommittedFile};
use treeterm_core::review::{ReviewComment, Side};
use treeterm_model::review::{CommentDraft, GitOp, ImageSides, ReviewModel, StageTarget};

use super::ReviewView;
use crate::comment_ui::{CommentActions, Placement, comment_display, comment_input};
use super::parts::{change_stats, checkbox, outline_button, plain_icon, spinner, status_badge};
use crate::file_browser_view::language_for;
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

/// The parsed patch shown, for one patch and one filtered, tree-ordered file list.
pub struct DiffCache {
    key: String,
    pub state: Entity<DiffState>,
    order: Vec<String>,
    /// Binary files: always collapsed in the `DiffState` (no "binary file" notice); the
    /// view folds them itself (`ReviewView::closed_binary`).
    binary: HashSet<String>,
    /// Viewed files already collapsed (a file collapses when it becomes viewed).
    collapsed_viewed: HashSet<String>,
    _events: Subscription,
}

impl DiffCache {
    /// The files in the diff's order.
    pub fn paths(&self) -> Vec<String> {
        self.order.clone()
    }
}

/// The comment being typed, and the draft it belongs to.
pub struct DraftInput {
    draft: CommentDraft,
    input: Entity<TextareaState>,
    text: String,
    _events: Subscription,
}

/// What an annotation of the diff shows.
enum Annotated {
    /// A file's header (and, for an image, both sides under it).
    Header(Header),
    /// The comments under one line, and the comment box when the draft is there.
    Comments { comments: Vec<ReviewComment>, draft: bool },
}

struct Header {
    /// A binary file and whether the view has it folded.
    binary: Option<bool>,
    file: ListedFile,
    viewed: bool,
    /// The files above it (right-click "Viewed" marks them).
    above: Vec<String>,
    image: Option<ImageSides>,
}

impl ReviewView {
    /// The stacked diff of `patch`, limited to `files` (already filtered), in tree order.
    /// `comments`: the Committed and Uncommitted views take comments, a commit's diff does not.
    pub(super) fn stacked(
        &mut self,
        patch: &str,
        files: &[ListedFile],
        viewed: &HashSet<String>,
        comments: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, border, bg_primary) = (c.text_secondary, c.text_primary, c.border, c.bg_primary);
        let entries: Vec<(&str, FileChangeStats)> = files.iter().map(|f| (f.path.as_str(), f.stats)).collect();
        let order: Vec<ListedFile> = treeterm_model::review_tree::tree_order(&treeterm_model::review_tree::build_tree(&entries))
            .into_iter()
            .map(|index| files[index].clone())
            .collect();
        let state = match self.diff_state(patch, &order, viewed, cx) {
            Ok(state) => state,
            Err(text) => return Self::message(format!("Cannot show this diff: {text}"), rgb(super::parts::ERROR_RED).into(), 20.),
        };
        let shown_viewed = order.iter().filter(|f| viewed.contains(&f.path)).count();
        let all_viewed = !order.is_empty() && shown_viewed == order.len();
        let paths: Vec<String> = order.iter().map(|f| f.path.clone()).collect();
        let listed = paths.clone();
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
                h_flex()
                    .id("check-all-viewed")
                    .gap(px(6.))
                    .items_center()
                    .cursor_pointer()
                    .child(checkbox(all_viewed, cx))
                    .child(format!("{shown_viewed}/{} viewed", order.len()))
                    .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                        view.model.update(cx, |m, cx| m.set_files_viewed(&paths, !all_viewed, cx));
                    })),
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
                    .cursor_pointer()
                    .child(if self.all_collapsed { "Expand all" } else { "Collapse all" })
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                        view.all_collapsed = !view.all_collapsed;
                        let collapsed = view.all_collapsed;
                        if let Some(diff) = &view.diff {
                            let (paths, binary) = (diff.paths(), diff.binary.clone());
                            diff.state.update(cx, |s, cx| {
                                for path in paths.iter().filter(|p| !binary.contains(*p)) {
                                    s.set_file_collapsed(path, collapsed, cx);
                                }
                            });
                            if collapsed {
                                view.closed_binary.extend(binary);
                            } else {
                                view.closed_binary.clear();
                            }
                        }
                        cx.notify();
                    })),
            );
        let diff = self.diff_element(&state, patch, &order, viewed, comments, window, cx);
        let diff = self.with_diff_menu(diff, &state, cx);
        let find = self.find_panel(&state, patch, &listed, window, cx);
        v_flex().size_full().child(header).child(div().flex_1().min_h_0().child(diff)).children(find).into_any_element()
    }

    /// The cached `DiffState`, rebuilt when the patch or the listed files change. New
    /// viewed files collapse (`FileDiffSection` starts collapsed when viewed).
    fn diff_state(&mut self, patch: &str, order: &[ListedFile], viewed: &HashSet<String>, cx: &mut Context<Self>) -> Result<Entity<DiffState>, String> {
        let key = format!("{patch}\u{0}{}", order.iter().map(|f| f.path.as_str()).collect::<Vec<_>>().join("\u{0}"));
        if self.diff.as_ref().is_none_or(|diff| diff.key != key) {
            let mut parsed: HashMap<String, DiffFile> = HashMap::new();
            for file in DiffFile::parse(patch).map_err(|e| e.to_string())? {
                let path = file.path().to_string();
                let language = language_for(&path);
                parsed.entry(path).or_insert_with(|| file.with_language(language));
            }
            let mut files = Vec::new();
            let mut shown = Vec::new();
            let mut binary = HashSet::new();
            for listed in order {
                if let Some(file) = parsed.remove(&listed.path) {
                    if file.is_binary() {
                        binary.insert(listed.path.clone());
                    }
                    files.push(file);
                    shown.push(listed.path.clone());
                }
            }
            let mode = if self.split { DiffMode::Split } else { DiffMode::Unified };
            let state = cx.new(|cx| DiffState::new(files, cx)
                    .with_mode(mode)
                    .with_context_lines(Some(DIFF_CONTEXT))
                    .with_min_collapsed_lines(MIN_COLLAPSED_LINES));
            let top = shown.first().cloned();
            state.update(cx, |s, cx| {
                for path in &binary {
                    s.set_file_collapsed(path, true, cx);
                }
                // Folding re-anchors on the first code row; start at the first header.
                if let Some(top) = &top {
                    s.scroll_to_file(top, cx);
                }
            });
            if !self.hide_unchanged {
                state.update(cx, |s, cx| s.expand_unchanged(cx));
            }
            let comments = cx.subscribe(&state, |view, state, event: &DiffEvent, cx| {
                // A click on a line number (`onLineNumberClick`) starts a comment there; the
                // line does not stay selected.
                if let DiffEvent::SelectionEnded(range) = event {
                    state.update(cx, |s, cx| s.set_selected_lines(None, cx));
                    let side = match range.end_side() {
                        DiffSide::Original => Side::Original,
                        DiffSide::Modified => Side::Modified,
                    };
                    let line = u32::try_from(range.end()).unwrap_or(u32::MAX);
                    let path = range.path().to_string();
                    view.model.update(cx, |m, cx| m.start_comment(&path, side, line, cx));
                }
            });
            self.diff = Some(DiffCache { key, state, order: shown, binary, collapsed_viewed: HashSet::new(), _events: comments });
        }
        let Some(diff) = &mut self.diff else {
            unreachable!("made above")
        };
        let newly: Vec<String> =
            diff.order.iter().filter(|p| viewed.contains(*p) && !diff.collapsed_viewed.contains(*p)).cloned().collect();
        diff.collapsed_viewed.retain(|p| viewed.contains(p));
        if !newly.is_empty() {
            diff.collapsed_viewed.extend(newly.iter().cloned());
            let binary = diff.binary.clone();
            diff.state.update(cx, |s, cx| {
                for path in newly.iter().filter(|p| !binary.contains(*p)) {
                    s.set_file_collapsed(path, true, cx);
                }
            });
            self.closed_binary.extend(newly.into_iter().filter(|p| binary.contains(p)));
        }
        Ok(diff.state.clone())
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
        let events = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
            InputEvent::PressEnter { secondary: true, .. } => {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.submit_comment(&text, cx));
            }
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
        order: &[ListedFile],
        viewed: &HashSet<String>,
        with_comments: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut annotations: Vec<DiffAnnotation> = Vec::new();
        let mut items: HashMap<ElementId, Annotated> = HashMap::new();
        let binary = self.diff.as_ref().map(|diff| diff.binary.clone()).unwrap_or_default();
        let model = self.model.read(cx);
        for (index, file) in order.iter().enumerate() {
            let id = ElementId::from(SharedString::from(format!("header-{}", file.path)));
            annotations.push(DiffAnnotation::file(id.clone(), file.path.clone()));
            items.insert(
                id,
                Annotated::Header(Header {
                    binary: binary.contains(&file.path).then(|| self.closed_binary.contains(&file.path)),
                    file: file.clone(),
                    viewed: viewed.contains(&file.path),
                    above: order[..index].iter().map(|f| f.path.clone()).collect(),
                    image: model.image(&file.path).cloned(),
                }),
            );
        }
        let draft = if with_comments { model.draft().cloned() } else { None };
        let adding = model.adding().clone();
        let comments = if with_comments { model.comments(cx).unwrap_or_default() } else { Vec::new() };
        let shown: HashSet<&str> = order.iter().map(|f| f.path.as_str()).collect();
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
            if !groups.iter().any(|(k, _)| *k == key) {
                groups.push((key, Vec::new()));
            }
        }
        for ((path, side, line), list) in groups {
            let is_draft = draft.as_ref().is_some_and(|d| d.file_path == path && d.side == side && d.line_number == line);
            let id = ElementId::from(SharedString::from(format!("comments-{path}-{}-{line}", side.as_str())));
            let side = match side {
                Side::Original => DiffSide::Original,
                Side::Modified => DiffSide::Modified,
            };
            annotations.push(DiffAnnotation::line(id.clone(), DiffLinePosition::new(path, side, line as usize)));
            items.insert(id, Annotated::Comments { comments: list, draft: is_draft });
        }
        let draft_box = if draft.is_some() { self.draft_box(window, cx) } else {
            self.draft_input = None;
            None
        };
        let items = Rc::new(items);
        let this = cx.weak_entity();
        let model = self.model.clone();
        let state_for_headers = state.clone();
        let draft_label = draft.map(|d| format!("Comment on line {} ({})", d.line_number, d.side.as_str()));
        // Pierre tints changed rows (`--diffs-deletion/addition-color`) on its near-black body.
        let mut decorations = Vec::new();
        for (path, lines) in treeterm_model::review_tree::changed_lines(patch) {
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
            .soft_wrap(true)
            .change_background(false)
            .change_indicator(DiffChangeIndicator::None)
            .hunk_separator(DiffHunkSeparator::LineInfo)
            .bg(colors(cx).bg_primary)
            .border_0()
            .decorations(decorations)
            .on_line_hover({
                // The file being read, for the find bar's "In this file".
                let this = this.clone();
                move |position, _, cx| {
                    if let Some(position) = position {
                        let path = position.path().to_string();
                        match this.update(cx, |view, _| view.hovered_file = Some(path)) {
                            Ok(()) | Err(_) => {} // Err: the tab closed
                        }
                    }
                }
            })
            .annotations(annotations)
            .render_annotation(move |annotation, window, cx| {
                let Some(item) = items.get(annotation.id()) else {
                    return div().into_any_element();
                };
                match item {
                    Annotated::Header(header) => file_header(header, &state_for_headers, &this, &model, window, cx),
                    Annotated::Comments { comments, draft } => {
                        let input = draft_box.clone().filter(|_| *draft);
                        comment_group(comments, input, draft_label.clone(), &adding, &model, window, cx)
                    }
                }
            })
            .into_any_element();
        // `--diffs-font-size: 12px` (the theme's mono size is 13px): the diff is laid out at
        // a rem that makes its code 12px.
        RemScope { rem: px(16. * 12. / 13.), child: diff }.into_any_element()
    }
}

/// Pierre's body and its changed-row tints, as Chromium draws them.
const DIFF_BG: u32 = 0x070707;
const DELETED_ROW: u32 = 0x351b17;
const ADDED_ROW: u32 = 0x192919;

/// Lays its child out and paints it at another rem size (GPUI Kit's `Diff` sizes its code
/// from the theme's mono size in rems).
struct RemScope {
    rem: Pixels,
    child: AnyElement,
}

impl IntoElement for RemScope {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for RemScope {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let child = &mut self.child;
        let layout = window.with_rem_size(Some(self.rem), |window| child.request_layout(window, cx));
        (layout, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.prepaint(window, cx));
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        _prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let child = &mut self.child;
        window.with_rem_size(Some(self.rem), |window| child.paint(window, cx));
    }
}

/// `.file-diff-header`: chevron, status, path, stats, Stage / Unstage, refresh, Viewed.
fn file_header(
    header: &Header,
    state: &Entity<DiffState>,
    view: &WeakEntity<ReviewView>,
    model: &Entity<ReviewModel>,
    window: &mut Window,
    cx: &mut App,
) -> AnyElement {
    let c = colors(cx);
    let (secondary, primary, accent, hover, tertiary, border) =
        (c.text_secondary, c.text_primary, c.accent, c.bg_hover, c.bg_tertiary, c.border);
    let file = &header.file;
    let collapsed = match header.binary {
        Some(closed) => closed,
        None => state.read(cx).is_file_collapsed(&file.path),
    };
    let refreshing = model.read(cx).is_refreshing();
    let staging_busy = model.read(cx).staging() == &GitOp::Running;
    let toggle = {
        let (state, path, view, binary) = (state.clone(), file.path.clone(), view.clone(), header.binary.is_some());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            if binary {
                let path = path.clone();
                match view.update(cx, |view, cx| {
                    if !view.closed_binary.remove(&path) {
                        view.closed_binary.insert(path);
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
    let stage = file.staged.map(|staged| {
        let (model, target) = (model.clone(), StageTarget::File(file.path.clone()));
        outline_button(SharedString::from(format!("header-stage-{}", file.path)), 11., (1., 6.), 3., !staging_busy, cx)
            .child(if staged == Staging::Staged { "Unstage" } else { "Stage" })
            .when(!staging_busy, |d| {
                d.on_click(move |_, _, cx| {
                    cx.stop_propagation();
                    let target = target.clone();
                    model.update(cx, |m, cx| if staged == Staging::Staged { m.unstage(target, cx) } else { m.stage(target, cx) });
                })
            })
    });
    let refresh = {
        let model = model.clone();
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
            .when(refreshing, |d| d.opacity(0.5))
            .when(!refreshing, |d| d.cursor_pointer().hover(move |d| d.bg(hover).text_color(primary)))
            .child(if refreshing { spinner(12., secondary) } else { plain_icon(IconName::RefreshCw, 12.).into_any_element() })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                model.update(cx, |m, cx| m.refresh(cx));
            })
    };
    let viewed = {
        let (model, path, above, view) = (model.clone(), file.path.clone(), header.above.clone(), view.clone());
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
            .child("Viewed")
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                model.update(cx, |m, cx| m.toggle_viewed(&path, cx));
                // The view collapses it when it sees it viewed.
                match view.update(cx, |_, cx| cx.notify()) {
                    Ok(()) | Err(_) => {}
                }
            })
            .when(!above.is_empty(), |d| {
                d.on_mouse_down(MouseButton::Right, move |_, _, cx| {
                    cx.stop_propagation();
                    mark_above.update(cx, |m, cx| m.set_files_viewed(&above, true, cx));
                })
            })
    };
    let row = h_flex()
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
        .child(div().flex_1().min_w_0().truncate().text_color(primary).child(file.path.clone()))
        .child(h_flex().flex_shrink_0().gap(px(6.)).text_size(px(11.)).children(change_stats(file.stats, cx)))
        .children(stage)
        .child(refresh)
        .child(viewed)
        .on_click(toggle);
    // GPUI Kit pads (0.75rem) and borders every annotation, painting the borders over its
    // content: the header fills the padding and the borders are its own. (Pierre's 8px above
    // the code would sit above the bottom border, so it is left out.)
    let pad = window.rem_size() * 0.75;
    v_flex()
        .mx(-pad)
        .my(-pad)
        .child(row)
        .when(header.binary == Some(false), |d| match header.image.clone() {
            Some(sides) => d.child(image_diff(&file.path, sides, cx)),
            // `.file-diff-placeholder`.
            None => d.child(
                div().px(px(12.)).py(px(16.)).bg(c.bg_primary).text_size(px(12.)).italic().text_color(secondary).child("Binary file \u{2014} diff not shown"),
            ),
        })
        .into_any_element()
}

/// `ImageDiffViewer`: before and after side by side on a checkerboard.
fn image_diff(path: &str, sides: ImageSides, cx: &App) -> AnyElement {
    let c = colors(cx);
    let (secondary, bg) = (c.text_secondary, c.bg_primary);
    let format = match path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
        Some("jpg" | "jpeg") => gpui_kit::ImageFormat::Jpeg,
        Some("gif") => gpui_kit::ImageFormat::Gif,
        Some("webp") => gpui_kit::ImageFormat::Webp,
        Some("svg") => gpui_kit::ImageFormat::Svg,
        Some("bmp") => gpui_kit::ImageFormat::Bmp,
        Some("ico") => gpui_kit::ImageFormat::Ico,
        Some(_) | None => gpui_kit::ImageFormat::Png,
    };
    let side = |label: &'static str, bytes: Option<std::sync::Arc<Vec<u8>>>| {
        v_flex()
            .flex_1()
            .min_w_0()
            .items_center()
            .gap(px(6.))
            .child(div().self_start().text_size(px(11.)).text_color(secondary).child(label.to_uppercase()))
            .child(match bytes {
                Some(bytes) => {
                    let image = std::sync::Arc::new(gpui_kit::Image::from_bytes(format, (*bytes).clone()));
                    div()
                        .max_w_full()
                        .bg(rgb(0x2a2a2a))
                        .child(gpui_kit::img(image).max_w_full().max_h(px(480.)).object_fit(gpui_kit::ObjectFit::Contain))
                        .into_any_element()
                }
                None => div().p(px(24.)).text_size(px(12.)).italic().text_color(secondary).child("No file").into_any_element(),
            })
    };
    let body = match sides {
        ImageSides::Loading => h_flex()
            .gap(px(6.))
            .items_center()
            .px(px(12.))
            .py(px(16.))
            .text_size(px(12.))
            .text_color(secondary)
            .child(spinner(16., secondary))
            .child("Loading diff...")
            .into_any_element(),
        ImageSides::Failed(error) => div().p(px(12.)).text_size(px(12.)).text_color(rgb(0xf85149)).child(error).into_any_element(),
        ImageSides::Loaded { before, after } => h_flex()
            .items_start()
            .gap(px(12.))
            .p(px(12.))
            .child(side("Before", before))
            .child(side("After", after))
            .into_any_element(),
    };
    div().bg(bg).child(body).into_any_element()
}

/// `.pierre-diff-annotation`: a line's comments (`CommentDisplay`) and the comment box
/// (`CommentInput`).
fn comment_group(
    comments: &[ReviewComment],
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
    let group = v_flex()
        .mx(-pad)
        .my(-pad)
        .px(px(8.))
        .py(px(4.))
        .bg(colors(cx).bg_secondary)
        .font_family(super::parts::MONO)
        .children(comments.iter().map(|comment| comment_display(comment, &Placement::Diff, &actions, cx)));
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

/// `new Date(iso).toLocaleDateString()` (en-US): `11/11/2026`, local time.
pub fn locale_date(iso: &str) -> String {
    let Some(millis) = parse_iso(iso) else {
        return iso.to_owned();
    };
    match treeterm_core::clock::local(treeterm_core::clock::from_millis(millis)) {
        Some(t) => format!("{}/{}/{}", t.month, t.day, t.year),
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

    #[test]
    fn locale_dates_have_no_padding() {
        let date = locale_date("2026-01-05T12:00:00Z");
        assert!(date.ends_with("/2026"), "{date}");
        assert!(!date.starts_with('0'), "{date}");
        assert_eq!(locale_date("garbage"), "garbage");
    }
}
