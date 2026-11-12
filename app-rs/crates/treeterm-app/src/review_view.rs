//! The Review tab. Port of the view side of `ReviewBrowser.tsx`: the header (branch → base
//! picker, refresh, prompt and merge buttons), the conflict banner, the view tabs, and per
//! view the summary, `DiffToolbar`, the file tree (`DiffFileTree`) beside the stacked diff
//! (`StackedDiffList` / `FileDiffSection`, here GPUI Kit's virtualized `Diff`) and the
//! commit box. Sizes and colours are `index.css`'s.

mod files;
mod parts;
mod stacked;

use std::collections::HashSet;

use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Focusable as _, Context, Entity, EntityId, FontWeight, MouseButton, MouseMoveEvent, ScrollHandle, SharedString,
    Subscription, Window, deferred, div, px, rgb, rgba,
};
use treeterm_core::git::{GitLogCommit, Staging};
use treeterm_model::review::{
    ConflictCheck, FavouriteSelection, GitOp, Loaded, Notice, ReviewBase, ReviewContent, ReviewEvent, ReviewHeader,
    ReviewMode, ReviewModel, StageTarget,
};
use treeterm_model::session_store::AfterMerge;
use treeterm_model::workspace_store::{MergeOutput, MergeState, WorkspaceStore};

use crate::theme::colors;
use parts::{ERROR_RED, MONO, action_button, branch_pill, plain_icon, spinner, toolbar_button, totals};
use stacked::{DiffCache, ListedFile};

/// The advance of 12px Menlo (0.6em).
const MONO_12_ADVANCE: f32 = 7.224;

pub struct ReviewView {
    model: Entity<ReviewModel>,
    workspace: Entity<WorkspaceStore>,
    /// The selected favourite's viewer, and the model it shows.
    favourite_view: Option<(EntityId, Entity<crate::file_browser_view::FileBrowserView>)>,
    /// The stacked diff of what is shown, rebuilt when the patch or the filter changes.
    diff: Option<DiffCache>,
    /// `isSplitView` (Electron starts split).
    split: bool,
    /// `hideUnchangedRegions`.
    hide_unchanged: bool,
    /// The file list's width (`fileListWidth`, 150–500), dragged by the divider.
    list_width: f32,
    /// The divider is being dragged: the x where the content row starts.
    resizing: Option<f32>,
    /// "Filter to this folder" (`dirFilter`).
    dir_filter: Option<String>,
    /// The "Filter files..." box, made on first render (it needs the window).
    search: Option<(Entity<InputState>, Subscription)>,
    query: String,
    /// Directories folded in the file tree (all start open).
    closed_dirs: HashSet<String>,
    /// Binary files folded in the diff (their `DiffState` entry stays collapsed).
    closed_binary: HashSet<String>,
    /// The last "Collapse all" / "Expand all" (`collapseAll.collapsed`).
    all_collapsed: bool,
    /// The commit message box.
    message: Option<(Entity<InputState>, Subscription)>,
    message_text: String,
    /// A commit succeeded: empty the message box on the next render.
    clear_message: bool,
    draft_input: Option<stacked::DraftInput>,
    /// The base branch picker, when open: its filter box.
    base_picker: Option<(Entity<InputState>, Subscription)>,
    base_query: String,
    /// The merge button's menu is open.
    merge_menu: bool,
    commits_scroll: ScrollHandle,
    /// The merge output, kept scrolled to its end (`AutoScrollPre`), and how much of it was
    /// shown when it last scrolled.
    merge_scroll: ScrollHandle,
    merge_len: usize,
    _observe: Subscription,
    _events: Subscription,
}

impl ReviewView {
    pub fn new(model: Entity<ReviewModel>, workspace: Entity<WorkspaceStore>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        let events = cx.subscribe(&model, |view, _, event: &ReviewEvent, cx| match event {
            ReviewEvent::Committed => {
                view.clear_message = true;
                cx.notify();
            }
            ReviewEvent::StateChanged(_)
            | ReviewEvent::StateRemoved(_)
            | ReviewEvent::CloseRequested
            | ReviewEvent::MergeRequested { .. }
            | ReviewEvent::RemoveRequested(_) => {}
        });
        Self {
            model,
            workspace,
            favourite_view: None,
            diff: None,
            split: true,
            hide_unchanged: true,
            list_width: 250.,
            resizing: None,
            dir_filter: None,
            search: None,
            query: String::new(),
            closed_dirs: HashSet::new(),
            closed_binary: HashSet::new(),
            all_collapsed: false,
            message: None,
            message_text: String::new(),
            clear_message: false,
            draft_input: None,
            base_picker: None,
            base_query: String::new(),
            merge_menu: false,
            commits_scroll: ScrollHandle::new(),
            merge_scroll: ScrollHandle::new(),
            merge_len: 0,
            _observe: observe,
            _events: events,
        }
    }

    // ---- header ------------------------------------------------------------------------

