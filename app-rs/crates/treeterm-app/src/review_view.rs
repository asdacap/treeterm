//! The Review tab. Port of the view side of `ReviewBrowser.tsx` (`@pierre/diffs` → GPUI Kit's
//! virtualized `Diff` with tree-sitter highlighting).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, Hsla, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::git::{FileChangeStats, FileChangeStatus, Staging, UncommittedFile};
use treeterm_model::review::{GitOp, ReviewBase, ReviewContent, ReviewEvent, ReviewMode, ReviewModel, StageTarget, Uncommitted};

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

    /// One changed file in the Staged or Unstaged list.
    fn file_row(&self, index: usize, file: &UncommittedFile, busy: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (letter, color) = status_badge(file.entry.status);
        let path = file.entry.path.clone();
        let staged = file.entry.staged == Staging::Staged;
        let target = StageTarget::File(path.clone());
        let action = Button::new(SharedString::from(format!("stage-{index}")))
            .xsmall()
            .ghost()
            .label(if staged { "Unstage" } else { "Stage" })
            .disabled(busy)
            .on_click(cx.listener(move |view, _, _, cx| {
                let target = target.clone();
                view.model.update(cx, |m, cx| if staged { m.unstage(target, cx) } else { m.stage(target, cx) });
            }));
        let scroll_to = path.clone();
        h_flex()
            .id(SharedString::from(format!("uc-{index}")))
            .px_2()
            .gap_2()
            .items_center()
            .text_sm()
            .cursor_pointer()
            .hover(move |d| d.bg(c.bg_hover))
            .child(div().w(px(12.)).text_xs().text_color(color).child(letter))
            .child({
                let (dir, name) = match path.rsplit_once('/') {
                    Some((dir, name)) => (dir.to_owned(), name.to_owned()),
                    None => (String::new(), path.clone()),
                };
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_2()
                    .child(div().flex_none().child(name))
                    .child(div().min_w_0().truncate().text_xs().text_color(c.text_secondary).child(dir))
            })
            .child(div().text_xs().text_color(c.text_secondary).child(stats_label(file.stats)))
            .child(action)
            .on_click(cx.listener(move |view, _, _, cx| {
                if let Some((_, state)) = &view.diff {
                    state.update(cx, |s, cx| s.scroll_to_file(&scroll_to, cx));
                }
            }))
            .into_any_element()
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
                list = list.child(self.file_row(index, file, busy, cx));
            }
        }
        let mut side = v_flex()
            .w(px(320.))
            .h_full()
            .border_r_1()
            .border_color(border)
            .child(header)
            .when_some(match &staging {
                GitOp::Failed(text) => Some(text.clone()),
                GitOp::Idle | GitOp::Running => None,
            }, |d, text| d.child(div().px_2().py_1().text_xs().text_color(error).child(text)))
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
        let diff = match self.diff_state(&uncommitted.patch, cx) {
            Ok(state) => Diff::new(&state).line_number(true).syntax_highlight(true).into_any_element(),
            Err(text) => div().p_4().text_color(error).child(format!("Cannot show this diff: {text}")).into_any_element(),
        };
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

    fn body(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error) = (c.text_secondary, c.error);
        let content = self.model.read(cx).content().clone();
        let message = |text: String, color| div().p_4().text_color(color).child(text).into_any_element();
        match content {
            ReviewContent::Loading => message("Loading…".into(), secondary),
            ReviewContent::Failed(text) => message(text, error),
            ReviewContent::Patch(patch) if patch.trim().is_empty() => message("No changes".into(), secondary),
            ReviewContent::Uncommitted(uncommitted) => self.uncommitted(uncommitted, window, cx),
            ReviewContent::Patch(patch) => match self.diff_state(&patch, cx) {
                Ok(state) => Diff::new(&state).line_number(true).syntax_highlight(true).into_any_element(),
                Err(text) => message(format!("Cannot show this diff: {text}"), error),
            },
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

impl Render for ReviewView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let border = c.border;
        let secondary = c.text_secondary;
        let (mode, base) = {
            let model = self.model.read(cx);
            (model.mode(), model.base().clone())
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
        bar = bar
            .child(div().pl_2().text_xs().text_color(secondary).child(base_label))
            .child(div().flex_1())
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
        let body = self.body(window, cx);
        v_flex().size_full().child(bar).child(div().flex_1().min_h_0().child(body)).min_w(px(0.))
    }
}
