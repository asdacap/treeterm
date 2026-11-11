//! The session pane (click a session's name). Port of `SessionInfoPane.tsx`: Info, SSH (with
//! its Bootstrap / Tunnel / Daemon / Port Forwards / Monitor / Upload sub-tabs), JSON and
//! TTYs, plus a Daemon Mismatch tab while the remote daemon is outdated.

mod forward_dialog;
mod monitor;
mod ttys;

use std::collections::HashSet;

use gpui_kit::component::spinner::Spinner;
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, Hsla, ScrollHandle, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::connection::{ConnectionId, ConnectionTarget, OutdatedDaemon, OutputKind, PortForwardStatus};
use treeterm_model::app_store::{AppStore, ConnState};
use treeterm_model::session_info::SessionInfo;

use crate::theme::colors;
use crate::ui::{MONO, normal_line, pane_button, pane_subtabs, pane_tab, pane_tabs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Info,
    Ssh,
    Json,
    Tty,
    Mismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SshTab {
    Log(OutputKind),
    Forwards,
    Monitor,
    Upload,
}

/// The connection as Electron's `ConnectionStatus` names it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Connecting,
    Connected,
    Reconnecting,
    Error,
    Disconnected,
}

impl Status {
    fn of(state: Option<&ConnState>) -> Self {
        match state {
            Some(ConnState::Connecting(_)) => Status::Connecting,
            Some(ConnState::Connected(_)) => Status::Connected,
            Some(ConnState::Reconnecting { .. }) => Status::Reconnecting,
            Some(ConnState::Failed { .. }) => Status::Error,
            None => Status::Disconnected,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Status::Connecting => "connecting",
            Status::Connected => "connected",
            Status::Reconnecting => "reconnecting",
            Status::Error => "error",
            Status::Disconnected => "disconnected",
        }
    }

    /// `getStatusColor`.
    fn color(self) -> u32 {
        match self {
            Status::Connected => 0x4caf50,
            Status::Connecting | Status::Reconnecting => 0xff9800,
            Status::Error | Status::Disconnected => 0xf44336,
        }
    }
}

/// `getPfStatusColor`.
fn forward_color(status: &PortForwardStatus) -> u32 {
    match status {
        PortForwardStatus::Active => 0x4caf50,
        PortForwardStatus::Connecting => 0xff9800,
        PortForwardStatus::Error(_) => 0xf44336,
        PortForwardStatus::Stopped => 0x666666,
    }
}

const RED: u32 = 0xf44336;
const ORANGE: u32 = 0xff9800;

/// The JSON tab's keys in the order of Electron's `sessionData`; keys only the native
/// session has follow.
fn electron_order(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::Object(mut map) = value else {
        return value;
    };
    let mut ordered = serde_json::Map::new();
    for key in ["sessionId", "connection", "activeWorkspaceId", "sessionVersion", "workspaces"] {
        if let Some(field) = map.remove(key) {
            ordered.insert(key.to_owned(), field);
        }
    }
    ordered.extend(map);
    serde_json::Value::Object(ordered)
}

pub struct SessionInfoView {
    app: Entity<AppStore>,
    names: Entity<treeterm_model::session_names::SessionNames>,
    info: Entity<SessionInfo>,
    /// The tab the user picked; `None` follows the default (Daemon Mismatch, SSH or Info).
    selected: Option<Tab>,
    ssh_tab: SshTab,
    /// Logs stick to their bottom as lines arrive.
    log_scroll: ScrollHandle,
    log_len: (SshTab, usize),
    forward_dialog: Option<Entity<forward_dialog::ForwardDialog>>,
    /// Polls only while the Monitor sub-tab is shown.
    monitor: Option<Entity<monitor::MonitorView>>,
    /// JSON tab: paths flipped from their default fold, and the session it follows while shown.
    json_toggled: HashSet<String>,
    json_follow: Option<Subscription>,
    /// TTYs tab: the session's PTYs, grouped by workspace.
    ttys: Option<Entity<ttys::SessionTtys>>,
    upload: Option<Entity<crate::tool_views::SshUploadView>>,
    _observe: [Subscription; 3],
}

