//! The GitHub tab (`GitHubBrowser.tsx`): the PR the session's git controller loaded.

use gpui_kit::assets::IconName;
use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FontWeight, Hsla, SharedString, Subscription, Window, div, px, rgb, rgba};
use treeterm_model::git_controller::GitController;
use treeterm_model::tool_tabs::github::{Mark, PrPanel, check_mark, panel, review_label, review_mark, state_label, thread_location};

use crate::theme::colors;
use crate::ui::{MONO, action_button, icon, normal_line};

pub struct GitHubView {
    /// `None`: the session has no GitHub client.
    controller: Option<Entity<GitController>>,
    _observe: Option<Subscription>,
}

/// A mark's icon in its `.github-icon-*` colour; a running check spins.
fn mark(mark: Mark, secondary: Hsla) -> AnyElement {
    let (name, color): (IconName, Hsla) = match mark {
        Mark::Approved => (IconName::CircleCheck, rgb(0x238636).into()),
        Mark::Rejected => (IconName::CircleX, rgb(0xb62324).into()),
        Mark::Commented => (IconName::MessageSquare, rgb(0xd29922).into()),
        Mark::Pending => (IconName::Clock, secondary),
        Mark::Dismissed | Mark::Unknown => (IconName::CircleAlert, secondary),
        Mark::Running => {
            return Spinner::new()
                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                .with_size(px(14.))
                .color(secondary)
                .into_any_element();
        }
    };
    icon(name, 14., color).flex_shrink_0().into_any_element()
}

impl GitHubView {
    pub fn new(controller: Option<Entity<GitController>>, cx: &mut Context<Self>) -> Self {
        let observe = controller.as_ref().map(|controller| cx.observe(controller, |_, _, cx| cx.notify()));
        Self { controller, _observe: observe }
    }

    /// `.workspace-action-btn` with the refresh icon (a spinner while refreshing), and the
    /// "Refresh" label in the empty state.
    fn refresh_button(&self, refreshing: bool, label: bool, cx: &mut Context<Self>) -> AnyElement {
        let controller = self.controller.clone();
        let primary = colors(cx).text_primary;
        let glyph = if refreshing {
            Spinner::new()
                .icon(gpui_kit::component::Icon::new(IconName::LoaderCircle))
                .with_size(px(14.))
                .color(primary)
                .into_any_element()
        } else {
            icon(IconName::RefreshCw, 14., primary).into_any_element()
        };
        action_button("github-refresh", cx)
            .when(refreshing, |d| d.opacity(0.5))
            .child(glyph)
            .when(label, |d| d.child(div().ml(px(4.)).child("Refresh")))
            .when(!refreshing, |d| {
                d.on_click(move |_, _, cx| {
                    if let Some(controller) = &controller {
                        controller.update(cx, |c, cx| c.refresh(cx));
                    }
                })
            })
            .into_any_element()
    }
}

/// `.github-section`: a bordered box with its 13px title.
fn section(title: String, border: Hsla) -> gpui_kit::Div {
    v_flex().p(px(12.)).rounded(px(6.)).border_1().border_color(border).child(
        div()
            .mb(px(8.))
            .text_size(px(13.))
            .line_height(normal_line(13.))
            .font_weight(FontWeight::SEMIBOLD)
            .child(title),
    )
}