    /// `.review-header`: name, `branch → base`, then the actions.
    fn header(&mut self, header: &ReviewHeader, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let selector = self.base_selector(header, window, cx);
        let actions = self.header_actions(header, cx);
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        let info = h_flex()
            .gap(px(12.))
            .items_center()
            .min_w_0()
            .child(
                div()
                    .flex_shrink_0()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(primary)
                    .child(header.name.clone()),
            )
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_center()
                    .text_size(px(12.))
                    .line_height(px(14.))
                    .child(branch_pill(cx).border_1().border_color(gpui_kit::transparent_black()).child(header.branch.clone()))
                    .child(div().w(px(11.)).flex_shrink_0().text_color(secondary).child("→"))
                    .child(selector),
            );
        h_flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .px(px(20.))
            .py(px(12.))
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .child(info)
            .child(actions)
            .into_any_element()
    }

    /// `BaseBranchSelector`: the base as a pill with a chevron; the dropdown filters local
    /// and remote branches, takes any ref typed, and resets an override.
    fn base_selector(&mut self, header: &ReviewHeader, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, accent, border, tertiary) = (c.text_secondary, c.accent, c.border, c.bg_tertiary);
        let (base, overridden) = {
            let model = self.model.read(cx);
            (model.base().clone(), model.is_base_overridden(cx))
        };
        let current = match &base {
            ReviewBase::Branch(branch) => Some(branch.clone()),
            ReviewBase::None => None,
        };
        let label: SharedString = match &current {
            Some(base) if base.chars().count() > 20 => format!("{}…", base.chars().take(12).collect::<String>()).into(),
            Some(base) => base.clone().into(),
            None => "Pick base branch".into(),
        };
        let empty = current.is_none();
        let button = branch_pill(cx)
            .id("review-base-selector")
            .flex()
            .items_center()
            .gap(px(4.))
            .border_1()
            .border_color(gpui_kit::transparent_black())
            .cursor_pointer()
            .when(overridden, |d| d.border_color(accent).italic())
            .when(empty, |d| d.text_color(secondary).bg(tertiary).border_color(border).italic())
            .hover(|d| d.bg(rgba(0x0078d440)))
            .child(label)
            .child(plain_icon(IconName::ChevronDown, 12.))
            .on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.toggle_base_picker(window, cx)));
        let mut selector = div().relative().child(button);
        if self.base_picker.is_some() {
            // `top: 100%; left: 0; margin-top: 4px`, painted over the page.
            let dropdown = self.base_dropdown(header, current, overridden, window, cx);
            selector = selector.child(deferred(div().absolute().top_full().left_0().child(dropdown)).with_priority(1));
        }
        selector.into_any_element()
    }

    fn toggle_base_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.base_picker.take().is_some() {
            cx.notify();
            return;
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter branches or paste a commit..."));
        let changed = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| {
            if let InputEvent::Change = event {
                view.base_query = input.read(cx).value().to_string();
                cx.notify();
            }
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        self.base_picker = Some((input, changed));
        self.base_query.clear();
        if !matches!(self.model.read(cx).branches(), Loaded::Ready(_)) {
            self.model.update(cx, |m, cx| m.load_branches(cx));
        }
        cx.notify();
    }

    fn choose_base(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        self.base_picker = None;
        self.base_query.clear();
        self.diff = None;
        self.model.update(cx, |m, cx| m.set_base(branch, cx));
    }

    /// `.base-branch-dropdown`.
    fn base_dropdown(
        &mut self,
        header: &ReviewHeader,
        current: Option<String>,
        overridden: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, accent, border, hover, bg) =
            (c.text_secondary, c.text_primary, c.accent, c.border, c.bg_hover, c.bg_secondary);
        let Some((input, _)) = &self.base_picker else {
            return div().into_any_element();
        };
        let search = div()
            .p(px(6.))
            .border_b_1()
            .border_color(border)
            .child(
                parts::framed_input(input, 24., (6., 6.), 12., 3., border, cx)
                    // No `:focus` rule: Chromium's focus ring.
                    .when(input.read(cx).focus_handle(cx).is_focused(window), |d| parts::focus_ring(d, 3.)),
            );
        let item = |id: SharedString| {
            div()
                .id(id)
                .flex_shrink_0()
                .w_full()
                .px(px(10.))
                .py(px(6.))
                .font_family(MONO)
                .text_size(px(12.))
                .text_color(primary)
                .truncate()
                .cursor_pointer()
                .hover(move |d| d.bg(hover))
        };
        let message = |text: String, color| div().p(px(12.)).text_size(px(12.)).text_color(color).flex().justify_center().child(text);
        let query = self.base_query.trim().to_owned();
        let body = match self.model.read(cx).branches().clone() {
            Loaded::Loading => message("Loading branches...".into(), secondary).into_any_element(),
            Loaded::Failed(error) => message(error, rgb(ERROR_RED).into()).into_any_element(),
            Loaded::Ready(branches) => {
                let needle = query.to_lowercase();
                let filtered: Vec<&String> =
                    branches.iter().filter(|b| needle.is_empty() || b.to_lowercase().contains(&needle)).collect();
                let mut list = v_flex().id("base-branch-list").flex_1().min_h_0().overflow_y_scroll();
                if let (true, Some(default)) = (overridden, header.parent_branch.clone()) {
                    list = list.child(
                        item("base-reset".into())
                            .border_b_1()
                            .border_color(border)
                            .text_color(secondary)
                            .italic()
                            .child(format!("Reset to default ({default})"))
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.choose_base(None, cx))),
                    );
                }
                if !query.is_empty() && !branches.contains(&query) {
                    let custom = query.clone();
                    list = list.child(
                        item("base-custom".into())
                            .border_b_1()
                            .border_color(border)
                            .text_color(secondary)
                            .flex()
                            .child("Compare against commit/ref: ")
                            .child(div().text_color(accent).child(query.clone()))
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_base(Some(custom.clone()), cx))),
                    );
                }
                if filtered.is_empty() {
                    list = list.child(message(if query.is_empty() { "No branches found" } else { "No branches match" }.into(), secondary));
                }
                for (index, branch) in filtered.into_iter().enumerate() {
                    let selected = current.as_ref() == Some(branch);
                    let chosen = branch.clone();
                    list = list.child(
                        item(SharedString::from(format!("base-{index}")))
                            .when(selected, |d| d.text_color(accent).font_weight(FontWeight::SEMIBOLD))
                            .child(branch.clone())
                            .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| view.choose_base(Some(chosen.clone()), cx))),
                    );
                }
                list.into_any_element()
            }
        };
        // As wide as its longest branch (12px Menlo, 10px padding each side, a 10px
        // scrollbar), 240–360px. Its border names an unset variable, so it has none.
        let longest = match self.model.read(cx).branches() {
            Loaded::Ready(branches) => branches.iter().map(|b| b.chars().count()).max().unwrap_or(0),
            Loaded::Loading | Loaded::Failed(_) => 0,
        };
        let width = (longest as f32 * MONO_12_ADVANCE + 30.).clamp(240., 360.);
        v_flex()
            .id("base-branch-dropdown")
            .occlude()
            .mt(px(4.))
            .w(px(width))
            .max_h(px(320.))
            .bg(bg)
            .rounded(px(4.))
            .shadow_lg()
            .text_size(px(12.))
            .line_height(gpui_kit::relative(1.2))
            .child(search)
            .child(body)
            .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                view.base_picker = None;
                cx.notify();
            }))
            .into_any_element()
    }

    /// `.review-header-actions`.
    fn header_actions(&mut self, header: &ReviewHeader, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, tertiary, hover, primary, accent) = (c.text_secondary, c.bg_tertiary, c.bg_hover, c.text_primary, c.accent);
        let (overview, conflicts, refreshing, merge) = {
            let model = self.model.read(cx);
            (model.overview(), model.conflicts().clone(), model.is_refreshing(), model.merge_output(cx))
        };
        // `isProcessing`: the actions wait for a running merge.
        let merging = merge.as_ref().filter(|m| m.state == MergeState::Running);
        let idle = merging.is_none();
        let has_uncommitted = overview.uncommitted > 0;
        let white = gpui_kit::white();
        let mut actions = h_flex().gap(px(8.)).items_center().flex_shrink_0();
        if conflicts == ConflictCheck::Checking {
            actions = actions.child(div().text_size(px(12.)).italic().text_color(secondary).child("Checking for conflicts..."));
        }
        actions = actions.child(
            div()
                .id("review-refresh")
                .flex()
                .items_center()
                .px(px(6.))
                .py(px(4.))
                .bg(tertiary)
                .text_color(primary)
                .border_1()
                .border_color(c.border)
                .rounded(px(4.))
                .when(refreshing, |d| d.opacity(0.5))
                .when(!refreshing, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
                .child(if refreshing { spinner(14., primary) } else { plain_icon(IconName::RefreshCw, 14.).into_any_element() })
                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.refresh(cx)))),
        );
        let prompt_commit = (has_uncommitted && header.harness).then(|| {
            action_button("review-prompt-commit", rgb(0x6366f1).into(), rgb(0x7c7ff7).into(), white, idle)
                .child("Prompt Commit")
                .when(idle, |d| d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.prompt_commit(cx)))))
        });
        let cancel = |label: &'static str, cx: &mut Context<Self>| {
            action_button("review-cancel", tertiary, hover, primary, idle)
                .child(label)
                .when(idle, |d| d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.close(cx)))))
        };
        match (&header.parent_branch, header.detached) {
            (None, _) => {
                actions = actions.children(prompt_commit).child(cancel("Close", cx));
            }
            (Some(_), true) => {
                actions = actions
                    .children(prompt_commit)
                    .child(
                        action_button("review-close-and-clean", rgb(0x0066cc).into(), rgb(0x0077ee).into(), white, idle)
                            .child("Close and Clean")
                            .when(idle, |d| {
                                d.on_click(cx.listener(|view, _: &ClickEvent, window, cx| view.confirm_close_and_clean(window, cx)))
                            }),
                    )
                    .child(cancel("Cancel", cx));
            }
            (Some(parent), false) => {
                let (has_conflicts, parent_moved) = match &conflicts {
                    ConflictCheck::Checked { conflicts, parent_has_new_commits } => (conflicts.has_conflicts, *parent_has_new_commits),
                    ConflictCheck::Idle | ConflictCheck::Checking | ConflictCheck::Failed(_) => (false, false),
                };
                let violet = (rgb(0x8b5cf6).into(), rgb(0xa78bfa).into());
                actions = actions
                    .children(prompt_commit)
                    .when(has_conflicts && header.harness, |d| {
                        d.child(
                            action_button("review-prompt-rebase", violet.0, violet.1, white, idle)
                                .child("Prompt Rebase")
                                .when(idle, |d| {
                                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.prompt_rebase(cx))))
                                }),
                        )
                    })
                    .when(parent_moved && header.harness, |d| {
                        d.child(
                            action_button("review-prompt-update", violet.0, violet.1, white, idle)
                                .tooltip({
                                    let tip = SharedString::from(format!("Ask the AI harness to merge {parent} into this branch"));
                                    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
                                })
                                .child("Prompt Update from Parent")
                                .when(idle, |d| {
                                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                        view.model.update(cx, |m, cx| m.prompt_update_from_parent(cx))
                                    }))
                                }),
                        )
                    })
                    .child(self.merge_group(has_conflicts, has_uncommitted, merging.cloned(), accent, cx))
                    .child(cancel("Cancel", cx));
            }
        }
        actions.into_any_element()
    }

    /// `.merge-btn-group`: "Merge" and a chevron opening the other merges.
    /// `merging`: the merge running, whose button reads "Merging..." (`processingAction`).
    fn merge_group(
        &mut self,
        has_conflicts: bool,
        has_uncommitted: bool,
        merging: Option<MergeOutput>,
        accent: gpui_kit::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let c = colors(cx);
        let (hover, primary, bg) = (c.bg_hover, c.text_primary, c.bg_secondary);
        let enabled = !has_conflicts && merging.is_none();
        let white = gpui_kit::white();
        let blue_hover: gpui_kit::Hsla = rgb(0x006cbf).into();
        // Only the plain merge is on the button; the others are in the closed menu.
        let plain_merge = merging.is_some_and(|m| !m.squash && m.after == AfterMerge::Remove);
        let main = action_button("review-merge", accent, blue_hover, white, enabled)
            .rounded_r(px(0.))
            // `.btn-spinner`: 12px, 6px before the label.
            .when(plain_merge, |d| d.child(div().mr(px(6.)).child(spinner(12., white))))
            .child(format!("{}{}", if plain_merge { "Merging..." } else { "Merge" }, if has_conflicts { " (has conflicts)" } else { "" }))
            .when(enabled, |d| {
                d.on_click(cx.listener(move |view, _: &ClickEvent, window, cx| view.start_merge(false, AfterMerge::Remove, has_uncommitted, window, cx)))
            });
        let toggle = action_button("review-merge-more", accent, blue_hover, white, enabled)
            .px(px(6.))
            .rounded_l(px(0.))
            .border_l_1()
            .border_color(rgba(0xffffff4d))
            .child(plain_icon(IconName::ChevronDown, 14.))
            .when(enabled, |d| {
                d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                    view.merge_menu = !view.merge_menu;
                    cx.notify();
                }))
            });
        let mut group = h_flex().relative().child(main).child(toggle);
        if self.merge_menu {
            let suffix = if has_conflicts { " (has conflicts)" } else { "" };
            let item = |id: &'static str, label: &'static str, squash: bool, after: AfterMerge, cx: &mut Context<Self>| {
                div()
                    .id(id)
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .text_size(px(13.))
                    .text_color(primary)
                    .whitespace_nowrap()
                    .cursor_pointer()
                    .hover(move |d| d.bg(hover))
                    .child(format!("{label}{suffix}"))
                    .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                        view.merge_menu = false;
                        view.start_merge(squash, after, has_uncommitted, window, cx);
                    }))
            };
            let menu = v_flex()
                .id("merge-dropdown")
                .occlude()
                .mt(px(4.))
                .min_w(px(160.))
                .bg(bg)
                // `border: 1px solid var(--border)` names an unset variable: no border.
                .rounded(px(4.))
                .shadow_lg()
                .line_height(gpui_kit::relative(1.2))
                .child(item("merge-squash", "Squash Merge", true, AfterMerge::Remove, cx))
                .child(item("merge-keep", "Merge and Keep", false, AfterMerge::Keep, cx))
                .child(item("merge-squash-keep", "Squash Merge and Keep", true, AfterMerge::Keep, cx))
                .on_mouse_down_out(cx.listener(|view, _, _, cx| {
                    view.merge_menu = false;
                    cx.notify();
                }));
            // `top: 100%; right: 0; margin-top: 4px`, painted over the page.
            group = group.child(deferred(div().absolute().top_full().right_0().child(menu)).with_priority(1));
        }
        group.into_any_element()
    }

    /// `handleMerge` / `handleMergeAndKeep`: a merge that removes the workspace first asks
    /// whether its uncommitted files may be committed.
    fn start_merge(&mut self, squash: bool, after: AfterMerge, has_uncommitted: bool, window: &mut Window, cx: &mut Context<Self>) {
        let count = self.model.read(cx).overview().uncommitted;
        if !(has_uncommitted && after == AfterMerge::Remove) {
            self.model.update(cx, |m, cx| m.merge(squash, after, cx));
            return;
        }
        let model = self.model.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let model = model.clone();
            alert
                .title("Uncommitted changes")
                .description(format!(
                    "You have {count} uncommitted file{}. These changes will be auto-committed before merging. Continue?",
                    if count == 1 { "" } else { "s" }
                ))
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    model.update(cx, |m, cx| m.merge(squash, after, cx));
                    true
                })
        });
    }

    fn confirm_close_and_clean(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let model = self.model.clone();
        window.open_alert_dialog(cx, move |alert, _, _| {
            let model = model.clone();
            alert
                .title("Close and Clean")
                .description("Close this workspace? The worktree will be removed but the branch will be kept.")
                .show_cancel(true)
                .on_ok(move |_, _, cx| {
                    model.update(cx, |m, cx| m.close_and_clean(cx));
                    true
                })
        });
    }

    // ---- banners and tabs --------------------------------------------------------------

    /// `.review-load-error`, `.review-conflict-error`, `.review-conflict-banner`.
    fn banners(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        let mut banners = Vec::new();
        if let Notice::Error(text) = self.model.read(cx).notice().clone() {
            banners.push(
                h_flex()
                    .text_size(px(13.))
                    .text_color(primary)
                    .child(div().flex_1().child(text))
                    .child(
                        div()
                            .id("dismiss-notice")
                            .px(px(6.))
                            .cursor_pointer()
                            .text_color(secondary)
                            .child("×")
                            .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.dismiss_notice(cx)))),
                    )
                    .into_any_element(),
            );
        }
        match self.model.read(cx).conflicts().clone() {
            ConflictCheck::Failed(error) => banners.push(
                div().text_size(px(13.)).text_color(primary).child(format!("Could not check for conflicts: {error}")).into_any_element(),
            ),
            ConflictCheck::Checked { conflicts, .. } if conflicts.has_conflicts => {
                let files = &conflicts.conflicted_files;
                let mut listed = files.iter().take(3).cloned().collect::<Vec<_>>().join(", ");
                if files.len() > 3 {
                    listed.push_str(&format!(" and {} more", files.len() - 3));
                }
                banners.push(
                    h_flex()
                        .items_start()
                        .gap(px(12.))
                        .px(px(20.))
                        .py(px(12.))
                        .bg(rgba(0xf0a0001a))
                        .border_b_1()
                        .border_color(rgba(0xf0a0004d))
                        // Chromium draws "⚠️" in its outline text form, 19×24.
                        .child(
                            div()
                                .w(px(19.))
                                .h(px(24.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(crate::ui::icon(IconName::TriangleAlert, 16., primary)),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .text_size(px(13.))
                                .child(
                                    div()
                                        .mb(px(4.))
                                        .font_weight(FontWeight::BOLD)
                                        .text_color(rgb(0xf0a000))
                                        .child(format!("{} conflict(s) detected", files.len())),
                                )
                                .child(div().text_size(px(12.)).font_family(MONO).text_color(secondary).child(listed)),
                        )
                        .into_any_element(),
                );
            }
            ConflictCheck::Idle | ConflictCheck::Checking | ConflictCheck::Checked { .. } => {}
        }
        banners
    }

    /// `.review-merge-output-panel`: the merge's title and git's output, under the banners.
    /// Closable once the merge has finished.
    fn merge_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let output = self.model.read(cx).merge_output(cx)?;
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        if output.output.len() != self.merge_len {
            self.merge_len = output.output.len();
            self.merge_scroll.scroll_to_bottom();
        }
        let icon = match output.state {
            MergeState::Running => spinner(14., primary),
            MergeState::Succeeded => crate::ui::icon(IconName::Check, 14., rgb(0x4ec9b0)).into_any_element(),
            MergeState::Failed => crate::ui::icon(IconName::X, 14., rgb(ERROR_RED)).into_any_element(),
        };
        let header = h_flex()
            .items_center()
            .gap(px(8.))
            .px(px(20.))
            .py(px(6.))
            .font_family(MONO)
            .text_size(px(12.))
            .text_color(primary)
            .bg(c.bg_secondary)
            .child(icon)
            .child(div().flex_1().min_w_0().truncate().child(output.title()))
            .when(output.state != MergeState::Running, |d| {
                d.child(
                    div()
                        .id("review-merge-output-close")
                        .flex()
                        .cursor_pointer()
                        .text_color(secondary)
                        .hover(move |d| d.text_color(primary))
                        .child(plain_icon(IconName::X, 14.))
                        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Hide merge output").build(window, cx))
                        .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.dismiss_merge_output(cx)))),
                )
            });
        // A `<pre>` does not show its last newline.
        let text = output.output.strip_suffix('\n').unwrap_or(&output.output).to_owned();
        let body = div()
            .id("review-merge-output")
            .track_scroll(&self.merge_scroll)
            .min_h(px(48.))
            .max_h(px(200.))
            .overflow_y_scroll()
            .px(px(20.))
            .py(px(8.))
            .font_family(MONO)
            .text_size(px(12.))
            .text_color(primary)
            .child(text);
        Some(
            v_flex()
                .flex_shrink_0()
                .border_b_1()
                .border_color(c.border)
                .bg(c.bg_primary)
                .child(header)
                .child(body)
                .into_any_element(),
        )
    }

    /// `.diff-tabs`: Committed Changes / Uncommitted / Commits, each with its count.
    fn view_tabs(&self, mode: ReviewMode, has_base: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, hover, accent, active_bg) = (c.text_secondary, c.text_primary, c.bg_hover, c.accent, c.bg_active);
        let overview = self.model.read(cx).overview();
        let tab = |target: ReviewMode, label: &'static str, count: Option<String>, cx: &mut Context<Self>| {
            let active = mode == target;
            div()
                .id(SharedString::from(format!("review-tab-{}", target.as_str())))
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(16.))
                .pt(px(10.))
                .pb(px(10.))
                .mb(px(-1.))
                .border_b_2()
                .border_color(if active { accent } else { gpui_kit::transparent_black() })
                .text_size(px(13.))
                .text_color(if active { primary } else { secondary })
                .cursor_pointer()
                .hover(move |d| d.text_color(primary).bg(hover))
                .child(label)
                .when_some(count, |d, count| {
                    d.child(
                        div()
                            .min_w(px(18.))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(10.))
                            .bg(active_bg)
                            .text_size(px(11.))
                            .flex()
                            .justify_center()
                            .child(count),
                    )
                })
                .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.model.update(cx, |m, cx| m.set_mode(target, cx));
                }))
        };
        let count = |n: usize| (n > 0).then(|| n.to_string());
        h_flex()
            .flex_shrink_0()
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .when(has_base, |d| d.child(tab(ReviewMode::Committed, "Committed Changes", count(overview.committed), cx)))
            .child(tab(ReviewMode::Uncommitted, "Uncommitted", count(overview.uncommitted), cx))
            .child(tab(
                ReviewMode::Commits,
                "Commits",
                (overview.commits > 0).then(|| format!("{}{}", overview.commits, if overview.more_commits { "+" } else { "" })),
                cx,
            ))
            .into_any_element()
    }

    // ---- shared pieces -----------------------------------------------------------------

    /// `.diff-summary`: a row of pills and totals.
    fn summary(&self, cx: &App) -> gpui_kit::Div {
        let c = colors(cx);
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .px(px(20.))
            .py(px(12.))
            .bg(c.bg_primary)
            .border_b_1()
            .border_color(c.border)
            .text_size(px(13.))
            // Chromium's `normal` for 13px Menlo.
            .line_height(px(15.))
    }

    /// `DiffToolbar`.
    fn toolbar(&self, viewed: (usize, usize), comments: usize, navigate: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, accent, hover) = (c.text_secondary, c.text_primary, c.accent, c.bg_hover);
        let (done, total) = viewed;
        let unviewed = done < total;
        let progress = h_flex()
            .gap(px(8.))
            .items_center()
            .min_w_0()
            .when(total > 0, |d| {
                d.child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .px(px(6.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(secondary)
                        .child(plain_icon(IconName::CircleCheck, 14.))
                        .child(format!("{done}/{total} files viewed")),
                )
            })
            .when_some(self.dir_filter.clone(), |d, dir| {
                d.child(
                    h_flex()
                        .gap(px(4.))
                        .items_center()
                        .max_w(px(320.))
                        .pl(px(8.))
                        .pr(px(4.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(primary)
                        .bg(c.bg_primary)
                        .border_1()
                        .border_color(accent)
                        .rounded(px(10.))
                        .child(plain_icon(IconName::Funnel, 12.))
                        .child(div().truncate().child(dir))
                        .child(
                            div()
                                .id("clear-dir-filter")
                                .p(px(1.))
                                .rounded_full()
                                .text_color(secondary)
                                .cursor_pointer()
                                .hover(move |d| d.bg(hover).text_color(primary))
                                .child(plain_icon(IconName::X, 12.))
                                .on_click(cx.listener(|view, _: &ClickEvent, _, cx| {
                                    view.dir_filter = None;
                                    cx.notify();
                                })),
                        ),
                )
            });
        let mut controls = h_flex().gap(px(4.)).items_center();
        if navigate {
            controls = controls
                .child(toolbar_button("prev-unviewed", IconName::ChevronUp, false, unviewed, cx).when(unviewed, |d| {
                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.jump_unviewed(treeterm_model::review::Direction::Previous, cx)))
                }))
                .child(toolbar_button("next-unviewed", IconName::ChevronDown, false, unviewed, cx).when(unviewed, |d| {
                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.jump_unviewed(treeterm_model::review::Direction::Next, cx)))
                }));
        }
        let ignore = self.model.read(cx).ignore_whitespace();
        controls = controls
            .child(
                toolbar_button("toggle-split", if self.split { IconName::Columns2 } else { IconName::TextAlignJustify }, self.split, true, cx)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_split(cx))),
            )
            .child(
                toolbar_button("toggle-unchanged", if self.hide_unchanged { IconName::EyeOff } else { IconName::Eye }, self.hide_unchanged, true, cx)
                    .on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.toggle_unchanged(cx))),
            )
            .child(
                toolbar_button("toggle-whitespace", IconName::Space, ignore, true, cx).on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                    view.diff = None;
                    view.model.update(cx, |m, cx| m.set_ignore_whitespace(!ignore, cx));
                })),
            )
            .when(comments > 0, |d| {
                d.child(
                    h_flex()
                        .gap(px(3.))
                        .items_center()
                        .px(px(6.))
                        .py(px(2.))
                        .text_size(px(11.))
                        .text_color(secondary)
                        .child(plain_icon(IconName::MessageSquare, 14.))
                        .child(comments.to_string()),
                )
            });
        h_flex()
            .flex_shrink_0()
            .items_center()
            .justify_between()
            .px(px(8.))
            .py(px(4.))
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .child(progress)
            .child(controls)
            .into_any_element()
    }

    fn toggle_split(&mut self, cx: &mut Context<Self>) {
        self.split = !self.split;
        let mode = if self.split { gpui_kit::component::diff::DiffMode::Split } else { gpui_kit::component::diff::DiffMode::Unified };
        if let Some(diff) = &self.diff {
            diff.state.update(cx, |s, cx| s.set_mode(mode, cx));
        }
        cx.notify();
    }

    fn toggle_unchanged(&mut self, cx: &mut Context<Self>) {
        self.hide_unchanged = !self.hide_unchanged;
        let hide = self.hide_unchanged;
        if let Some(diff) = &self.diff {
            diff.state.update(cx, |s, cx| if hide { s.collapse_unchanged(cx) } else { s.expand_unchanged(cx) });
        }
        cx.notify();
    }

    fn jump_unviewed(&mut self, direction: treeterm_model::review::Direction, cx: &mut Context<Self>) {
        let Some(diff) = &self.diff else {
            return;
        };
        // `navigateUnviewed` walks the files in the diff's (tree, filtered) order.
        let order = diff.paths();
        let viewed = self.model.read(cx).viewed_files(cx).unwrap_or_default();
        let current = self.model.read(cx).focused_file().map(str::to_owned);
        let target = treeterm_model::review::unviewed_from(&order, &viewed, current.as_deref(), direction).map(str::to_owned);
        if let Some(path) = target {
            self.select_file(&path, cx);
        }
    }

    /// A click in the file list (or a jump): the diff scrolls to the file, expanded.
    fn select_file(&mut self, path: &str, cx: &mut Context<Self>) {
        self.model.update(cx, |m, cx| {
            m.show_diff(cx);
            m.focus_file(path, cx);
        });
        if let Some(diff) = &self.diff {
            let path = path.to_owned();
            diff.state.update(cx, |s, cx| s.scroll_to_file(&path, cx));
        }
        cx.notify();
    }

    /// `.review-loading` / `.review-error` / `.diff-empty`.
    fn message(text: impl Into<SharedString>, color: gpui_kit::Hsla, pad_y: f32) -> AnyElement {
        div()
            .w_full()
            .flex()
            .justify_center()
            .px(px(20.))
            .py(px(pad_y))
            .text_size(px(13.))
            .text_color(color)
            .child(text.into())
            .into_any_element()
    }

    /// `.diff-empty` inside `.diff-content` (a flex row): sized to its text, so it sits at the
    /// left of the diff area; 20px padding, the body's 16px text, secondary colour.
    fn empty(text: &'static str, color: gpui_kit::Hsla) -> AnyElement {
        div().flex_shrink_0().p(px(20.)).text_size(px(16.)).text_color(color).child(text).into_any_element()
    }

    /// Show the selected favourite's PDF / model webview only while this tab is shown.
    pub fn set_shown(&mut self, shown: bool, cx: &mut Context<Self>) {
        let selected = self.model.read(cx).favourite_selection().relative_path().is_some();
        if let Some((_, view)) = &self.favourite_view {
            view.update(cx, |v, cx| v.set_shown(shown && selected, cx));
        }
    }

    /// `renderRightPane`: the selected favourite's file viewer in the diff's place.
    fn favourite_pane(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let FavouriteSelection::Selected { viewer, .. } = self.model.read(cx).favourite_selection().clone() else {
            self.favourite_view = None;
            return None;
        };
        let view = match &self.favourite_view {
            Some((shown, view)) if *shown == viewer.entity_id() => view.clone(),
            Some(_) | None => {
                let workspace = self.workspace.clone();
                let id = viewer.entity_id();
                let view = cx.new(|cx| crate::file_browser_view::FileBrowserView::embedded(viewer, workspace, cx));
                self.favourite_view = Some((id, view.clone()));
                view
            }
        };
        // `.review-favourite-viewer`.
        Some(div().flex().flex_1().min_w_0().h_full().child(view).into_any_element())
    }

    /// `.diff-content`: the file list, the divider and the diff.
    fn content_row(&mut self, list: AnyElement, right: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (border, accent) = (c.border, c.accent);
        let dragging = self.resizing.is_some();
        h_flex()
            .id("review-diff-content")
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(
                v_flex()
                    .id("review-file-list")
                    .w(px(self.list_width))
                    .flex_shrink_0()
                    .h_full()
                    .overflow_y_scroll()
                    .border_r_1()
                    .border_color(border)
                    .child(list),
            )
            .child(
                div()
                    .id("review-divider")
                    .w(px(4.))
                    .h_full()
                    .flex_shrink_0()
                    .bg(if dragging { accent } else { border })
                    .cursor_col_resize()
                    .hover(move |d| d.bg(accent))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|view, event: &gpui_kit::MouseDownEvent, _, cx| {
                            view.resizing = Some(f32::from(event.position.x) - view.list_width - 1.);
                            cx.notify();
                        }),
                    ),
            )
            .child(div().flex_1().min_w_0().h_full().child(right))
            .on_mouse_move(cx.listener(|view, event: &MouseMoveEvent, _, cx| {
                if let Some(start) = view.resizing {
                    if event.pressed_button != Some(MouseButton::Left) {
                        view.resizing = None;
                    } else {
                        view.list_width = (f32::from(event.position.x) - start).clamp(150., 500.);
                    }
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| {
                    if view.resizing.take().is_some() {
                        cx.notify();
                    }
                }),
            )
            .into_any_element()
    }

    /// The "Filter files..." box (`.file-tree-search`).
    fn search_box(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input = match &self.search {
            Some((input, _)) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Filter files..."));
                let changed = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| {
                    if let InputEvent::Change = event {
                        view.query = input.read(cx).value().to_string();
                        cx.notify();
                    }
                });
                self.search = Some((input.clone(), changed));
                input
            }
        };
        let c = colors(cx);
        let (secondary, primary, hover) = (c.text_secondary, c.text_primary, c.bg_hover);
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let clear = input.clone();
        div()
            .relative()
            .p(px(8.))
            .border_b_1()
            .border_color(c.border)
            .child(
                // `:focus { border-color: var(--accent-color) }` names an unset variable, so the
                // border takes the text colour.
                parts::framed_input(&input, 30., (10., 28.), 13., 4., if focused { c.text_primary } else { c.border }, cx),
            )
            .when(!self.query.is_empty(), |d| {
                d.child(
                    div()
                        .id("clear-file-filter")
                        .absolute()
                        .right(px(12.))
                        .top(px(14.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(3.))
                        .text_size(px(12.))
                        .text_color(secondary)
                        .cursor_pointer()
                        .hover(move |d| d.bg(hover).text_color(primary))
                        .child("\u{2715}")
                        .on_click(cx.listener(move |view, _: &ClickEvent, window, cx| {
                            clear.update(cx, |input, cx| input.set_value("", window, cx));
                            view.query.clear();
                            cx.notify();
                        })),
                )
            })
            .into_any_element()
    }

    // ---- the three views ---------------------------------------------------------------

    fn committed(&mut self, committed: treeterm_model::review::Committed, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let secondary = c.text_secondary;
        let has_changes = !committed.files.is_empty();
        let listed: Vec<ListedFile> = committed.files.iter().map(ListedFile::committed).collect();
        let shown = self.filtered(&listed);
        let viewed = self.viewed(cx);
        let comments = self.model.read(cx).comments(cx).map(|c| c.len()).unwrap_or(0);
        let mut column = v_flex().size_full();
        if has_changes {
            let shown_viewed = listed.iter().filter(|f| viewed.contains(&f.path)).count();
            column = column
                .child(
                    self.summary(cx)
                        .child(branch_pill(cx).child(committed.base_branch.clone()))
                        .child(div().text_color(secondary).child("..."))
                        .child(branch_pill(cx).child(committed.head_branch.clone()))
                        .child(
                            h_flex()
                                .ml_auto()
                                .gap(px(8.))
                                .font_family(MONO)
                                .children(totals(committed.total_additions, committed.total_deletions)),
                        ),
                )
                .child(self.toolbar((shown_viewed, listed.len()), comments, true, cx));
        }
        let search = self.search_box(window, cx);
        let tree = files::tree(self, "committed", &shown, &viewed, None, false, cx);
        let favourites = files::favourites(self, &listed, cx);
        let list = v_flex().child(search).children(favourites).when(has_changes, |d| d.child(tree)).into_any_element();
        let right = match self.favourite_pane(cx) {
            Some(pane) => pane,
            None if has_changes => self.stacked(&committed.patch, &shown, &viewed, true, window, cx),
            None => Self::empty("No committed changes to show", secondary),
        };
        column.child(self.content_row(list, right, cx)).into_any_element()
    }

    fn uncommitted(&mut self, uncommitted: treeterm_model::review::Uncommitted, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let secondary = c.text_secondary;
        let (staging, commit) = {
            let model = self.model.read(cx);
            (model.staging().clone(), model.commit_op().clone())
        };
        let busy = staging == GitOp::Running;
        let changes = &uncommitted.changes;
        let has_changes = !changes.files.is_empty();
        let listed: Vec<ListedFile> = changes.files.iter().map(ListedFile::uncommitted).collect();
        let viewed = self.viewed(cx);
        let comments = self.model.read(cx).comments(cx).map(|c| c.len()).unwrap_or(0);
        let mut column = v_flex().size_full();
        if has_changes {
            let shown_viewed = listed.iter().filter(|f| viewed.contains(&f.path)).count();
            let label = if busy { "Processing..." } else { "Stage All" };
            let unlabel = if busy { "Processing..." } else { "Unstage All" };
            column = column
                .child(
                    self.summary(cx)
                        .child(branch_pill(cx).child("Working directory"))
                        .child(
                            h_flex()
                                .ml_auto()
                                .gap(px(8.))
                                .font_family(MONO)
                                .children(totals(changes.total_additions, changes.total_deletions)),
                        )
                        .child(
                            h_flex()
                                .ml_auto()
                                .gap(px(8.))
                                .child(squeezed_button("stage-all", label, !busy, cx).when(!busy, |d| {
                                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.stage(StageTarget::All, cx))))
                                }))
                                .child(squeezed_button("unstage-all", unlabel, !busy, cx).when(!busy, |d| {
                                    d.on_click(cx.listener(|view, _: &ClickEvent, _, cx| view.model.update(cx, |m, cx| m.unstage(StageTarget::All, cx))))
                                })),
                        ),
                )
                .when_some(
                    match &staging {
                        GitOp::Failed(text) => Some(text.clone()),
                        GitOp::Idle | GitOp::Running => None,
                    },
                    |d, text| d.child(div().text_size(px(13.)).child(text)),
                )
                .child(self.toolbar((shown_viewed, listed.len()), comments, true, cx));
        }
        let search = self.search_box(window, cx);
        let mut list = v_flex().child(search).children(files::favourites(self, &listed, cx));
        let mut shown_all: Vec<ListedFile> = Vec::new();
        for (title, side) in [("Staged", Staging::Staged), ("Unstaged", Staging::Unstaged)] {
            let section: Vec<ListedFile> = listed.iter().filter(|f| f.staged == Some(side)).cloned().collect();
            let shown = self.filtered(&section);
            if shown.is_empty() {
                continue;
            }
            list = list.child(files::section_title(title, cx)).child(files::tree(self, title, &shown, &viewed, Some(busy), true, cx));
            shown_all.extend(shown);
        }
        let right = match self.favourite_pane(cx) {
            Some(pane) => pane,
            None if has_changes => self.stacked(&uncommitted.patch, &shown_all, &viewed, true, window, cx),
            None => Self::empty("No uncommitted changes", secondary),
        };
        column = column.child(self.content_row(list.into_any_element(), right, cx));
        if uncommitted.has_staged() {
            column = column.child(self.commit_box(&commit, window, cx));
        }
        column.into_any_element()
    }

    /// `.review-commit-section`.
    fn commit_box(&mut self, commit: &GitOp, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let input = match &self.message {
            Some((input, _)) => input.clone(),
            None => {
                let input = cx.new(|cx| InputState::new(window, cx).placeholder("Commit message..."));
                let sub = cx.subscribe_in(&input, window, |view, input, event: &InputEvent, _, cx| match event {
                    InputEvent::PressEnter { shift: false, .. } => {
                        let text = input.read(cx).value().to_string();
                        view.model.update(cx, |m, cx| m.commit(&text, cx));
                    }
                    InputEvent::Change => {
                        view.message_text = input.read(cx).value().to_string();
                        cx.notify();
                    }
                    InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
                });
                self.message = Some((input.clone(), sub));
                input
            }
        };
        if self.clear_message {
            self.clear_message = false;
            self.message_text.clear();
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        let c = colors(cx);
        let (secondary, active) = (c.text_secondary, c.bg_active);
        let committing = *commit == GitOp::Running;
        let enabled = !committing && !self.message_text.trim().is_empty();
        let focused = input.read(cx).focus_handle(cx).is_focused(window);
        let message = input.clone();
        h_flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(8.))
            .px(px(16.))
            .py(px(12.))
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .child(
                parts::framed_input(&input, 34., (12., 12.), 13., 4., if focused { c.accent } else { c.border }, cx).flex_1(),
            )
            .child(
                div()
                    .id("review-commit")
                    .flex_shrink_0()
                    .px(px(16.))
                    .py(px(8.))
                    .rounded(px(4.))
                    .text_size(px(13.))
                    .line_height(px(16.))
                    .whitespace_nowrap()
                    .when(enabled, |d| {
                        d.bg(rgb(0x238636)).text_color(gpui_kit::white()).cursor_pointer().hover(|d| d.bg(rgb(0x2ea043)))
                    })
                    .when(!enabled, |d| d.bg(active).text_color(secondary))
                    .child(if committing { "Committing..." } else { "Commit" })
                    .when(enabled, |d| {
                        d.on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            let text = message.read(cx).value().to_string();
                            view.model.update(cx, |m, cx| m.commit(&text, cx));
                        }))
                    }),
            )
            .when_some(
                match commit {
                    GitOp::Failed(text) => Some(text.clone()),
                    GitOp::Idle | GitOp::Running => None,
                },
                |d, text| d.child(div().text_size(px(12.)).text_color(rgb(parts::DELETED)).child(text)),
            )
            .into_any_element()
    }

    fn commits(&mut self, commits: Vec<GitLogCommit>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, accent, hover, active, border) =
            (c.text_secondary, c.text_primary, c.accent, c.bg_hover, c.bg_active, c.border);
        let (selected, loading_more, more) = {
            let model = self.model.read(cx);
            (model.selected_commit().cloned(), model.loading_more_commits(), model.overview().more_commits)
        };
        let pane = if commits.is_empty() {
            Self::empty("No commits on this branch", secondary)
        } else {
            let mut list = v_flex().id("commits-list").track_scroll(&self.commits_scroll).max_h(px(160.)).overflow_y_scroll();
            for commit in &commits {
                let is_selected = selected.as_ref().is_some_and(|s| s.commit.hash == commit.hash);
                let hash = commit.hash.clone();
                list = list.child(
                    h_flex()
                        .id(SharedString::from(format!("commit-{}", commit.hash)))
                        .items_center()
                        .gap(px(12.))
                        .px(px(12.))
                        .py(px(4.))
                        .text_size(px(12.))
                        .line_height(px(14.))
                        .border_b_1()
                        .border_color(rgba(0xffffff0d))
                        .cursor_pointer()
                        .when(is_selected, |d| d.bg(active))
                        .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                        .child(div().flex_shrink_0().font_family(MONO).text_size(px(11.)).text_color(accent).child(commit.short_hash.clone()))
                        .child(div().flex_1().min_w_0().truncate().text_color(primary).child(commit.message.lines().next().unwrap_or_default().to_owned()))
                        .child(div().flex_shrink_0().text_size(px(11.)).text_color(secondary).child(commit.author.clone()))
                        .child(div().flex_shrink_0().text_size(px(11.)).text_color(secondary).child(stacked::locale_date(&commit.date)))
                        .on_click(cx.listener(move |view, _: &ClickEvent, _, cx| {
                            view.diff = None;
                            view.model.update(cx, |m, cx| m.select_commit(&hash, cx));
                        })),
                );
            }
            if more {
                list = list.child(
                    div()
                        .min_h(px(12.))
                        .px(px(12.))
                        .py(px(6.))
                        .flex()
                        .justify_center()
                        .text_size(px(12.))
                        .text_color(secondary)
                        .child(if loading_more { "Loading more commits..." } else { "" }),
                );
                // The sentinel: within 200px of the end, the next page loads.
                let handle = &self.commits_scroll;
                let (offset, max) = (handle.offset().y, handle.max_offset().y);
                if !loading_more && f32::from(max) - f32::from(-offset) < 200. {
                    let model = self.model.clone();
                    cx.defer(move |cx| model.update(cx, |m, cx| m.load_more_commits(cx)));
                }
            }
            list.on_scroll(cx.listener(|_, _, _, cx| cx.notify())).into_any_element()
        };
        let mut column = v_flex()
            .size_full()
            .child(div().flex_shrink_0().border_b_1().border_color(border).child(pane));
        if let Some(selected) = selected {
            let viewed = self.viewed(cx);
            let (listed, body): (Vec<ListedFile>, Option<String>) = match &selected.diff {
                Loaded::Ready(diff) => (diff.files.iter().map(ListedFile::committed).collect(), Some(diff.patch.clone())),
                Loaded::Loading | Loaded::Failed(_) => (Vec::new(), None),
            };
            let shown = self.filtered(&listed);
            let shown_viewed = listed.iter().filter(|f| viewed.contains(&f.path)).count();
            column = column.child(self.toolbar((shown_viewed, listed.len()), 0, false, cx));
            let search = self.search_box(window, cx);
            let tree = match &selected.diff {
                Loaded::Loading => div()
                    .w_full()
                    .flex()
                    .justify_center()
                    .p(px(20.))
                    .text_size(px(13.))
                    .text_color(secondary)
                    .child("Loading...")
                    .into_any_element(),
                Loaded::Failed(error) => Self::message(error.clone(), rgb(ERROR_RED).into(), 20.),
                Loaded::Ready(_) => files::tree(self, "commit", &shown, &viewed, None, false, cx),
            };
            let list = v_flex().child(search).children(files::favourites(self, &listed, cx)).child(tree).into_any_element();
            let right = match (self.favourite_pane(cx), body) {
                (Some(pane), _) => pane,
                (None, Some(patch)) => self.stacked(&patch, &shown, &viewed, false, window, cx),
                (None, None) => div().into_any_element(),
            };
            column = column.child(self.content_row(list, right, cx));
        }
        column.into_any_element()
    }

    /// The viewed paths (a metadata read error shows as the tab's notice).
    fn viewed(&self, cx: &App) -> HashSet<String> {
        self.model.read(cx).viewed_files(cx).map(|v| v.into_keys().collect()).unwrap_or_default()
    }

    /// The files the filters keep, in their list order.
    fn filtered(&self, files: &[ListedFile]) -> Vec<ListedFile> {
        files
            .iter()
            .filter(|f| treeterm_model::review_tree::file_matches(&f.path, self.dir_filter.as_deref(), &self.query))
            .cloned()
            .collect()
    }

    fn body(&mut self, mode: ReviewMode, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let secondary = colors(cx).text_secondary;
        let content = self.model.read(cx).content().clone();
        match content {
            ReviewContent::Loading => Self::message(
                match mode {
                    ReviewMode::Commits => "Loading commits...",
                    ReviewMode::Committed | ReviewMode::Uncommitted => "Loading changes...",
                },
                secondary,
                40.,
            ),
            ReviewContent::Failed(text) => Self::message(text, rgb(ERROR_RED).into(), 40.),
            ReviewContent::Committed(committed) => self.committed(committed, window, cx),
            ReviewContent::Uncommitted(uncommitted) => self.uncommitted(uncommitted, window, cx),
            ReviewContent::Commits(commits) => self.commits(commits, window, cx),
        }
    }
}

