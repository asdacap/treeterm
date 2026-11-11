//! "Window → Active Processes" dialog. Port of `ActiveProcessesDialog.tsx`; the live view
//! is the native terminal (Electron's `PtyViewer`).

use gpui_kit::component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::active_processes::{ActiveProcesses, ProcessList, display_name, relative_time};
use treeterm_model::session_store::SessionStore;

use crate::terminal_view::TerminalView;
use crate::theme::colors;

pub struct ActiveProcessesView {
    model: Entity<ActiveProcesses>,
    /// `(path, name)` of the session's workspaces, to name processes by their cwd.
    workspaces: Vec<(String, String)>,
    terminal: Option<(String, Entity<TerminalView>)>,
    _observe: Subscription,
}

impl ActiveProcessesView {
    pub(crate) fn new(session: &Entity<SessionStore>, settings: treeterm_core::settings::TerminalSettings, cx: &mut Context<Self>) -> Self {
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
        Self {
            model,
            workspaces,
            terminal: None,
            _observe: observe,
        }
    }

    /// The view of the selected process's terminal, made when the selection changes.
    fn terminal_view(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Option<Entity<TerminalView>> {
        let (id, terminal) = {
            let model = self.model.read(cx);
            let (id, terminal) = model.selected()?;
            (id.to_owned(), terminal.clone())
        };
        match &self.terminal {
            Some((shown, view)) if *shown == id => Some(view.clone()),
            Some(_) | None => {
                let view = cx.new(|cx| TerminalView::new(terminal, window, cx));
                self.terminal = Some((id, view.clone()));
                Some(view)
            }
        }
    }

    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let model = self.model.read(cx);
        let empty = |text: &str| div().p_3().text_sm().text_color(c.text_secondary).child(text.to_owned()).into_any_element();
        let sessions = match model.list() {
            ProcessList::Loading => return empty("Loading…"),
            ProcessList::Failed(error) => {
                return div().p_3().text_sm().text_color(c.error).child(error.clone()).into_any_element();
            }
            ProcessList::Loaded(sessions) if sessions.is_empty() => return empty("No active PTY sessions"),
            ProcessList::Loaded(sessions) => sessions.clone(),
        };
        let selected = model.selected().map(|(id, _)| id.to_owned());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        let mut list = v_flex().id("process-list").size_full().overflow_y_scroll();
        for session in sessions {
            let is_selected = selected.as_deref() == Some(session.id.as_str());
            let id = session.id.clone();
            list = list.child(
                v_flex()
                    .id(SharedString::from(format!("process-{}", session.id)))
                    .px_3()
                    .py_2()
                    .gap_0p5()
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(c.border)
                    .when(is_selected, |d| d.bg(c.bg_active))
                    .when(!is_selected, |d| d.hover(move |d| d.bg(c.bg_hover)))
                    .child(div().text_sm().truncate().child(display_name(&session.cwd, &self.workspaces)))
                    .child(
                        h_flex()
                            .gap_2()
                            .text_xs()
                            .text_color(c.text_secondary)
                            // Electron shows the first 8 characters, which are the same for
                            // every `pty-<timestamp>-<n>` id; the whole id tells them apart.
                            .child(session.id.clone())
                            .child(relative_time(session.created_at, now)),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.select(&id, cx)))),
            );
        }
        list.into_any_element()
    }
}

impl Render for ActiveProcessesView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (border, secondary, error) = {
            let c = colors(cx);
            (c.border, c.text_secondary, c.error)
        };
        let list = self.list(cx);
        let stop_error = self.model.read(cx).stop_error().map(str::to_owned);
        let detail = match self.terminal_view(window, cx) {
            Some(terminal) => v_flex()
                .size_full()
                .child(
                    h_flex().p_1().justify_end().child(
                        Button::new("stop-process")
                            .small()
                            .with_variant(ButtonVariant::Danger)
                            .label("Stop Process")
                            .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.stop(cx)))),
                    ),
                )
                .child(div().flex_1().min_h_0().child(terminal))
                .into_any_element(),
            None => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_sm()
                .text_color(secondary)
                .child("Select a process to view its terminal")
                .into_any_element(),
        };
        v_flex()
            .size_full()
            .min_h(px(480.))
            .when_some(stop_error, |d, text| d.child(div().pb_1().text_sm().text_color(error).child(text)))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .border_1()
                    .border_color(border)
                    .child(div().w(px(220.)).h_full().border_r_1().border_color(border).child(list))
                    .child(div().flex_1().min_w_0().h_full().child(detail)),
            )
    }
}

pub fn open(session: &Entity<SessionStore>, settings: treeterm_core::settings::TerminalSettings, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ActiveProcessesView::new(session, settings, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        dialog.title("Active Processes").w(px(900.)).child(view.clone())
    });
}

/// Ask before closing when child workspaces are still unmerged (`CloseConfirmDialog.tsx`).
pub fn confirm_close(unmerged: Vec<(String, Option<String>)>, window: &mut Window, cx: &mut App) {
    let secondary = colors(cx).text_secondary;
    window.open_alert_dialog(cx, move |alert, _, _| {
        let mut list = v_flex().gap_1().py_2();
        for (name, branch) in &unmerged {
            list = list.child(
                h_flex()
                    .gap_2()
                    .child(div().child(format!("• {name}")))
                    .when_some(branch.clone(), |d, branch| d.child(div().text_color(secondary).child(format!("({branch})")))),
            );
        }
        alert
            .title("Unmerged Workspaces")
            .description(
                v_flex()
                    .gap_1()
                    .child("The following sub-workspaces have not been merged:")
                    .child(list)
                    .child("Closing the application will not delete these workspaces, but their work remains unmerged."),
            )
            .show_cancel(true)
            .ok_text("Close Anyway")
            .ok_variant(ButtonVariant::Danger)
            .on_ok(|_, _, cx| {
                cx.quit();
                true
            })
    });
}