impl SessionInfoView {
    pub fn new(
        app: Entity<AppStore>,
        names: Entity<treeterm_model::session_names::SessionNames>,
        id: ConnectionId,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let info = cx.new(|cx| SessionInfo::new(&app, id, cx));
        let observe = [
            cx.observe(&info, |_, _, cx| cx.notify()),
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.observe(&names, |_, _, cx| cx.notify()),
        ];
        Self {
            app,
            names,
            info,
            selected: None,
            ssh_tab: SshTab::Log(OutputKind::Bootstrap),
            log_scroll: ScrollHandle::new(),
            log_len: (SshTab::Log(OutputKind::Bootstrap), 0),
            forward_dialog: None,
            monitor: None,
            json_toggled: HashSet::new(),
            json_follow: None,
            ttys: None,
            upload: None,
            _observe: observe,
        }
    }

    pub fn connection(&self, cx: &App) -> ConnectionId {
        self.info.read(cx).id().clone()
    }

    /// Connect the ssh host again (`reconnect`): Retry, Refresh daemon, connect anyway.
    fn reconnect(&self, outdated: OutdatedDaemon, cx: &mut Context<Self>) {
        let id = self.connection(cx);
        let target = self.app.read(cx).session_slot(&id).map(|slot| slot.target.clone());
        match target {
            Some(ConnectionTarget::Remote(config)) => {
                let config = (*config).clone();
                self.app.update(cx, |app, cx| app.connect_remote(config, outdated, cx));
            }
            // Electron only reconnects ssh sessions; the local one retries in place.
            Some(ConnectionTarget::Local) => self.app.read(cx).reconnect(&id),
            None => {}
        }
    }