impl Render for GitHubView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border, hover_bg) = (c.text_secondary, c.text_primary, c.border, c.bg_secondary);
        let root = v_flex().id("github-pr").size_full().overflow_y_scroll().p(px(16.)).text_color(primary);
        let Some(controller) = &self.controller else {
            return root.child(empty(div().child("GitHub is not available in this session"), secondary));
        };
        let (status, refreshing) = {
            let git = controller.read(cx);
            (git.status().clone(), git.is_refreshing())
        };
        let (pr, details) = match panel(&status.pull_request) {
            PrPanel::NoPr => {
                return root.child(empty(
                    v_flex()
                        .gap(px(12.))
                        .items_center()
                        .child(div().child("No open pull request found for this branch."))
                        .child(self.refresh_button(refreshing, true, cx)),
                    secondary,
                ));
            }
            PrPanel::Found { pr, details } => (pr.clone(), details.cloned().map_err(str::to_owned)),
        };
        let url = pr.url.clone();
        let mut header = v_flex().mb(px(16.)).child(
            h_flex()
                .justify_between()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(16.))
                        .line_height(normal_line(16.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(format!("#{} {}", pr.number, pr.title)),
                )
                .child(
                    h_flex().flex_shrink_0().gap(px(4.)).child(self.refresh_button(refreshing, false, cx)).child(
                        action_button("github-open", cx)
                            .child(icon(IconName::ExternalLink, 14., primary))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Open on GitHub").build(window, cx))
                            .on_click(move |_, _, cx| cx.open_url(&url)),
                    ),
                ),
        );
        let details = match details {
            Ok(details) => details,
            Err(reason) => {
                return root.child(header).child(
                    div()
                        .text_size(px(12.))
                        .text_color(secondary)
                        .child(format!("Reviews and checks are unavailable: {reason}")),
                );
            }
        };
        let (badge_fg, badge_bg) = match details.state {
            treeterm_core::github::PrState::Open => (0x238636, 0x23863626),
            treeterm_core::github::PrState::Merged => (0x8250df, 0x8250df26),
            treeterm_core::github::PrState::Closed | treeterm_core::github::PrState::Other(_) => (0xb62324, 0xb6232426),
        };
        header = header.child(
            h_flex().mt(px(6.)).child(
                div()
                    .px(px(8.))
                    .py(px(2.))
                    .rounded(px(12.))
                    .bg(rgba(badge_bg))
                    .text_size(px(11.))
                    .line_height(normal_line(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(badge_fg))
                    .child(state_label(&details.state)),
            ),
        );
        let mut sections = v_flex().gap(px(16.));
        let item = || h_flex().gap(px(6.)).text_size(px(12.)).line_height(normal_line(12.));
        if !details.reviews.is_empty() {
            let mut list = v_flex().gap(px(6.));
            for review in &details.reviews {
                list = list.child(
                    item()
                        .child(mark(review_mark(&review.state), secondary))
                        .child(div().font_weight(FontWeight::MEDIUM).child(review.author.clone()))
                        .child(div().text_color(secondary).child(review_label(&review.state))),
                );
            }
            sections = sections.child(section("Reviews".into(), border).child(list));
        }
        if !details.check_runs.is_empty() {
            let mut list = v_flex().gap(px(6.));
            for check in &details.check_runs {
                list = list.child(
                    item()
                        .child(mark(check_mark(check), secondary))
                        .child(div().min_w_0().truncate().child(check.name.clone())),
                );
            }
            sections = sections.child(section("Checks".into(), border).child(list));
        }
        let threads = &details.unresolved_threads;
        let threads_section = if threads.is_empty() {
            section("Unresolved Comments".into(), border)
                .child(div().text_size(px(12.)).text_color(secondary).child("No unresolved comments"))
        } else {
            let mut list = v_flex().gap(px(6.));
            for (index, thread) in threads.iter().enumerate() {
                let url = pr.url.clone();
                list = list.child(
                    v_flex()
                        .id(SharedString::from(format!("github-thread-{index}")))
                        .p(px(8.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(border)
                        .cursor_pointer()
                        .hover(move |d| d.bg(hover_bg))
                        .child(
                            h_flex()
                                .justify_between()
                                .mb(px(4.))
                                .text_size(px(11.))
                                .line_height(normal_line(11.))
                                .text_color(secondary)
                                .child(div().font_family(MONO).child(thread_location(thread)))
                                .child(div().child(thread.author.clone())),
                        )
                        .child(
                            div()
                                .max_h(px(60.))
                                .overflow_hidden()
                                .text_size(px(12.))
                                .line_height(px(16.8))
                                .child(thread.body.clone()),
                        )
                        .on_click(move |_, _, cx| cx.open_url(&url)),
                );
            }
            section(format!("Unresolved Comments ({})", threads.len()), border).child(list)
        };
        root.child(header).child(sections.child(threads_section))
    }
}

/// `.github-browser-empty`: centred, secondary, 16px.
fn empty(child: impl IntoElement, secondary: Hsla) -> AnyElement {
    v_flex()
        .size_full()
        .items_center()
        .justify_center()
        .text_size(px(16.))
        .line_height(px(19.))
        .text_color(secondary)
        .child(child)
        .into_any_element()
}