/// `.diff-action-btn` as Electron draws it: two rules share the class, so the button keeps
/// the first rule's 20×20 box (with the second's padding and border, 26×20) and its bold
/// label overflows it, one word per line, centred.
fn squeezed_button(id: &'static str, label: &'static str, enabled: bool, cx: &App) -> gpui_kit::Stateful<gpui_kit::Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    div()
        .id(id)
        .w(px(26.))
        .h(px(20.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(c.border)
        .rounded(px(4.))
        .text_size(px(11.))
        .line_height(px(13.))
        .font_weight(FontWeight::BOLD)
        .text_color(c.text_secondary)
        .when(!enabled, |d| d.cursor_default())
        .when(enabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover).text_color(primary)))
        .child(
            v_flex()
                .flex_shrink_0()
                .items_center()
                .whitespace_nowrap()
                .children(label.split(' ').map(|word| div().child(word))),
        )
}

impl Render for ReviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (mode, base, header) = {
            let model = self.model.read(cx);
            (model.mode(), model.base().clone(), model.header(cx))
        };
        let has_base = base != ReviewBase::None;
        let header_el = self.header(&header, window, cx);
        let banners = self.banners(cx);
        let merge = self.merge_panel(cx);
        let tabs = self.view_tabs(mode, has_base, cx);
        let body = self.body(mode, window, cx);
        v_flex()
            .size_full()
            .min_w(px(0.))
            .overflow_hidden()
            .bg(colors(cx).bg_primary)
            .child(header_el)
            .children(banners)
            .children(merge)
            .child(tabs)
            .child(div().flex_1().min_h_0().child(body))
    }
}
