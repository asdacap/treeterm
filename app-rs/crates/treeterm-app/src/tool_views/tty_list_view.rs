//! The TTYs tab (`TtyListBrowser.tsx`).

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, rgb};
use treeterm_model::session_store::SessionStore;
use treeterm_model::tool_tabs::tty_list::{ProcessList, TtyList, now_millis, referenced_pty_ids, relative_time};

use crate::theme::colors;
use crate::ui::{MONO, normal_line, text_button};

/// `--color-danger` / `--color-warning` are not defined: their fallbacks.
const DANGER: u32 = 0xe74c3c;
const WARNING: u32 = 0xd08770;

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
        let (secondary, primary, border, active) = (c.text_secondary, c.text_primary, c.border, c.bg_active);
        let referenced = referenced_pty_ids(self.session.read(cx), cx);
        let model = self.model.read(cx);
        let (path, list, rows, kill_error) =
            (model.path().to_owned(), model.list().clone(), model.rows(&referenced), model.kill_error().map(str::to_owned));
        let orphans = rows.iter().filter(|row| row.orphan).count();
        // `.tty-list-refresh-btn` / `.tty-list-open-btn`.
        let button = |id: SharedString, label: &'static str| text_button(id, label, 12., (4., 10.), primary, active, border, 4.);
        let kill_orphans = text_button(
            "tty-kill-orphans",
            if orphans > 0 { format!("Kill all orphans ({orphans})") } else { "Kill all orphans".to_owned() },
            12.,
            (4., 10.),
            if orphans > 0 { rgb(WARNING).into() } else { secondary },
            active,
            if orphans > 0 { rgb(WARNING).into() } else { border },
            4.,
        )
        .when(orphans == 0, |d| d.opacity(0.4).cursor_default())
        .when(orphans > 0, |d| {
            d.on_click(cx.listener(move |view, _, _, cx| {
                let referenced = referenced_pty_ids(view.session.read(cx), cx);
                view.model.update(cx, |m, cx| m.kill_orphans(&referenced, cx));
            }))
        });
        let toolbar = h_flex()
            .flex_shrink_0()
            .justify_between()
            .gap(px(12.))
            .px(px(16.))
            .py(px(10.))
            .border_b_1()
            .border_color(border)
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.))
                    .line_height(normal_line(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(secondary)
                    .child(format!("TTY sessions for {path}")),
            )
            .child(
                h_flex().flex_shrink_0().gap(px(8.)).child(kill_orphans).child(
                    button("tty-refresh".into(), "Refresh")
                        .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.refresh(cx)))),
                ),
            );
        let centered = |child: gpui_kit::AnyElement| {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p(px(32.))
                .text_size(px(13.))
                .line_height(normal_line(13.))
                .text_color(secondary)
                .child(child)
                .into_any_element()
        };
        let body = match list {
            ProcessList::Loading => centered(div().child("Loading TTY sessions…").into_any_element()),
            ProcessList::Failed(message) => centered(
                v_flex()
                    .items_center()
                    .child(div().mb(px(6.)).font_weight(FontWeight::MEDIUM).text_color(rgb(DANGER)).child("Failed to load TTY sessions"))
                    .child(div().text_size(px(12.)).opacity(0.85).child(message))
                    .into_any_element(),
            ),
            ProcessList::Loaded(_) if rows.is_empty() => {
                centered(div().child("No TTY sessions for this workspace").into_any_element())
            }
            ProcessList::Loaded(_) => {
                let now = now_millis();
                let mut list = v_flex().id("tty-list-rows").flex_1().min_h_0().overflow_y_scroll();
                for row in rows {
                    let (open_id, kill_id) = (row.info.id.clone(), row.info.id.clone());
                    let short: String = row.info.id.chars().take(8).collect();
                    list = list.child(
                        h_flex()
                            .justify_between()
                            .gap(px(16.))
                            .px(px(16.))
                            .py(px(10.))
                            .border_b_1()
                            .border_color(border)
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w_0()
                                    .child(
                                        h_flex()
                                            .mb(px(4.))
                                            .font_family(MONO)
                                            .text_size(px(13.))
                                            .line_height(normal_line(13.))
                                            .text_color(primary)
                                            .child(short)
                                            .when(row.orphan, |d| {
                                                d.child(
                                                    div()
                                                        .id(SharedString::from(format!("tty-orphan-{}", row.info.id)))
                                                        .ml(px(8.))
                                                        .px(px(6.))
                                                        .py(px(1.))
                                                        .rounded(px(4.))
                                                        .border_1()
                                                        .border_color(rgb(WARNING))
                                                        .font_family(".SystemUIFont")
                                                        .text_size(px(10.))
                                                        .line_height(normal_line(10.))
                                                        .text_color(rgb(WARNING))
                                                        .child("ORPHAN")
                                                        .tooltip(|window, cx| {
                                                            gpui_kit::component::tooltip::Tooltip::new("No tab references this TTY")
                                                                .build(window, cx)
                                                        }),
                                                )
                                            }),
                                    )
                                    .child(
                                        h_flex()
                                            .gap(px(12.))
                                            .text_size(px(11.))
                                            .line_height(normal_line(11.))
                                            .text_color(secondary)
                                            .child(format!("{}×{}", row.info.cols, row.info.rows))
                                            .child(format!("created {}", relative_time(row.info.created_at, now)))
                                            .child(format!("active {}", relative_time(row.info.last_activity, now))),
                                    ),
                            )
                            .child(
                                h_flex()
                                    .gap(px(6.))
                                    .child(button(SharedString::from(format!("tty-open-{}", row.info.id)), "Open").on_click(
                                        cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.open(&open_id, cx))),
                                    ))
                                    .child(
                                        text_button(
                                            SharedString::from(format!("tty-kill-{}", row.info.id)),
                                            "Kill",
                                            12.,
                                            (4., 10.),
                                            gpui_kit::white(),
                                            rgb(DANGER),
                                            gpui_kit::transparent_black(),
                                            4.,
                                        )
                                        .on_click(cx.listener(move |view, _, _, cx| {
                                            view.model.update(cx, |m, cx| m.kill(&kill_id, cx));
                                        })),
                                    ),
                            ),
                    );
                }
                list.into_any_element()
            }
        };
        v_flex()
            .size_full()
            .bg(c.bg_primary)
            .text_color(primary)
            .child(toolbar)
            .when_some(kill_error, |d, text| {
                d.child(div().px(px(16.)).py(px(6.)).text_size(px(12.)).text_color(rgb(DANGER)).child(text))
            })
            .child(body)
    }
}
