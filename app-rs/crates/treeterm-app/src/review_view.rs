//! The Review tab. Port of the view side of `ReviewBrowser.tsx` (`@pierre/diffs` → GPUI Kit's
//! virtualized `Diff` with tree-sitter highlighting), `CommentsList.tsx` and the prompt
//! buttons.
//!
//! Comments show as annotations under their diff line; the `+` beside a hovered line starts
//! one. Files are marked viewed from the file list or their diff header.

use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{
    Diff, DiffAnnotation, DiffFile, DiffLinePosition, DiffLineRange, DiffMode, DiffSide, DiffState,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, ElementId, Entity, Hsla, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::git::{FileChangeStats, FileChangeStatus, Staging, UncommittedFile};
use treeterm_core::review::{Freshness, Resolution, ReviewComment, Side};
use treeterm_model::review::{
    CommentDraft, Committed, Direction, GitOp, Notice, ReviewBase, ReviewContent, ReviewEvent, ReviewMode, ReviewModel,
    StageTarget, Uncommitted,
};

use crate::file_browser_view::language_for;
use crate::theme::colors;

pub struct ReviewView {
    model: Entity<ReviewModel>,
    /// The parsed patch currently shown, keyed by its text.
    diff: Option<(String, Entity<DiffState>)>,
    split: bool,
    /// The commit message box, made on first render (it needs the window).
    message: Option<(Entity<InputState>, Subscription)>,
    /// A commit succeeded: empty the message box on the next render.
    clear_message: bool,
    /// The box the draft comment is typed in, and the draft it belongs to.
    draft_input: Option<(CommentDraft, Entity<InputState>, Subscription)>,
    /// The comments list beside the diff.
    comments_open: bool,
    /// Which comments the list shows: not yet sent to the harness (`Open`) or sent.
    comment_filter: Resolution,
    _observe: Subscription,
    _events: Subscription,
}

/// Status letter and colour of a changed file (Electron's `getStatusIcon`).
fn status_badge(status: FileChangeStatus) -> (&'static str, Hsla) {
    match status {
        FileChangeStatus::Added => ("A", rgb(0x81b88b).into()),
        FileChangeStatus::Modified => ("M", rgb(0xe2c08d).into()),
        FileChangeStatus::Deleted => ("D", rgb(0xc74e39).into()),
        FileChangeStatus::Renamed => ("R", rgb(0x73c991).into()),
        FileChangeStatus::Untracked => ("U", rgb(0x73c991).into()),
    }
}

fn stats_label(stats: FileChangeStats) -> String {
    match stats {
        FileChangeStats::Text { additions, deletions } => format!("+{additions} −{deletions}"),
        FileChangeStats::Binary { byte_change } => format!("{byte_change:+} B"),
        FileChangeStats::Unknown => String::new(),
    }
}

fn diff_side(side: Side) -> DiffSide {
    match side {
        Side::Original => DiffSide::Original,
        Side::Modified => DiffSide::Modified,
    }
}

fn comment_side(side: DiffSide) -> Side {
    match side {
        DiffSide::Original => Side::Original,
        DiffSide::Modified => Side::Modified,
    }
}

/// The line a new comment goes under: the last line of the range.
fn comment_target(range: &DiffLineRange) -> (Side, u32) {
    let line = if range.side() == range.end_side() { range.start().max(range.end()) } else { range.end() };
    (comment_side(range.end_side()), u32::try_from(line).unwrap_or(u32::MAX))
}

fn comment_position(comment: &ReviewComment) -> DiffLinePosition {
    DiffLinePosition::new(comment.file_path.clone(), diff_side(comment.side), comment.line_number as usize)
}

/// What an annotation under a diff line shows.
enum Annotated {
    Comment(ReviewComment),
    Draft,
}

const DRAFT_ID: &str = "review-comment-draft";

impl ReviewView {
    pub fn new(model: Entity<ReviewModel>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        let events = cx.subscribe(&model, |view, _, event: &ReviewEvent, cx| match event {
            ReviewEvent::Committed => {
                view.clear_message = true;
                cx.notify();
            }
            ReviewEvent::StateChanged(_) => {}
        });
        Self {
            model,
            diff: None,
            split: false,
            message: None,
            clear_message: false,
            draft_input: None,
            comments_open: false,
            comment_filter: Resolution::Open,
            _observe: observe,
            _events: events,
        }
    }

    fn message_input(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<InputState> {
        if let Some((input, _)) = &self.message {
            if self.clear_message {
                self.clear_message = false;
                input.update(cx, |state, cx| state.set_value("", window, cx));
            }
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message"));
        let enter = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.commit(&text, cx));
            }
        });
        self.message = Some((input.clone(), enter));
        self.clear_message = false;
        input
    }

    /// The box for the model's draft comment, made when the draft changes; `None` without one.
    fn draft_input(&mut self, focus: bool, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<InputState>> {
        let Some(draft) = self.model.read(cx).draft().cloned() else {
            self.draft_input = None;
            return None;
        };
        if let Some((shown, input, _)) = &self.draft_input
            && *shown == draft
        {
            return Some(input.clone());
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Leave a comment (Enter to save)"));
        let enter = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let text = input.read(cx).value().to_string();
                view.model.update(cx, |m, cx| m.submit_comment(&text, cx));
            }
        });
        if focus {
            input.update(cx, |input, cx| input.focus(window, cx));
        }
        self.draft_input = Some((draft, input.clone(), enter));
        Some(input)
    }

    /// The `+` beside a line: start a comment there and type into it.
    fn start_comment(&mut self, range: &DiffLineRange, window: &mut Window, cx: &mut Context<Self>) {
        let (side, line) = comment_target(range);
        let path = range.path().to_string();
        self.model.update(cx, |m, cx| m.start_comment(&path, side, line, cx));
        let _focused = self.draft_input(true, window, cx);
    }

    fn scroll_to_file(&self, path: &str, cx: &mut Context<Self>) {
        if let Some((_, state)) = &self.diff {
            state.update(cx, |s, cx| s.scroll_to_file(path, cx));
        }
    }

    fn jump_unviewed(&mut self, direction: Direction, cx: &mut Context<Self>) {
        if let Some(path) = self.model.update(cx, |m, cx| m.jump_unviewed(direction, cx)) {
            self.scroll_to_file(&path, cx);
        }
    }

    /// One file of the Committed list.
    fn committed_row(&self, index: usize, file: &treeterm_core::git::DiffFile, viewed: &HashSet<String>, cx: &mut Context<Self>) -> AnyElement {
        let (letter, color) = status_badge(file.status);
        self.file_row(("committed", index), &file.path, letter, color, file.stats, viewed, None, cx)
    }

    /// One changed file in a list: status, name, stats, the viewed toggle and, for uncommitted
    /// files, staging.
    #[allow(clippy::too_many_arguments)]
    fn file_row(
        &self,
        id: (&'static str, usize),
        path: &str,
        letter: &'static str,
        color: Hsla,
        stats: FileChangeStats,
        viewed: &HashSet<String>,
        staging: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors(cx);
        let is_viewed = viewed.contains(path);
        let focused = self.model.read(cx).focused_file() == Some(path);
        let (toggle_path, scroll_to) = (path.to_owned(), path.to_owned());
        let (dir, name) = match path.rsplit_once('/') {
            Some((dir, name)) => (dir.to_owned(), name.to_owned()),
            None => (String::new(), path.to_owned()),
        };
        let viewed_toggle = Button::new(SharedString::from(format!("viewed-{}-{}", id.0, id.1)))
            .xsmall()
            .ghost()
            .label(if is_viewed { "☑" } else { "☐" })
            .on_click(cx.listener(move |view, _, _, cx| {
                cx.stop_propagation(); // not a click on the row
                view.model.update(cx, |m, cx| m.toggle_viewed(&toggle_path, cx));
            }));
        h_flex()
            .id(SharedString::from(format!("file-{}-{}", id.0, id.1)))
            .px_2()
            .gap_2()
            .items_center()
            .text_sm()
            .cursor_pointer()
            .when(focused, |d| d.bg(c.bg_active))
            .when(!focused, |d| d.hover(move |d| d.bg(c.bg_hover)))
            .child(viewed_toggle)
            .child(div().w(px(12.)).text_xs().text_color(color).child(letter))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .when(is_viewed, |d| d.opacity(0.6))
                    .child(div().flex_none().child(name))
                    .child(div().min_w_0().truncate().text_xs().text_color(c.text_secondary).child(dir)),
            )
            .child(div().text_xs().text_color(c.text_secondary).child(stats_label(stats)))
            .children(staging)
            .on_click(cx.listener(move |view, _, _, cx| {
                view.model.update(cx, |m, cx| m.focus_file(&scroll_to, cx));
                view.scroll_to_file(&scroll_to, cx);
            }))
            .into_any_element()
    }

    /// One changed file in the Staged or Unstaged list.
    fn uncommitted_row(&self, index: usize, file: &UncommittedFile, busy: bool, viewed: &HashSet<String>, cx: &mut Context<Self>) -> AnyElement {
        let (letter, color) = status_badge(file.entry.status);
        let staged = file.entry.staged == Staging::Staged;
        let target = StageTarget::File(file.entry.path.clone());
        let action = Button::new(SharedString::from(format!("stage-{index}")))
            .xsmall()
            .ghost()
            .label(if staged { "Unstage" } else { "Stage" })
            .disabled(busy)
            .on_click(cx.listener(move |view, _, _, cx| {
                let target = target.clone();
                view.model.update(cx, |m, cx| if staged { m.unstage(target, cx) } else { m.stage(target, cx) });
            }))
            .into_any_element();
        self.file_row(("uncommitted", index), &file.entry.path, letter, color, file.stats, viewed, Some(action), cx)
    }

    /// The viewed paths, or the error reading them (shown in the list header).
    fn viewed_set(&self, cx: &App) -> Result<HashSet<String>, String> {
        match self.model.read(cx).viewed_files(cx) {
            Ok(viewed) => Ok(viewed.into_keys().collect()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// The Committed view: the changed files beside the patch.
    fn committed(&mut self, committed: Committed, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, error, border) = {
            let c = colors(cx);
            (c.text_secondary, c.error, c.border)
        };
        if committed.patch.trim().is_empty() {
            return div().p_4().text_color(secondary).child("No changes").into_any_element();
        }
        let (viewed, viewed_error) = match self.viewed_set(cx) {
            Ok(viewed) => (viewed, None),
            Err(message) => (HashSet::new(), Some(message)),
        };
        let mut list = v_flex().id("committed-files").flex_1().min_h_0().overflow_y_scroll().py_1();
        for (index, file) in committed.files.iter().enumerate() {
            list = list.child(self.committed_row(index, file, &viewed, cx));
        }
        let side = v_flex()
            .w(px(300.))
            .h_full()
            .border_r_1()
            .border_color(border)
            .when_some(viewed_error, |d, text| d.child(div().px_2().py_1().text_xs().text_color(error).child(text)))
            .child(list);
        let diff = self.diff_view(&committed.patch, &viewed, window, cx);
        h_flex().size_full().child(side).child(div().flex_1().min_w_0().h_full().child(diff)).into_any_element()
    }

    /// The Uncommitted view: file list with staging and the commit box, the patch beside it.
    fn uncommitted(&mut self, uncommitted: Uncommitted, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let (secondary, error, border) = {
            let c = colors(cx);
            (c.text_secondary, c.error, c.border)
        };
        let (staging, commit) = {
            let model = self.model.read(cx);
            (model.staging().clone(), model.commit_op().clone())
        };
        let busy = staging == GitOp::Running || commit == GitOp::Running;
        if uncommitted.changes.files.is_empty() {
            return div().p_4().text_color(secondary).child("No uncommitted changes").into_any_element();
        }
        let (viewed, viewed_error) = match self.viewed_set(cx) {
            Ok(viewed) => (viewed, None),
            Err(message) => (HashSet::new(), Some(message)),
        };
        let header = h_flex()
            .px_2()
            .py_1()
            .gap_1()
            .items_center()
            .text_xs()
            .border_b_1()
            .border_color(border)
            .child(div().text_color(rgb(0x81b88b)).child(format!("+{}", uncommitted.changes.total_additions)))
            .child(div().text_color(rgb(0xc74e39)).child(format!("−{}", uncommitted.changes.total_deletions)))
            .child(div().flex_1())
            .child(Button::new("prompt-commit").xsmall().ghost().label("Prompt Commit").on_click(
                cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.prompt_commit(cx))),
            ))
            .child(Button::new("stage-all").xsmall().ghost().label("Stage All").disabled(busy).on_click(
                cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.stage(StageTarget::All, cx))),
            ))
            .child(Button::new("unstage-all").xsmall().ghost().label("Unstage All").disabled(busy).on_click(
                cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.unstage(StageTarget::All, cx))),
            ));
        let section = |title: &'static str| {
            div().px_2().pt_2().pb_1().text_xs().text_color(secondary).child(title)
        };
        let mut list = v_flex().id("uncommitted-files").flex_1().min_h_0().overflow_y_scroll();
        for (title, side) in [("Staged", Staging::Staged), ("Unstaged", Staging::Unstaged)] {
            let files: Vec<(usize, &UncommittedFile)> = uncommitted
                .changes
                .files
                .iter()
                .enumerate()
                .filter(|(_, f)| f.entry.staged == side)
                .collect();
            if files.is_empty() {
                continue;
            }
            list = list.child(section(title));
            for (index, file) in files {
                list = list.child(self.uncommitted_row(index, file, busy, &viewed, cx));
            }
        }
        let mut side = v_flex()
            .w(px(340.))
            .h_full()
            .border_r_1()
            .border_color(border)
            .child(header)
            .when_some(match &staging {
                GitOp::Failed(text) => Some(text.clone()),
                GitOp::Idle | GitOp::Running => None,
            }, |d, text| d.child(div().px_2().py_1().text_xs().text_color(error).child(text)))
            .when_some(viewed_error, |d, text| d.child(div().px_2().py_1().text_xs().text_color(error).child(text)))
            .child(list);
        if uncommitted.has_staged() {
            let input = self.message_input(window, cx);
            let message = input.clone();
            side = side.child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_t_1()
                    .border_color(border)
                    .child(Input::new(&input).small())
                    .child(
                        Button::new("commit")
                            .small()
                            .primary()
                            .label(if commit == GitOp::Running { "Committing…" } else { "Commit" })
                            .disabled(busy)
                            .on_click(cx.listener(move |view, _, _, cx| {
                                let text = message.read(cx).value().to_string();
                                view.model.update(cx, |m, cx| m.commit(&text, cx));
                            })),
                    )
                    .when_some(match &commit {
                        GitOp::Failed(text) => Some(text.clone()),
                        GitOp::Idle | GitOp::Running => None,
                    }, |d, text| d.child(div().text_xs().text_color(error).child(text))),
            );
        }
        let diff = self.diff_view(&uncommitted.patch, &viewed, window, cx);
        h_flex()
            .size_full()
            .child(side)
            .child(div().flex_1().min_w_0().h_full().child(diff))
            .into_any_element()
    }

    fn diff_state(&mut self, patch: &str, cx: &mut Context<Self>) -> Result<Entity<DiffState>, String> {
        if let Some((shown, state)) = &self.diff
            && shown == patch
        {
            return Ok(state.clone());
        }
        let files = DiffFile::parse(patch).map_err(|e| e.to_string())?;
        let files: Vec<DiffFile> = files
            .into_iter()
            .map(|file| {
                let language = language_for(file.path());
                file.with_language(language)
            })
            .collect();
        let mode = if self.split { DiffMode::Split } else { DiffMode::Unified };
        let state = cx.new(|cx| DiffState::new(files, cx).with_mode(mode));
        self.diff = Some((patch.to_owned(), state.clone()));
        Ok(state)
    }

    /// The patch, with its comments under their lines, the draft's box, and a viewed toggle
    /// in each file header.
    fn diff_view(&mut self, patch: &str, viewed: &HashSet<String>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let state = match self.diff_state(patch, cx) {
            Ok(state) => state,
            Err(text) => {
                return div().p_4().text_color(colors(cx).error).child(format!("Cannot show this diff: {text}")).into_any_element();
            }
        };
        let c = colors(cx);
        let (secondary, error, border, card) = (c.text_secondary, c.error, c.border, c.bg_secondary);
        // A metadata error is shown by the comments list and the notice; the diff still shows.
        let comments = self.model.read(cx).comments(cx).unwrap_or_default();
        let draft = self.model.read(cx).draft().cloned();
        let adding = self.model.read(cx).adding().clone();
        let draft_input = self.draft_input(false, window, cx);

        let mut annotations: Vec<DiffAnnotation> = Vec::new();
        let mut items: Vec<(ElementId, Annotated)> = Vec::new();
        for comment in comments {
            let id = ElementId::from(SharedString::from(comment.id.clone()));
            annotations.push(DiffAnnotation::line(id.clone(), comment_position(&comment)));
            items.push((id, Annotated::Comment(comment)));
        }
        if let Some(draft) = &draft {
            let id = ElementId::from(SharedString::from(DRAFT_ID));
            let position = DiffLinePosition::new(draft.file_path.clone(), diff_side(draft.side), draft.line_number as usize);
            annotations.push(DiffAnnotation::line(id.clone(), position));
            items.push((id, Annotated::Draft));
        }
        let items = Rc::new(items);
        let model = self.model.clone();
        let render_model = model.clone();
        let header_model = model.clone();
        let viewed = Rc::new(viewed.clone());
        let this = cx.weak_entity();
        Diff::new(&state)
            .line_number(true)
            .syntax_highlight(true)
            .annotations(annotations)
            .render_annotation(move |annotation, _, _| {
                let model = render_model.clone();
                let Some((_, item)) = items.iter().find(|(id, _)| id == annotation.id()) else {
                    return div().into_any_element();
                };
                let frame = v_flex().mx_2().my_1().p_2().gap_1().rounded_md().border_1().border_color(border).bg(card);
                match item {
                    Annotated::Draft => {
                        let Some(input) = &draft_input else {
                            return div().into_any_element();
                        };
                        let (save_model, cancel_model, text) = (model.clone(), model.clone(), input.clone());
                        frame
                            .child(Input::new(input).small())
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        Button::new("comment-save")
                                            .xsmall()
                                            .primary()
                                            .label(if adding == GitOp::Running { "Saving…" } else { "Comment" })
                                            .disabled(adding == GitOp::Running)
                                            .on_click(move |_, _, cx| {
                                                let text = text.read(cx).value().to_string();
                                                save_model.update(cx, |m, cx| m.submit_comment(&text, cx));
                                            }),
                                    )
                                    .child(
                                        Button::new("comment-cancel")
                                            .xsmall()
                                            .ghost()
                                            .label("Cancel")
                                            .on_click(move |_, _, cx| cancel_model.update(cx, |m, cx| m.cancel_comment(cx))),
                                    )
                                    .when_some(
                                        match &adding {
                                            GitOp::Failed(message) => Some(message.clone()),
                                            GitOp::Idle | GitOp::Running => None,
                                        },
                                        |d, message| d.child(div().text_xs().text_color(error).child(message)),
                                    ),
                            )
                            .into_any_element()
                    }
                    Annotated::Comment(comment) => frame
                        .child(comment_actions(comment, &model, secondary))
                        .child(div().text_sm().child(comment.text.clone()))
                        .into_any_element(),
                }
            })
            .on_add_annotation(move |range, window, cx| {
                let range = range.clone();
                match this.update(cx, |view, cx| view.start_comment(&range, window, cx)) {
                    Ok(()) | Err(_) => {} // Err: the tab closed
                }
            })
            .render_header_suffix(move |file, _, _| {
                let path = file.path().to_string();
                let is_viewed = viewed.contains(&path);
                let model = header_model.clone();
                Button::new(SharedString::from(format!("header-viewed-{path}")))
                    .xsmall()
                    .ghost()
                    .label(if is_viewed { "☑ Viewed" } else { "☐ Viewed" })
                    .on_click(move |_, _, cx| model.update(cx, |m, cx| m.toggle_viewed(&path, cx)))
            })
            .into_any_element()
    }

    /// The comments list (`CommentsList.tsx`), filtered to unprompted or prompted comments.
    fn comments_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error, border, card, accent) = (c.text_secondary, c.error, c.border, c.bg_secondary, c.accent);
        let comments = match self.model.read(cx).comments(cx) {
            Ok(comments) => comments,
            Err(message) => {
                return v_flex()
                    .w(px(340.))
                    .h_full()
                    .border_l_1()
                    .border_color(border)
                    .p_2()
                    .child(div().text_xs().text_color(error).child(message.to_string()))
                    .into_any_element();
            }
        };
        let unprompted = comments.iter().filter(|c| c.resolution == Resolution::Open).count();
        let prompted = comments.len() - unprompted;
        let filter = self.comment_filter;
        let tab = |id: &'static str, label: String, target: Resolution, cx: &mut Context<Self>| {
            let button = Button::new(id).xsmall().label(label);
            let button = if filter == target { button.primary() } else { button.ghost() };
            button.on_click(cx.listener(move |view, _, _, cx| {
                view.comment_filter = target;
                cx.notify();
            }))
        };
        let header = h_flex()
            .p_1()
            .gap_1()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(tab("comments-unprompted", format!("Unprompted ({unprompted})"), Resolution::Open, cx))
            .child(tab("comments-prompted", format!("Prompted ({prompted})"), Resolution::Addressed, cx))
            .child(div().flex_1())
            .child(Button::new("comments-close").xsmall().ghost().label("×").on_click(cx.listener(|view, _, _, cx| {
                view.comments_open = false;
                cx.notify();
            })));
        let shown: Vec<&ReviewComment> = comments.iter().filter(|c| c.resolution == filter).collect();
        let mut list = v_flex().id("comments-list").flex_1().min_h_0().overflow_y_scroll().p_2().gap_2();
        if shown.is_empty() {
            let empty = match (comments.is_empty(), filter) {
                (true, _) => "No review comments yet",
                (false, Resolution::Open) => "No unprompted comments",
                (false, Resolution::Addressed) => "No prompted comments",
            };
            list = list.child(div().text_xs().text_color(secondary).child(empty));
        }
        for comment in shown {
            let position = comment_position(comment);
            let go_id = SharedString::from(format!("goto-{}", comment.id));
            list = list.child(
                v_flex()
                    .p_2()
                    .gap_1()
                    .rounded_md()
                    .border_1()
                    .border_color(border)
                    .bg(card)
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .child(div().flex_1().truncate().text_color(accent).child(comment.file_path.clone()))
                            .child(div().text_color(secondary).child(format!("L{} ({})", comment.line_number, comment.side.as_str()))),
                    )
                    .child(div().text_sm().child(comment.text.clone()))
                    .child(
                        h_flex()
                            .child(comment_actions(comment, &self.model, secondary))
                            .child(Button::new(go_id).xsmall().ghost().label("Go to").on_click(cx.listener(
                                move |view, _, _, cx| {
                                    if let Some((_, state)) = &view.diff {
                                        let position = position.clone();
                                        state.update(cx, |s, cx| s.scroll_to_line(position, cx));
                                    }
                                },
                            ))),
                    ),
            );
        }
        v_flex()
            .w(px(340.))
            .h_full()
            .flex_shrink_0()
            .border_l_1()
            .border_color(border)
            .child(header)
            .child(list)
            .when(unprompted > 0, |d| {
                d.child(
                    div().p_2().border_t_1().border_color(border).child(
                        Button::new("address-comments-list")
                            .small()
                            .primary()
                            .w_full()
                            .label(format!("Address {unprompted} comment(s)"))
                            .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.prompt_open_comments(cx)))),
                    ),
                )
            })
            .into_any_element()
    }

    /// `ImageDiffViewer`: the selected image's two sides, over the diff.
    fn image_diff_panel(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        use treeterm_model::review::ImageSides;
        let diff = self.model.read(cx).image_diff()?.clone();
        let c = colors(cx);
        let (secondary, error, border) = (c.text_secondary, c.error, c.border);
        let format_of = |path: &str| match path.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).as_deref() {
            Some("png") => gpui_kit::ImageFormat::Png,
            Some("jpg" | "jpeg") => gpui_kit::ImageFormat::Jpeg,
            Some("gif") => gpui_kit::ImageFormat::Gif,
            Some("webp") => gpui_kit::ImageFormat::Webp,
            Some("svg") => gpui_kit::ImageFormat::Svg,
            Some("bmp") => gpui_kit::ImageFormat::Bmp,
            Some("ico") => gpui_kit::ImageFormat::Ico,
            Some(_) | None => gpui_kit::ImageFormat::Png,
        };
        let format = format_of(&diff.path);
        let side = move |label: &'static str, missing: &'static str, bytes: Option<std::sync::Arc<Vec<u8>>>| {
            v_flex()
                .flex_1()
                .min_w_0()
                .h_full()
                .gap_1()
                .child(div().text_xs().text_color(secondary).child(label))
                .child(match bytes {
                    Some(bytes) => {
                        let image = std::sync::Arc::new(gpui_kit::Image::from_bytes(format, (*bytes).clone()));
                        div()
                            .flex_1()
                            .min_h_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .child(gpui_kit::img(image).max_w_full().max_h_full().object_fit(gpui_kit::ObjectFit::Contain))
                            .into_any_element()
                    }
                    None => div().flex_1().flex().items_center().justify_center().text_sm().text_color(secondary).child(missing).into_any_element(),
                })
        };
        let body = match diff.sides {
            ImageSides::Loading => div().p_3().text_sm().text_color(secondary).child("Loading images…").into_any_element(),
            ImageSides::Failed(message) => div().p_3().text_sm().text_color(error).child(message).into_any_element(),
            ImageSides::Loaded { before, after } => h_flex()
                .size_full()
                .gap_3()
                .p_2()
                .child(side("Before", "(added)", before))
                .child(side("After", "(deleted)", after))
                .into_any_element(),
        };
        let model = self.model.clone();
        Some(
            v_flex()
                .h(px(280.))
                .flex_shrink_0()
                .border_b_1()
                .border_color(border)
                .child(
                    h_flex()
                        .px_3()
                        .py_1()
                        .gap_2()
                        .text_xs()
                        .text_color(secondary)
                        .child(div().flex_1().truncate().child(diff.path.clone()))
                        .child(Button::new("close-image-diff").xsmall().ghost().label("Close").on_click(move |_, _, cx| {
                            model.update(cx, |m, cx| m.close_image_diff(cx));
                        })),
                )
                .child(div().flex_1().min_h_0().child(body))
                .into_any_element(),
        )
    }

    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let images = self.image_diff_panel(cx);
        let content = self.body_content(window, cx);
        match images {
            Some(panel) => v_flex().size_full().child(panel).child(div().flex_1().min_h_0().child(content)).into_any_element(),
            None => content,
        }
    }

    fn body_content(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error) = (c.text_secondary, c.error);
        let content = self.model.read(cx).content().clone();
        let message = |text: String, color| div().p_4().text_color(color).child(text).into_any_element();
        match content {
            ReviewContent::Loading => message("Loading…".into(), secondary),
            ReviewContent::Failed(text) => message(text, error),
            ReviewContent::Committed(committed) => self.committed(committed, window, cx),
            ReviewContent::Uncommitted(uncommitted) => self.uncommitted(uncommitted, window, cx),
            ReviewContent::Commits(commits) if commits.is_empty() => message("No commits on this branch yet".into(), secondary),
            ReviewContent::Commits(commits) => {
                let mut list = v_flex().id("commits").size_full().overflow_y_scroll().p_2().gap_1();
                for commit in commits {
                    let subject = commit.message.lines().next().unwrap_or_default().to_owned();
                    list = list.child(
                        h_flex()
                            .gap_3()
                            .text_sm()
                            .child(div().font_family("Menlo").text_color(c.accent).child(commit.short_hash.clone()))
                            .child(div().flex_1().truncate().child(subject))
                            .child(div().text_xs().text_color(secondary).child(commit.author.clone())),
                    );
                }
                list.into_any_element()
            }
        }
    }
}

