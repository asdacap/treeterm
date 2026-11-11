//! The TTYs tab (`TtyListBrowser.tsx`).

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, SharedString, Subscription, Window, div, rgb};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::tty_list::{ProcessList, TtyList, now_millis, referenced_pty_ids, relative_time};

use crate::theme::colors;

pub struct TtyListView {
    model: Entity<TtyList>,
    /// Every tab of the session counts when telling orphans apart.
    session: Entity<SessionStore>,
    _observe: Vec<Subscription>,
}

impl TtyListView {
    pub fn new(model: Entity<TtyList>, session: Entity<SessionStore>, cx: &mut Context<Self>) -> Self {
        let observe = vec![cx.observe(&model, |_, _, cx| cx.notify()), cx.observe(&session, |_, _, cx| cx.notify())];
        Self { model, session, _observe: observe }
    }
}

impl Render for TtyListView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, border, card) = (c.text_secondary, c.error, c.border, c.bg_secondary);
        let referenced = referenced_pty_ids(self.session.read(cx), cx);
        let model = self.model.read(cx);
        let (path, list, rows, kill_error) =
            (model.path().to_owned(), model.list().clone(), model.rows(&referenced), model.kill_error().map(str::to_owned));
        let orphans = rows.iter().filter(|row| row.orphan).count();
        let toolbar = h_flex()
            .gap_2()
            .items_center()
            .child(div().flex_1().truncate().text_base().child(format!("TTY sessions for {path}")))
            .child(
                Button::new("tty-kill-orphans")
                    .small()
                    .with_variant(ButtonVariant::Danger)
                    .disabled(orphans == 0)
                    .label(if orphans > 0 { format!("Kill all orphans ({orphans})") } else { "Kill all orphans".to_owned() })
                    .on_click(cx.listener(move |view, _, _, cx| {
                        let referenced = referenced_pty_ids(view.session.read(cx), cx);
                        view.model.update(cx, |m, cx| m.kill_orphans(&referenced, cx));
                    })),
            )
            .child(
                Button::new("tty-refresh")
                    .small()
                    .outline()
                    .label("Refresh")
                    .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.refresh(cx)))),
            );
        let body = match list {
            ProcessList::Loading => div().text_sm().text_color(secondary).child("Loading TTY sessions…").into_any_element(),
            ProcessList::Failed(message) => v_flex()
                .gap_1()
                .child(div().text_color(error).child("Failed to load TTY sessions"))
                .child(div().text_sm().text_color(error).child(message))
                .into_any_element(),
            ProcessList::Loaded(_) if rows.is_empty() => {
                div().text_sm().text_color(secondary).child("No TTY sessions for this workspace").into_any_element()
            }
            ProcessList::Loaded(_) => {
                let now = now_millis();
                let mut list = v_flex().gap_1();
                for row in rows {
                    let (open_id, kill_id) = (row.info.id.clone(), row.info.id.clone());
                    list = list.child(
                        h_flex()
                            .p_2()
                            .gap_3()
                            .items_center()
                            .rounded_md()
                            .border_1()
                            .border_color(border)
                            .bg(card)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .gap_1()
                                    .child(
                                        h_flex()
                                            .gap_2()
                                            .font_family("Menlo")
                                            .text_sm()
                                            .child(row.info.id.clone())
                                            .when(row.orphan, |d| {
                                                d.child(
                                                    div().px_1().rounded_sm().bg(rgb(0x5a4a1a)).text_xs().child("orphan"),
                                                )
                                            }),
                                    )
                                    .child(
                                        h_flex()
                                            .gap_3()
                                            .text_xs()
                                            .text_color(secondary)
                                            .child(format!("{}×{}", row.info.cols, row.info.rows))
                                            .child(format!("created {}", relative_time(row.info.created_at, now)))
                                            .child(format!("active {}", relative_time(row.info.last_activity, now))),
                                    ),
                            )
                            .child(
                                Button::new(SharedString::from(format!("tty-open-{}", row.info.id)))
                                    .xsmall()
                                    .outline()
                                    .label("Open")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.model.update(cx, |m, cx| m.open(&open_id, cx));
                                    })),
                            )
                            .child(
                                Button::new(SharedString::from(format!("tty-kill-{}", row.info.id)))
                                    .xsmall()
                                    .with_variant(ButtonVariant::Danger)
                                    .label("Kill")
                                    .on_click(cx.listener(move |view, _, _, cx| {
                                        view.model.update(cx, |m, cx| m.kill(&kill_id, cx));
                                    })),
                            ),
                    );
                }
                list.into_any_element()
            }
        };
        v_flex()
            .id("tty-list")
            .size_full()
            .p_3()
            .gap_3()
            .overflow_y_scroll()
            .child(toolbar)
            .when_some(kill_error, |d, text| d.child(div().text_sm().text_color(error).child(text)))
            .child(body)
    }
}