    /// `.ssh-pane-output`: padded 8px, monospace 12px, line-height 1.5, scrolling.
    fn output(id: &'static str) -> gpui_kit::Stateful<gpui_kit::Div> {
        v_flex()
            .id(id)
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(8.))
            .font_family(MONO)
            .text_size(px(12.))
            .line_height(px(18.))
    }

    fn info_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = Self::output("session-info");
        for line in self.info.read(cx).info_lines(cx) {
            list = list.child(div().child(line));
        }
        list.into_any_element()
    }

    fn log_view(&mut self, kind: OutputKind, cx: &mut Context<Self>) -> AnyElement {
        let secondary = colors(cx).text_secondary;
        let lines = self.info.read(cx).log(kind).to_vec();
        let key = (SshTab::Log(kind), lines.len());
        if self.log_len != key {
            self.log_len = key;
            self.log_scroll.scroll_to_bottom();
        }
        let list = Self::output("session-log").track_scroll(&self.log_scroll);
        if lines.is_empty() {
            let what = match kind {
                OutputKind::Bootstrap => "No bootstrap output yet...",
                OutputKind::Tunnel => "No tunnel output yet...",
                OutputKind::Daemon => "No daemon output yet...",
            };
            return list.child(div().text_color(secondary).child(what)).into_any_element();
        }
        list.children(lines.into_iter().map(|line| div().child(line))).into_any_element()
    }

    /// The port forward list (`.port-forward-list`): status, Log / Hide, Restart / Remove
    /// or Stop, the error and the expanded ssh output.
    fn forwards_view(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, border, bg) = (c.text_secondary, c.text_primary, c.border, c.bg_primary);
        let info = self.info.read(cx);
        let list = Self::output("port-forwards");
        if info.forwards().is_empty() {
            return list.child(div().text_color(secondary).child("No port forwards configured.")).into_any_element();
        }
        let count = info.forwards().len();
        let mut rows = v_flex();
        for (i, forward) in info.forwards().iter().enumerate() {
            let spec = &forward.config.spec;
            let id = forward.config.id.clone();
            let log = info.forward_log(&id).map(<[String]>::to_vec);
            let expanded = log.is_some();
            let failed = matches!(forward.status, PortForwardStatus::Error(_) | PortForwardStatus::Stopped);
            let button = |name: &str, label: &'static str, color: Hsla| {
                pane_button(SharedString::from(format!("pf-{name}-{i}")), label, false, color)
            };
            let mut header = h_flex()
                .gap(px(6.))
                .p(px(8.))
                .child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(rgb(forward_color(&forward.status))))
                .child(
                    div()
                        .flex_1()
                        .text_color(primary)
                        .child(format!("localhost:{} → {}:{}", spec.local_port, spec.remote_host, spec.remote_port)),
                )
                .child(div().text_color(secondary).child(format!("({})", forward.status.as_str())));
            let toggle = id.clone();
            header = header.child(
                button("log", if expanded { "Hide" } else { "Log" }, secondary)
                    .hover(move |d| d.text_color(primary))
                    .on_click(cx.listener(move |view, _, _, cx| {
                        view.info.update(cx, |info, cx| info.toggle_forward_log(&toggle, cx));
                    })),
            );
            if failed {
                let (restart, remove) = (id.clone(), id.clone());
                header = header
                    .child(button("restart", "Restart", rgb(ORANGE).into()).on_click(cx.listener(move |view, _, _, cx| {
                        view.info.update(cx, |info, cx| info.restart_forward(&restart, cx));
                    })))
                    .child(button("remove", "Remove", rgb(RED).into()).on_click(cx.listener(move |view, _, _, cx| {
                        view.info.update(cx, |info, cx| info.remove_forward(&remove, cx));
                    })));
            } else {
                let stop = id.clone();
                header = header.child(button("stop", "Stop", rgb(RED).into()).on_click(cx.listener(move |view, _, _, cx| {
                    view.info.update(cx, |info, cx| info.remove_forward(&stop, cx));
                })));
            }
            let error = match &forward.status {
                PortForwardStatus::Error(message) => Some(message.to_string()),
                PortForwardStatus::Active | PortForwardStatus::Connecting | PortForwardStatus::Stopped => None,
            };
            let output = log.map(|lines| {
                let body = if lines.is_empty() {
                    vec![div().text_color(secondary).child("No output yet...").into_any_element()]
                } else {
                    lines.into_iter().map(|line| div().child(line).into_any_element()).collect()
                };
                v_flex()
                    .id(SharedString::from(format!("pf-output-{i}")))
                    .max_h(px(200.))
                    .overflow_y_scroll()
                    .px(px(8.))
                    .py(px(6.))
                    .bg(bg)
                    .border_t_1()
                    .border_color(border)
                    .text_size(px(11.))
                    .line_height(px(16.5))
                    .children(body)
            });
            rows = rows.child(
                v_flex()
                    .when(i + 1 < count, |d| d.border_b_1().border_color(border))
                    .child(header)
                    .when_some(error, |d, error| {
                        d.child(
                            div()
                                .px(px(8.))
                                .pb(px(6.))
                                .text_size(px(11.))
                                .line_height(px(16.5))
                                .text_color(rgb(RED))
                                .child(error),
                        )
                    })
                    .children(output),
            );
        }
        list.child(rows).into_any_element()
    }

    fn mismatch_view(&self, message: Option<String>, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (primary, secondary) = (c.text_primary, c.text_secondary);
        let action = |button: gpui_kit::Stateful<gpui_kit::Div>, note: &'static str, color: Hsla| {
            h_flex()
                .flex_wrap()
                .gap(px(10.))
                .child(button)
                .child(div().text_size(px(12.)).text_color(color).child(note))
        };
        Self::output("session-mismatch")
            .gap(px(16.))
            .child(
                div()
                    .text_color(primary)
                    .child(message.unwrap_or_else(|| "The remote daemon binary does not match the local build.".to_owned())),
            )
            .child(action(
                pane_button("mismatch-refresh", "Refresh daemon", false, rgb(ORANGE))
                    .on_click(cx.listener(|view, _, _, cx| view.reconnect(OutdatedDaemon::Refresh, cx))),
                "⚠ Drops all running processes on the remote.",
                rgb(ORANGE).into(),
            ))
            .child(action(
                pane_button("mismatch-accept", "Ignore mismatch & connect anyway", false, secondary)
                    .hover(move |d| d.text_color(primary))
                    .on_click(cx.listener(|view, _, _, cx| view.reconnect(OutdatedDaemon::Accept, cx))),
                "Connects against the outdated remote daemon.",
                secondary,
            ))
            .into_any_element()
    }

    fn json_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.json_follow.is_none()
            && let Some(session) = self.info.read(cx).session(cx)
        {
            self.json_follow = Some(cx.observe(&session, |_, _, cx| cx.notify()));
        }
        let c = colors(cx);
        let (secondary, primary) = (c.text_secondary, c.text_primary);
        let value = electron_order(self.info.read(cx).debug_json(cx));
        let tree = crate::json_view::tree(&value, &self.json_toggled);
        let view = cx.entity().downgrade();
        let viewer = crate::json_view::render(tree, secondary, primary, move |path, _, cx| {
            match view.update(cx, |view, cx| {
                if !view.json_toggled.remove(&path) {
                    view.json_toggled.insert(path);
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the pane closed
            }
        });
        Self::output("session-json").child(viewer).into_any_element()
    }

    fn ttys_view(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.ttys.is_none()
            && let Some(session) = self.info.read(cx).session(cx)
        {
            let settings = self.app.read(cx).settings().read(cx).settings().terminal.clone();
            self.ttys = Some(cx.new(|cx| ttys::SessionTtys::new(&session, settings, cx)));
        }
        match &self.ttys {
            Some(view) => div().flex_1().min_h_0().child(view.clone()).into_any_element(),
            None => Self::output("session-ttys")
                .child(div().text_color(colors(cx).text_secondary).child("Not connected"))
                .into_any_element(),
        }
    }

    fn monitor_view(&mut self, state: Option<&ConnState>, cx: &mut Context<Self>) -> AnyElement {
        let Some(ConnState::Connected(session)) = state else {
            self.monitor = None;
            return div().into_any_element();
        };
        let monitor = match &self.monitor {
            Some(monitor) => monitor.clone(),
            None => {
                let exec = session.read(cx).connection().exec_client();
                let monitor = cx.new(|cx| monitor::MonitorView::new(exec, cx));
                self.monitor = Some(monitor.clone());
                monitor
            }
        };
        div().flex_1().min_h_0().flex().flex_col().child(monitor).into_any_element()
    }

    fn upload_view(&mut self, state: Option<&ConnState>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let Some(ConnState::Connected(session)) = state else {
            self.upload = None;
            return div().into_any_element();
        };
        let upload = match &self.upload {
            Some(upload) => upload.clone(),
            None => {
                let remote = treeterm_model::web_app::RemoteHost { app: self.app.downgrade(), connection: self.connection(cx) };
                let model = cx.new(|cx| treeterm_model::tool_tabs::ssh_upload::SshUpload::new(Some(remote), cx));
                let session = session.clone();
                let upload = cx.new(|cx| crate::tool_views::SshUploadView::new(model, session, window, cx));
                self.upload = Some(upload.clone());
                upload
            }
        };
        div().flex_1().min_h_0().flex().flex_col().child(upload).into_any_element()
    }

    fn open_forward_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let info = self.info.clone();
        let dialog = cx.new(|cx| forward_dialog::ForwardDialog::new(info, window, cx));
        cx.subscribe(&dialog, |view, _, _: &forward_dialog::Closed, cx| {
            view.forward_dialog = None;
            cx.notify();
        })
        .detach();
        self.forward_dialog = Some(dialog);
        cx.notify();
    }
}

