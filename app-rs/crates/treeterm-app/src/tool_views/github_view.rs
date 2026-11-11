//! The GitHub tab (`GitHubBrowser.tsx`): the PR the session's git controller loaded.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, Hsla, SharedString, Subscription, Window, div, px, rgb};
use treeterm_model::git_controller::GitController;
use treeterm_model::tool_tabs::github::{Mark, PrPanel, check_mark, panel, review_label, review_mark, state_label, thread_location};

use super::heading;
use crate::theme::colors;

pub struct GitHubView {
    /// `None`: the session has no GitHub client.
    controller: Option<Entity<GitController>>,
    _observe: Option<Subscription>,
}

/// A mark's glyph and colour (`github-icon-*`).
fn mark(mark: Mark) -> (&'static str, Hsla) {
    match mark {
        Mark::Approved => ("✓", rgb(0x4caf50).into()),
        Mark::Rejected => ("✗", rgb(0xf44336).into()),
        Mark::Commented => ("💬", rgb(0x2196f3).into()),
        Mark::Pending => ("◷", rgb(0xff9800).into()),
        Mark::Dismissed => ("!", rgb(0x9e9e9e).into()),
        Mark::Running => ("◌", rgb(0xff9800).into()),
        Mark::Unknown => ("!", rgb(0xff9800).into()),
    }
}

impl GitHubView {
    pub fn new(controller: Option<Entity<GitController>>, cx: &mut Context<Self>) -> Self {
        let observe = controller.as_ref().map(|controller| cx.observe(controller, |_, _, cx| cx.notify()));
        Self { controller, _observe: observe }
    }

    fn refresh_button(&self, refreshing: bool, label: bool) -> Button {
        let controller = self.controller.clone();
        let button = Button::new("github-refresh").small().ghost().disabled(refreshing);
        let button = match (refreshing, label) {
            (true, _) => button.child(Spinner::new()),
            (false, true) => button.label("⟳ Refresh"),
            (false, false) => button.label("⟳"),
        };
        button.on_click(move |_, _, cx| {
            if let Some(controller) = &controller {
                controller.update(cx, |c, cx| c.refresh(cx));
            }
        })
    }
}

impl Render for GitHubView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, border, card, accent) = (c.text_secondary, c.border, c.bg_secondary, c.accent);
        let Some(controller) = &self.controller else {
            return centered(div().text_color(secondary).child("GitHub is not available in this session"));
        };
        let (status, refreshing) = {
            let git = controller.read(cx);
            (git.status().clone(), git.is_refreshing())
        };
        let (pr, details) = match panel(&status.pull_request) {
            PrPanel::NoPr => {
                return centered(
                    v_flex()
                        .gap_2()
                        .items_center()
                        .child(div().text_color(secondary).child("No open pull request found for this branch."))
                        .child(self.refresh_button(refreshing, true)),
                );
            }
            PrPanel::Found { pr, details } => (pr.clone(), details.cloned().map_err(str::to_owned)),
        };
        let url = pr.url.clone();
        let header = v_flex()
            .gap_2()
            .pb_3()
            .border_b_1()
            .border_color(border)
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(div().flex_1().text_lg().child(format!("#{} {}", pr.number, pr.title)))
                    .child(self.refresh_button(refreshing, false))
                    .child(
                        Button::new("github-open")
                            .small()
                            .ghost()
                            .label("↗ Open on GitHub")
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
            );
        let mut sections = v_flex().gap_4().pt_3();
        let details = match details {
            Ok(details) => details,
            Err(reason) => {
                return v_flex()
                    .id("github-pr")
                    .size_full()
                    .p_4()
                    .overflow_y_scroll()
                    .child(header)
                    .child(div().pt_3().text_sm().text_color(secondary).child(format!("Reviews and checks are unavailable: {reason}")))
                    .into_any_element();
            }
        };
        let header = header.child(h_flex().child(
            div()
                .px_2()
                .rounded_md()
                .bg(match details.state {
                    treeterm_core::github::PrState::Open => rgb(0x238636),
                    treeterm_core::github::PrState::Merged => rgb(0x8957e5),
                    treeterm_core::github::PrState::Closed | treeterm_core::github::PrState::Other(_) => rgb(0xda3633),
                })
                .text_sm()
                .text_color(gpui_kit::white())
                .child(state_label(&details.state)),
        ));
        if !details.reviews.is_empty() {
            let mut list = v_flex().gap_1();
            for review in &details.reviews {
                let (glyph, color) = mark(review_mark(&review.state));
                list = list.child(
                    h_flex()
                        .gap_2()
                        .text_sm()
                        .child(div().text_color(color).child(glyph))
                        .child(div().child(review.author.clone()))
                        .child(div().text_color(secondary).child(review_label(&review.state))),
                );
            }
            sections = sections.child(v_flex().gap_2().child(heading("Reviews", None, secondary)).child(list));
        }
        if !details.check_runs.is_empty() {
            let mut list = v_flex().gap_1();
            for check in &details.check_runs {
                let (glyph, color) = mark(check_mark(check));
                list = list.child(
                    h_flex().gap_2().text_sm().child(div().text_color(color).child(glyph)).child(check.name.clone()),
                );
            }
            sections = sections.child(v_flex().gap_2().child(heading("Checks", None, secondary)).child(list));
        }
        let threads = &details.unresolved_threads;
        let threads_section = if threads.is_empty() {
            v_flex()
                .gap_2()
                .child(heading("Unresolved Comments", None, secondary))
                .child(div().text_sm().text_color(secondary).child("No unresolved comments"))
        } else {
            let mut list = v_flex().gap_2();
            for (index, thread) in threads.iter().enumerate() {
                let url = pr.url.clone();
                list = list.child(
                    v_flex()
                        .id(SharedString::from(format!("github-thread-{index}")))
                        .p_2()
                        .gap_1()
                        .rounded_md()
                        .border_1()
                        .border_color(border)
                        .bg(card)
                        .cursor_pointer()
                        .child(
                            h_flex()
                                .gap_2()
                                .text_xs()
                                .child(div().flex_1().truncate().text_color(accent).child(thread_location(thread)))
                                .child(div().text_color(secondary).child(thread.author.clone())),
                        )
                        .child(div().text_sm().child(thread.body.clone()))
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            }
            v_flex()
                .gap_2()
                .child(heading("Unresolved Comments", Some(format!("({})", threads.len())), secondary))
                .child(list)
        };
        v_flex()
            .id("github-pr")
            .size_full()
            .p_4()
            .overflow_y_scroll()
            .child(header)
            .child(sections.child(threads_section))
            .into_any_element()
    }
}

fn centered(child: impl IntoElement) -> AnyElement {
    div().size_full().flex().items_center().justify_center().p(px(16.)).child(child).into_any_element()
}
