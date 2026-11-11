//! The TTYs tab (`SessionTtyList.tsx`): every PTY of the session grouped by workspace, with
//! Open (a live viewer dialog) and Kill.

use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, rgb};
use treeterm_model::active_processes::{ActiveProcesses, ProcessList, group_by_workspace, relative_time};
use treeterm_model::session_store::SessionStore;

use crate::terminal_view::TerminalView;
use crate::theme::colors;
use crate::ui::{MONO, dialog_close, dialog_shadow, modal, normal_line, text_button};

/// `.tty-list-kill-btn`'s red (`--color-danger` is not defined).
const DANGER: u32 = 0xe74c3c;

pub struct SessionTtys {
    model: Entity<ActiveProcesses>,
    /// `(path, name)` of the session's workspaces, to group PTYs by their cwd.
    workspaces: Vec<(String, String)>,
    viewer: Option<(String, Entity<TerminalView>)>,
    _observe: Subscription,
}

impl SessionTtys {
    pub fn new(session: &Entity<SessionStore>, settings: treeterm_core::settings::TerminalSettings, cx: &mut Context<Self>) -> Self {
        let (conn, workspaces) = {
            let session = session.read(cx);
            let workspaces = session
                .workspace_ids()
                .filter_map(|id| session.workspace_store(id))
                .map(|w| {
                    let w = w.read(cx).workspace();
                    (w.path.clone(), w.file.name.clone())
                })
                .collect();
            (session.connection().clone(), workspaces)
        };
        let model = cx.new(|cx| ActiveProcesses::new(conn, settings, cx));
        let observe = cx.observe(&model, |_, _, cx| cx.notify());
        Self { model, workspaces, viewer: None, _observe: observe }
    }

    /// `.tty-list-refresh-btn` / `.tty-list-open-btn`.
    fn button(id: impl Into<gpui_kit::ElementId>, label: &'static str, cx: &Context<Self>) -> gpui_kit::Stateful<gpui_kit::Div> {
        let c = colors(cx);
        text_button(id, label, 12., (4., 10.), c.text_primary, c.bg_active, c.border, 4.)
    }

    fn kill_button(id: impl Into<gpui_kit::ElementId>) -> gpui_kit::Stateful<gpui_kit::Div> {
        text_button(id, "Kill", 12., (4., 10.), gpui_kit::white(), rgb(DANGER), gpui_kit::transparent_black(), 4.)
    }

    /// The viewer of the selected PTY, made when the selection changes.
    fn viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<(String, Entity<TerminalView>)> {
        let (id, terminal) = {
            let model = self.model.read(cx);
            let (id, terminal) = model.selected()?;
            (id.to_owned(), terminal.clone())
        };
        match &self.viewer {
            Some((shown, view)) if *shown == id => Some((id, view.clone())),
            Some(_) | None => {
                let view = cx.new(|cx| TerminalView::new(terminal, window, cx));
                self.viewer = Some((id.clone(), view.clone()));
                Some((id, view))
            }
        }
    }