/// A comment's state and actions: outdated, prompt / re-prompt, addressed, delete.
fn comment_actions(comment: &ReviewComment, model: &Entity<ReviewModel>, secondary: Hsla) -> impl IntoElement {
    let addressed = comment.resolution == Resolution::Addressed;
    let id = comment.id.clone();
    let (prompt_model, address_model, delete_model) = (model.clone(), model.clone(), model.clone());
    let (prompt_id, address_id, delete_id) = (id.clone(), id.clone(), id.clone());
    h_flex()
        .gap_1()
        .items_center()
        .text_xs()
        .when(comment.freshness == Freshness::Outdated, |d| {
            d.child(div().px_1().rounded_sm().bg(rgb(0x5a4a1a)).child("Outdated"))
        })
        .when(addressed, |d| d.child(div().text_color(secondary).child("Prompted")))
        .child(div().flex_1())
        .child(
            Button::new(SharedString::from(format!("prompt-{id}")))
                .xsmall()
                .ghost()
                .label(if addressed { "Re-prompt" } else { "Prompt" })
                .on_click(move |_, _, cx| prompt_model.update(cx, |m, cx| m.prompt_comment(&prompt_id, cx))),
        )
        .child(
            Button::new(SharedString::from(format!("address-{id}")))
                .xsmall()
                .ghost()
                .label(if addressed { "Reopen" } else { "Mark addressed" })
                .on_click(move |_, _, cx| address_model.update(cx, |m, cx| m.toggle_addressed(&address_id, cx))),
        )
        .child(
            Button::new(SharedString::from(format!("delete-{id}")))
                .xsmall()
                .ghost()
                .label("×")
                .on_click(move |_, _, cx| delete_model.update(cx, |m, cx| m.delete_comment(&delete_id, cx))),
        )
}

