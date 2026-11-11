//! "Window → Active Processes" dialog. Port of `ActiveProcessesDialog.tsx`; the live view
//! is the native terminal (Electron's `PtyViewer`).

use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, SharedString, Subscription, Window, div, px};
use treeterm_model::active_processes::{ActiveProcesses, ProcessList, display_name, relative_time};
use treeterm_model::session_store::SessionStore;

use crate::terminal_view::TerminalView;
use crate::theme::colors;
use crate::ui;

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

    /// `.active-processes-list`: 260px, a right border; one `.active-processes-item` per PTY.
    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let model = self.model.read(cx);
        let sessions = match model.list() {
            ProcessList::Loading => return empty("Loading…", c.text_secondary),
            ProcessList::Failed(error) => return empty(error.clone(), c.error),
            ProcessList::Loaded(sessions) if sessions.is_empty() => {
                return empty("No active PTY sessions", c.text_secondary);
            }
            ProcessList::Loaded(sessions) => sessions.clone(),
        };
        let selected = model.selected().map(|(id, _)| id.to_owned());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or_default();
        let mut list = div().id("process-list").flex().flex_col().size_full().overflow_y_scroll();
        for session in sessions {
            let is_selected = selected.as_deref() == Some(session.id.as_str());
            let id = session.id.clone();
            let (active, hover) = (c.bg_active, c.bg_hover);
            list = list.child(
                div()
                    .id(SharedString::from(format!("process-{}", session.id)))
                    .flex_shrink_0()
                    .px(px(16.))
                    .py(px(10.))
                    .cursor_pointer()
                    .border_b_1()
                    .border_color(c.border)
                    .when(is_selected, |d| d.bg(active))
                    .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                    .child(
                        div()
                            .mb(px(4.))
                            .text_size(px(ui::BODY))
                            .font_weight(gpui_kit::FontWeight::MEDIUM)
                            .text_color(c.text_primary)
                            .truncate()
                            .child(display_name(&session.cwd, &self.workspaces)),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .text_size(px(ui::SMALL))
                            .text_color(c.text_secondary)
                            .child(session.id.chars().take(8).collect::<String>())
                            .child(relative_time(session.created_at, now)),
                    )
                    .on_click(cx.listener(move |view, _, _, cx| view.model.update(cx, |m, cx| m.select(&id, cx)))),
            );
        }
        list.into_any_element()
    }
}

/// `.active-processes-empty`: 13px secondary text, centred.
fn empty(text: impl Into<SharedString>, color: gpui_kit::Hsla) -> AnyElement {
    let text: SharedString = text.into();
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(ui::BODY))
        .text_color(color)
        .child(text)
        .into_any_element()
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
            Some(terminal) => div()
                .flex()
                .flex_col()
                .size_full()
                // `.active-processes-toolbar`: 8px 12px, a bottom border; the red Stop button.
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .py(px(8.))
                        .border_b_1()
                        .border_color(border)
                        .child(
                            div()
                                .id("stop-process")
                                .px(px(12.))
                                .py(px(4.))
                                .rounded(px(4.))
                                .bg(gpui_kit::rgb(0xe74c3c))
                                .text_color(gpui_kit::white())
                                .text_size(px(12.))
                                .line_height(px(14.))
                                .cursor_pointer()
                                .hover(|d| d.opacity(0.9))
                                .child("Stop Process")
                                .on_click(cx.listener(|view, _, _, cx| view.model.update(cx, |m, cx| m.stop(cx)))),
                        )
                        .when_some(stop_error, |d, text| {
                            d.child(div().text_size(px(12.)).text_color(error).truncate().child(text))
                        }),
                )
                .child(div().flex_1().min_h_0().p(px(4.)).child(terminal))
                .into_any_element(),
            None => empty("Select a process to view its terminal", secondary),
        };
        // `.active-processes-dialog`: 70vh tall; the header, then the list beside the detail.
        let height = f32::from(window.viewport_size().height) * 0.7;
        ui::dialog_root(cx)
            .h(px(height))
            .child(ui::dialog_header("Active Processes", "×", colors(cx).text_primary, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(div().w(px(260.)).flex_shrink_0().h_full().border_r_1().border_color(border).child(list))
                    .child(div().flex_1().min_w_0().h_full().child(detail)),
            )
    }
}

pub fn open(session: &Entity<SessionStore>, settings: treeterm_core::settings::TerminalSettings, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ActiveProcessesView::new(session, settings, cx));
    // `width: 90%; max-width: 1000px`.
    let width = (f32::from(window.viewport_size().width) * 0.9).min(1000.);
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, width, cx).child(view.clone()));
}

/// Ask before closing when child workspaces are still unmerged (`CloseConfirmDialog.tsx`).
pub fn confirm_close(unmerged: Vec<(String, Option<String>)>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _, cx| {
        let c = colors(cx);
        // `.close-confirm-list`: bg-tertiary, 4px radius, 8px 12px padding, at most 200px.
        let mut list = div()
            .id("close-confirm-list")
            .flex()
            .flex_col()
            .max_h(px(200.))
            .overflow_y_scroll()
            .px(px(12.))
            .py(px(8.))
            .rounded(px(4.))
            .bg(c.bg_tertiary);
        for (name, branch) in &unmerged {
            list = list.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .py(px(6.))
                    .text_size(px(ui::BODY))
                    .child(
                        div().font_weight(gpui_kit::FontWeight::MEDIUM).text_color(c.text_primary).child(name.clone()),
                    )
                    .when_some(branch.clone(), |d, branch| {
                        d.child(
                            div()
                                .font_family("Menlo")
                                .text_size(px(12.))
                                .text_color(c.accent)
                                .child(format!("({branch})")),
                        )
                    }),
            );
        }
        let body = div()
            .p(px(20.))
            .child(
                div()
                    .mb(px(12.))
                    .text_size(px(14.))
                    .text_color(c.text_secondary)
                    .child("The following sub-workspaces have not been merged:"),
            )
            .child(list)
            .child(
                div()
                    .mt(px(12.))
                    .text_size(px(12.))
                    .italic()
                    .text_color(c.text_secondary)
                    .child("Closing the application will not delete these workspaces, but their work remains unmerged."),
            );
        let actions = ui::dialog_actions(cx)
            .child(
                ui::dialog_button("close-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                ui::dialog_button("close-anyway", "Close Anyway", ui::ButtonKind::Warning, false, cx)
                    .on_click(|_, _, cx| cx.quit()),
            );
        ui::dialog_frame(dialog, 450., cx).child(
            ui::dialog_root(cx)
                .child(ui::dialog_header("Unmerged Workspaces", "x", gpui_kit::rgb(0xf0a000), cx))
                .child(body)
                .child(actions),
        )
    });
}