    fn body(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, border) = (c.text_secondary, c.text_primary, c.border);
        let centered = |child: AnyElement| {
            v_flex()
                .flex_1()
                .items_center()
                .justify_center()
                .p(px(32.))
                .text_size(px(13.)).line_height(normal_line(13.))
                .text_color(secondary)
                .child(child)
                .into_any_element()
        };
        let sessions = match self.model.read(cx).list() {
            ProcessList::Loading => return centered(div().child("Loading TTY sessions…").into_any_element()),
            ProcessList::Failed(message) => {
                return centered(
                    v_flex()
                        .items_center()
                        .child(div().mb(px(6.)).font_weight(FontWeight::MEDIUM).text_color(rgb(DANGER)).child("Failed to load TTY sessions"))
                        .child(div().text_size(px(12.)).line_height(normal_line(12.)).opacity(0.85).child(message.clone()))
                        .into_any_element(),
                );
            }
            ProcessList::Loaded(sessions) => sessions.clone(),
        };
        let groups = group_by_workspace(&sessions, &self.workspaces);
        if groups.is_empty() {
            return centered(div().child("No TTY sessions").into_any_element());
        }
        let now = treeterm_model::tool_tabs::tty_list::now_millis();
        let mut rows = v_flex().id("session-tty-rows").flex_1().min_h_0().overflow_y_scroll();
        for (g, group) in groups.into_iter().enumerate() {
            rows = rows.child(
                div()
                    .px(px(16.))
                    .py(px(6.))
                    .bg(c.bg_secondary)
                    .border_b_1()
                    .border_color(border)
                    .text_size(px(11.)).line_height(normal_line(11.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(secondary)
                    .child(format!("{} ({})", group.label, group.sessions.len()).to_uppercase()),
            );
            for (i, session) in group.sessions.into_iter().enumerate() {
                let (open, kill) = (session.id.clone(), session.id.clone());
                let short: String = session.id.chars().take(8).collect();
                rows = rows.child(
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
                                .child(div().mb(px(4.)).font_family(MONO).text_size(px(13.)).line_height(normal_line(13.)).text_color(primary).child(short))
                                .child(
                                    h_flex()
                                        .gap(px(12.))
                                        .text_size(px(11.)).line_height(normal_line(11.))
                                        .text_color(secondary)
                                        .child(format!("{}×{}", session.cols, session.rows))
                                        .child(format!("created {}", relative_time(session.created_at, now)))
                                        .child(format!("active {}", relative_time(session.last_activity, now))),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap(px(6.))
                                .child(Self::button(SharedString::from(format!("tty-open-{g}-{i}")), "Open", cx).on_click(
                                    cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.select(&open, cx))),
                                ))
                                .child(Self::kill_button(SharedString::from(format!("tty-kill-{g}-{i}"))).on_click(
                                    cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.kill(&kill, cx))),
                                )),
                        ),
                );
            }
        }
        rows.into_any_element()
    }
}

impl Render for SessionTtys {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewer = self.viewer(window, cx);
        if viewer.is_none() {
            self.viewer = None;
        }
        let c = colors(cx);
        let (bg, secondary, primary, border) = (c.bg_primary, c.text_secondary, c.text_primary, c.border);
        let dialog = viewer.map(|(id, terminal)| {
            let size = window.viewport_size();
            let width = (f32::from(size.width) * 0.8).min(1000.);
            let short: String = id.chars().take(8).collect();
            let model = self.model.clone();
            let content = v_flex()
                .id("pty-viewer-dialog")
                .occlude()
                .w(px(width))
                .h(size.height * 0.7)
                .rounded(px(8.))
                .bg(c.bg_secondary)
                .shadow(dialog_shadow())
                .overflow_hidden()
                .child(
                    h_flex()
                        .justify_between()
                        .px(px(16.))
                        .py(px(10.))
                        .border_b_1()
                        .border_color(border)
                        .child(div().font_family(MONO).text_size(px(13.)).line_height(normal_line(13.)).text_color(secondary).child(short))
                        .child(
                            h_flex()
                                .gap(px(8.))
                                .child(Self::kill_button("pty-viewer-kill").on_click(cx.listener(move |view, _, _, cx| {
                                    view.model.update(cx, |m, cx| m.kill(&id, cx));
                                })))
                                .child(dialog_close("pty-viewer-close", "×", cx).on_click(cx.listener(|view, _, _, cx| {
                                    view.model.update(cx, |m, cx| m.deselect(cx));
                                }))),
                        ),
                )
                // `.active-processes-pty-viewer`: the terminal inset 4px.
                .child(div().flex_1().min_h_0().p(px(4.)).child(terminal));
            modal(window, content, move |_, cx| model.update(cx, |m, cx| m.deselect(cx)))
        });
        v_flex()
            .flex_1()
            .size_full()
            .min_h_0()
            .bg(bg)
            .text_color(primary)
            .child(
                h_flex()
                    .justify_between()
                    .gap(px(12.))
                    .px(px(16.))
                    .py(px(10.))
                    .border_b_1()
                    .border_color(border)
                    .child(div().truncate().text_size(px(13.)).line_height(normal_line(13.)).font_weight(FontWeight::MEDIUM).text_color(secondary).child("TTY sessions"))
                    .child(
                        Self::button("tty-refresh", "Refresh", cx)
                            .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.reload(cx)))),
                    ),
            )
            .child(self.body(cx))
            .children(dialog)
    }
}