impl Render for ReviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (border, secondary, error) = (c.border, c.text_secondary, c.error);
        let (mode, base, notice) = {
            let model = self.model.read(cx);
            (model.mode(), model.base().clone(), model.notice().clone())
        };
        let modes: &[(ReviewMode, &str)] = match base {
            ReviewBase::Branch(_) => &[
                (ReviewMode::Committed, "Committed"),
                (ReviewMode::Uncommitted, "Uncommitted"),
                (ReviewMode::Commits, "Commits"),
            ],
            ReviewBase::None => &[(ReviewMode::Uncommitted, "Uncommitted")],
        };
        let mut bar = h_flex().px_2().py_1().gap_1().items_center().border_b_1().border_color(border);
        for (target, label) in modes {
            let target = *target;
            let button = Button::new(SharedString::from(format!("review-{label}"))).label(*label).xsmall();
            let button = if mode == target { button.primary() } else { button.ghost() };
            bar = bar.child(button.on_click(cx.listener(move |view, _, _, cx| {
                view.model.update(cx, |m, cx| m.set_mode(target, cx));
            })));
        }
        let base_label: SharedString = match &base {
            ReviewBase::Branch(branch) => format!("against {branch}").into(),
            ReviewBase::None => "top-level workspace".into(),
        };
        let viewed = self.model.read(cx).viewed_count(cx);
        let open_comments = self
            .model
            .read(cx)
            .comments(cx)
            .map(|comments| comments.iter().filter(|c| c.resolution == Resolution::Open).count());
        let comment_total = self.model.read(cx).comments(cx).map(|comments| comments.len()).unwrap_or(0);
        bar = bar.child(div().pl_2().text_xs().text_color(secondary).child(base_label)).child(div().flex_1());
        match viewed {
            Ok((_, 0)) => {}
            Ok((done, total)) => {
                bar = bar
                    .child(div().text_xs().text_color(secondary).child(format!("Viewed {done}/{total}")))
                    .child(Button::new("prev-unviewed").xsmall().ghost().label("↑ Unviewed").disabled(done == total).on_click(
                        cx.listener(|view, _, _, cx| view.jump_unviewed(Direction::Previous, cx)),
                    ))
                    .child(Button::new("next-unviewed").xsmall().ghost().label("↓ Unviewed").disabled(done == total).on_click(
                        cx.listener(|view, _, _, cx| view.jump_unviewed(Direction::Next, cx)),
                    ));
            }
            Err(message) => bar = bar.child(div().text_xs().text_color(error).child(message.to_string())),
        }
        if let Ok(open) = open_comments
            && open > 0
        {
            bar = bar.child(Button::new("address-comments").xsmall().primary().label("Address Comments").on_click(
                cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.prompt_open_comments(cx))),
            ));
        }
        bar = bar
            .child(
                Button::new("review-comments")
                    .xsmall()
                    .ghost()
                    .label(format!("Comments ({comment_total})"))
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.comments_open = !view.comments_open;
                        cx.notify();
                    })),
            )
            .child(
                Button::new("review-split")
                    .xsmall()
                    .ghost()
                    .label(if self.split { "Unified" } else { "Split" })
                    .on_click(cx.listener(|view, _, _, cx| {
                        view.split = !view.split;
                        view.diff = None; // rebuilt in the new mode
                        cx.notify();
                    })),
            )
            .child(Button::new("review-refresh").xsmall().ghost().label("Refresh").on_click(cx.listener(
                |view, _, _, cx| view.model.update(cx, |m, cx| m.refresh(cx)),
            )));
        let notice = match notice {
            Notice::Clear => None,
            Notice::Error(text) => Some(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_2()
                    .items_center()
                    .text_xs()
                    .border_b_1()
                    .border_color(border)
                    .child(div().flex_1().text_color(error).child(text))
                    .child(Button::new("dismiss-notice").xsmall().ghost().label("×").on_click(
                        cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.dismiss_notice(cx))),
                    )),
            ),
        };
        let body = self.body(window, cx);
        let comments = if self.comments_open { Some(self.comments_panel(cx)) } else { None };
        v_flex()
            .size_full()
            .child(bar)
            .children(notice)
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().flex_1().min_w_0().h_full().child(body))
                    .children(comments),
            )
            .min_w(px(0.))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_comment_goes_under_the_last_line_of_the_range() {
        let single = DiffLineRange::new("a.rs", DiffSide::Modified, 7, 7);
        assert_eq!(comment_target(&single), (Side::Modified, 7));
        let upwards = DiffLineRange::new("a.rs", DiffSide::Original, 9, 4);
        assert_eq!(comment_target(&upwards), (Side::Original, 9));
        let across = DiffLineRange::new("a.rs", DiffSide::Original, 3, 5).with_end_side(DiffSide::Modified);
        assert_eq!(comment_target(&across), (Side::Modified, 5));
    }

    #[test]
    fn sides_map_both_ways() {
        for side in [Side::Original, Side::Modified] {
            assert_eq!(comment_side(diff_side(side)), side);
        }
    }
}
