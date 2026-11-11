//! The Review tab. Port of the view side of `ReviewBrowser.tsx` (`@pierre/diffs` → GPUI Kit's
//! virtualized `Diff` with tree-sitter highlighting).

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::diff::{Diff, DiffFile, DiffMode, DiffState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::review::{ReviewBase, ReviewContent, ReviewMode, ReviewModel};

use crate::file_browser_view::language_for;
use crate::theme::colors;

pub struct ReviewView {
    model: Entity<ReviewModel>,
    /// The parsed patch currently shown, keyed by its text.
    diff: Option<(String, Entity<DiffState>)>,
    split: bool,
    _observe: Subscription,
}

impl ReviewView {
    pub fn new(model: Entity<ReviewModel>, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self {
            model,
            diff: None,
            split: false,
            _observe: observe,
        }
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

    fn body(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, error) = (c.text_secondary, c.error);
        let content = self.model.read(cx).content().clone();
        let message = |text: String, color| div().p_4().text_color(color).child(text).into_any_element();
        match content {
            ReviewContent::Loading => message("Loading…".into(), secondary),
            ReviewContent::Failed(text) => message(text, error),
            ReviewContent::Patch(patch) if patch.trim().is_empty() => message("No changes".into(), secondary),
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let body = self.body(cx);
        v_flex().size_full().child(bar).child(div().flex_1().min_h_0().child(body)).min_w(px(0.))
    }
}