impl Render for SessionInfoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (bg, primary, secondary) = (c.bg_primary, c.text_primary, c.text_secondary);
        let id = self.connection(cx);
        let (label, state, remote) = match self.app.read(cx).session_slot(&id) {
            Some(slot) => {
                let name = self.names.read(cx).name(&slot.target);
                // `deriveDefaultSessionName` calls the local session "LOCAL".
                let name = if slot.is_local() && name == treeterm_model::session_names::default_name(&slot.target) {
                    "LOCAL".to_owned()
                } else {
                    name
                };
                (name, Some(slot.state.clone()), !slot.is_local())
            }
            None => ("Disconnected".to_owned(), None, false),
        };
        let status = Status::of(state.as_ref());
        let connected = status == Status::Connected;
        let (error, mismatch) = match &state {
            Some(ConnState::Failed { error, hash_mismatch }) => (Some(error.clone()), *hash_mismatch),
            Some(ConnState::Connecting(_) | ConnState::Connected(_) | ConnState::Reconnecting { .. }) | None => (None, false),
        };

        // Header: status dot, name, spinner, status, error and the connection's actions.
        let mut header = h_flex()
            .flex_shrink_0()
            .gap(px(8.))
            .px(px(12.))
            .py(px(8.))
            .border_b_1()
            .border_color(c.border)
            .text_size(px(13.)).line_height(normal_line(13.))
            .when(remote, |d| d.child(div().size(px(8.)).flex_shrink_0().rounded_full().bg(rgb(status.color()))))
            .child(div().font_weight(gpui_kit::FontWeight::SEMIBOLD).child(label))
            .when(remote && matches!(status, Status::Connecting | Status::Reconnecting), |d| {
                d.child(div().ml(px(8.)).child(
                    Spinner::new().icon(gpui_kit::component::Icon::new(gpui_kit::assets::IconName::LoaderCircle)).with_size(px(14.)).color(primary),
                ))
            })
            .child(div().text_size(px(12.)).line_height(normal_line(12.)).text_color(secondary).child(if remote {
                format!("({})", status.as_str())
            } else {
                "(Local)".to_owned()
            }))
            .when_some(error.clone(), |d, error| d.child(div().text_size(px(12.)).line_height(normal_line(12.)).text_color(rgb(RED)).child(error)));
        if connected {
            let again = id.clone();
            header = header.child(
                h_flex().ml_auto().gap(px(8.)).child(
                    pane_button("session-reconnect", "Reconnect", false, secondary)
                        .hover(move |d| d.text_color(primary))
                        .on_click(cx.listener(move |view, _, _, cx| {
                            if let Err(error) = view.app.read(cx).manager().force_reconnect(&again) {
                                tracing::error!(%error, "reconnect");
                            }
                        })),
                ),
            );
        } else if matches!(status, Status::Error | Status::Disconnected) {
            let gone = id.clone();
            header = header.child(
                h_flex()
                    .ml_auto()
                    .gap(px(8.))
                    .child(
                        pane_button("session-retry", "Retry", false, secondary)
                            .hover(move |d| d.text_color(primary))
                            .on_click(cx.listener(|view, _, _, cx| view.reconnect(OutdatedDaemon::Report, cx))),
                    )
                    .child(pane_button("session-remove", "Remove", false, rgb(RED)).on_click(cx.listener(
                        move |view, _, _, cx| view.app.update(cx, |app, cx| app.disconnect(&gone, cx)),
                    ))),
            );
        }

        // Tabs: Info + SSH (remote) + JSON/TTYs (local, or remote once connected).
        let mut tabs: Vec<(Tab, &str)> = Vec::new();
        if mismatch {
            tabs.push((Tab::Mismatch, "Daemon Mismatch"));
        }
        tabs.push((Tab::Info, "Info"));
        if remote {
            tabs.push((Tab::Ssh, "SSH"));
        }
        if !remote || connected {
            tabs.extend([(Tab::Json, "JSON"), (Tab::Tty, "TTYs")]);
        }
        let default = if mismatch {
            Tab::Mismatch
        } else if remote {
            Tab::Ssh
        } else {
            Tab::Info
        };
        let active = self.selected.unwrap_or(default);
        let mut tab_bar = pane_tabs(cx);
        for (tab, text) in tabs {
            tab_bar = tab_bar.child(pane_tab(SharedString::from(format!("info-tab-{text}")), text, active == tab, false, cx).on_click(
                cx.listener(move |view, _, _, cx| {
                    view.selected = Some(tab);
                    cx.notify();
                }),
            ));
        }

        // SSH sub-tabs: the logs always, the rest once connected.
        let ssh = active == Tab::Ssh && remote;
        let mut ssh_tabs: Vec<(SshTab, &str)> = vec![
            (SshTab::Log(OutputKind::Bootstrap), "Bootstrap"),
            (SshTab::Log(OutputKind::Tunnel), "Tunnel"),
            (SshTab::Log(OutputKind::Daemon), "Daemon"),
        ];
        if connected {
            ssh_tabs.extend([
                (SshTab::Forwards, "Port Forwards"),
                (SshTab::Monitor, "Monitor"),
                (SshTab::Upload, "Upload"),
            ]);
        }
        let subtab_bar = ssh.then(|| {
            let mut bar = pane_subtabs(cx);
            for (tab, text) in ssh_tabs {
                bar = bar.child(
                    pane_tab(SharedString::from(format!("ssh-tab-{text}")), text, self.ssh_tab == tab, true, cx).on_click(
                        cx.listener(move |view, _, _, cx| {
                            view.ssh_tab = tab;
                            cx.notify();
                        }),
                    ),
                );
            }
            if self.ssh_tab == SshTab::Forwards && connected {
                let accent = colors(cx).accent;
                bar = bar.child(
                    pane_button("pf-add", "+ Add Port Forward", true, primary)
                        .ml_auto()
                        .border_color(accent)
                        .on_click(cx.listener(|view, _, window, cx| view.open_forward_dialog(window, cx))),
                );
            }
            bar
        });

        // Pollers and followers live only while shown.
        let ssh_tab = ssh.then_some(self.ssh_tab);
        if ssh_tab != Some(SshTab::Monitor) {
            self.monitor = None;
        }
        if ssh_tab != Some(SshTab::Upload) {
            self.upload = None;
        }
        if active != Tab::Json {
            self.json_follow = None;
        }
        if active != Tab::Tty {
            self.ttys = None;
        }
        if !(ssh && connected) {
            self.forward_dialog = None;
        }
        let body = match active {
            Tab::Mismatch => self.mismatch_view(error, cx),
            Tab::Info => self.info_view(cx),
            Tab::Json => self.json_view(cx),
            Tab::Tty => self.ttys_view(cx),
            Tab::Ssh => match self.ssh_tab {
                SshTab::Log(kind) => self.log_view(kind, cx),
                SshTab::Forwards => self.forwards_view(cx),
                SshTab::Monitor => self.monitor_view(state.as_ref(), cx),
                SshTab::Upload => self.upload_view(state.as_ref(), window, cx),
            },
        };
        let dialog = self.forward_dialog.clone().map(|dialog| {
            let close = dialog.clone();
            crate::ui::modal(window, dialog, move |_, cx| close.update(cx, |d, cx| d.close(cx)))
        });
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .bg(bg)
            .text_color(primary)
            .child(header)
            .child(tab_bar)
            .children(subtab_bar)
            .child(body)
            .children(dialog)
    }
}

#[cfg(test)]
mod tests {
    use super::electron_order;

    #[test]
    fn the_json_tab_lists_keys_in_electrons_order() {
        let value = serde_json::json!({ "workspaces": {}, "extra": 1, "sessionId": "s", "connection": {} });
        let keys: Vec<String> = electron_order(value).as_object().unwrap().keys().cloned().collect();
        assert_eq!(keys, ["sessionId", "connection", "workspaces", "extra"]);
    }
}
