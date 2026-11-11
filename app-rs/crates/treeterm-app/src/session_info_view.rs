//! The session pane (click a session's name): status, connection logs and port forwards.
//! Port of `SessionInfoPane.tsx` and `PortForwardDialog.tsx`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Context, Entity, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::connection::{ConnectionId, OutputKind, PortForwardStatus};
use treeterm_core::settings::PortForwardSpec;
use treeterm_model::app_store::{AppStore, ConnState};
use treeterm_model::session_info::SessionInfo;

use crate::theme::colors;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Log(OutputKind),
    PortForwards,
}

pub struct SessionInfoView {
    app: Entity<AppStore>,
    info: Entity<SessionInfo>,
    tab: Tab,
    local_port: Entity<InputState>,
    remote_host: Entity<InputState>,
    remote_port: Entity<InputState>,
    save_forward: bool,
    _observe: [Subscription; 2],
}

/// Status dot colour (`getStatusColor` / `getPfStatusColor`).
fn forward_color(status: &PortForwardStatus) -> u32 {
    match status {
        PortForwardStatus::Active => 0x4caf50,
        PortForwardStatus::Connecting => 0xff9800,
        PortForwardStatus::Error(_) => 0xf44336,
        PortForwardStatus::Stopped => 0x888888,
    }
}

fn parse_forward(local: &str, host: &str, remote: &str) -> Option<PortForwardSpec> {
    let port = |text: &str| text.trim().parse::<u16>().ok().filter(|p| *p > 0);
    let host = host.trim();
    (!host.is_empty()).then_some(())?;
    Some(PortForwardSpec {
        local_port: port(local)?,
        remote_host: host.to_owned(),
        remote_port: port(remote)?,
        extra: Default::default(),
    })
}

impl SessionInfoView {
    pub fn new(app: Entity<AppStore>, id: ConnectionId, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let info = cx.new(|cx| SessionInfo::new(&app, id, cx));
        let observe = [cx.observe(&info, |_, _, cx| cx.notify()), cx.observe(&app, |_, _, cx| cx.notify())];
        let remote = app.read(cx).session_slot(info.read(cx).id()).is_some_and(|slot| !slot.is_local());
        let mut input = |placeholder: &str, value: &str| {
            let (placeholder, value) = (placeholder.to_owned(), value.to_owned());
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        Self {
            local_port: input("Local port", ""),
            remote_host: input("Remote host", "localhost"),
            remote_port: input("Remote port", ""),
            app,
            info,
            tab: if remote { Tab::Log(OutputKind::Bootstrap) } else { Tab::Log(OutputKind::Daemon) },
            save_forward: true,
            _observe: observe,
        }
    }

    pub fn connection(&self, cx: &gpui_kit::App) -> ConnectionId {
        self.info.read(cx).id().clone()
    }

    fn add_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = |state: &Entity<InputState>, cx: &gpui_kit::App| state.read(cx).value().to_string();
        let Some(spec) = parse_forward(&text(&self.local_port, cx), &text(&self.remote_host, cx), &text(&self.remote_port, cx))
        else {
            return;
        };
        let saved = self.save_forward;
        self.info.update(cx, |info, cx| info.add_forward(spec, saved, cx));
        self.local_port.update(cx, |input, cx| input.set_value("", window, cx));
        self.remote_port.update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn log_view(&self, kind: OutputKind, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let lines = self.info.read(cx).log(kind);
        if lines.is_empty() {
            let what = match kind {
                OutputKind::Bootstrap => "No bootstrap output yet…",
                OutputKind::Tunnel => "No tunnel output yet…",
                OutputKind::Daemon => "No daemon output yet…",
            };
            return div().p_3().text_sm().text_color(c.text_secondary).child(what).into_any_element();
        }
        let mut list = v_flex().id("session-log").size_full().overflow_y_scroll().p_2().font_family("Menlo").text_size(px(12.));
        for line in lines.iter().rev().take(500).rev() {
            list = list.child(div().child(line.clone()));
        }
        list.into_any_element()
    }

    fn forwards_view(&self, remote: bool, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        if !remote {
            return div().p_3().text_sm().text_color(c.text_secondary).child("Port forwards are for SSH sessions.").into_any_element();
        }
        let info = self.info.read(cx);
        let mut list = v_flex().gap_1().p_2();
        if info.forwards().is_empty() {
            list = list.child(div().text_sm().text_color(c.text_secondary).child("No port forwards configured."));
        }
        for (i, forward) in info.forwards().iter().enumerate() {
            let spec = &forward.config.spec;
            let failed = matches!(forward.status, PortForwardStatus::Error(_) | PortForwardStatus::Stopped);
            let (restart, remove) = (forward.config.id.clone(), forward.config.id.clone());
            let detail = match &forward.status {
                PortForwardStatus::Error(error) => format!("(error: {error})"),
                status => format!("({})", status.as_str()),
            };
            list = list.child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .text_sm()
                    .child(div().size(px(8.)).rounded_full().bg(rgb(forward_color(&forward.status))))
                    .child(format!("localhost:{} → {}:{}", spec.local_port, spec.remote_host, spec.remote_port))
                    .child(div().text_xs().text_color(c.text_secondary).child(detail))
                    .child(div().flex_1())
                    .when(failed, |d| {
                        d.child(Button::new(SharedString::from(format!("pf-restart-{i}"))).xsmall().ghost().label("Restart").on_click(
                            cx.listener(move |view, _, _, cx| view.info.update(cx, |info, cx| info.restart_forward(&restart, cx))),
                        ))
                    })
                    .child(Button::new(SharedString::from(format!("pf-stop-{i}"))).xsmall().ghost().label("Remove").on_click(
                        cx.listener(move |view, _, _, cx| view.info.update(cx, |info, cx| info.remove_forward(&remove, cx))),
                    )),
            );
        }
        let error = info.error().map(str::to_owned);
        list.child(
            h_flex()
                .pt_2()
                .gap_2()
                .items_center()
                .child(div().w(px(100.)).child(Input::new(&self.local_port).small()))
                .child("→")
                .child(div().w(px(160.)).child(Input::new(&self.remote_host).small()))
                .child(div().w(px(100.)).child(Input::new(&self.remote_port).small()))
                .child(Checkbox::new("pf-save").label("Save").checked(self.save_forward).on_click(cx.listener(
                    |view, checked: &bool, _, cx| {
                        view.save_forward = *checked;
                        cx.notify();
                    },
                )))
                .child(Button::new("pf-add").small().primary().label("Add Port Forward").on_click(
                    cx.listener(|view, _, window, cx| view.add_forward(window, cx)),
                )),
        )
        .when_some(error, |d, error| d.child(div().text_sm().text_color(c.error).child(error)))
        .into_any_element()
    }
}

impl Render for SessionInfoView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, border) = (c.text_secondary, c.error, c.border);
        let id = self.connection(cx);
        let (name, state, remote) = match self.app.read(cx).session_slot(&id) {
            Some(slot) => (slot.name(), Some(slot.state.clone()), !slot.is_local()),
            None => ("Disconnected".to_owned(), None, false),
        };
        let (status, problem) = match &state {
            Some(ConnState::Connecting(_)) => ("connecting".to_owned(), None),
            Some(ConnState::Connected(_)) => ("connected".to_owned(), None),
            Some(ConnState::Reconnecting { attempt, error }) => (format!("reconnecting, attempt {attempt}"), Some(error.clone())),
            Some(ConnState::Failed { error, .. }) => ("error".to_owned(), Some(error.clone())),
            None => ("disconnected".to_owned(), None),
        };
        let reconnect_id = id.clone();
        let mut header = h_flex()
            .px_3()
            .py_2()
            .gap_2()
            .items_center()
            .border_b_1()
            .border_color(border)
            .child(div().text_lg().child(name))
            .child(div().text_sm().text_color(secondary).child(if remote { format!("({status})") } else { "(Local)".to_owned() }))
            .child(div().flex_1());
        if matches!(state, Some(ConnState::Connected(_))) {
            header = header.child(Button::new("force-reconnect").small().ghost().label("Reconnect").on_click(cx.listener(
                move |view, _, _, cx| {
                    if let Err(error) = view.app.read(cx).manager().force_reconnect(&reconnect_id) {
                        tracing::error!(%error, "reconnect");
                    }
                },
            )));
        }
        let tabs: Vec<(Tab, &str)> = if remote {
            vec![
                (Tab::Log(OutputKind::Bootstrap), "Bootstrap"),
                (Tab::Log(OutputKind::Tunnel), "Tunnel"),
                (Tab::Log(OutputKind::Daemon), "Daemon"),
                (Tab::PortForwards, "Port Forwards"),
            ]
        } else {
            vec![(Tab::Log(OutputKind::Daemon), "Daemon")]
        };
        let mut tab_bar = h_flex().px_2().py_1().gap_1().border_b_1().border_color(border);
        for (tab, label) in tabs {
            let button = Button::new(SharedString::from(format!("info-tab-{label}"))).xsmall().label(label);
            let button = if self.tab == tab { button.primary() } else { button.ghost() };
            tab_bar = tab_bar.child(button.on_click(cx.listener(move |view, _, _, cx| {
                view.tab = tab;
                cx.notify();
            })));
        }
        let body = match self.tab {
            Tab::Log(kind) => self.log_view(kind, cx),
            Tab::PortForwards => self.forwards_view(remote, cx),
        };
        v_flex()
            .flex_1()
            .h_full()
            .min_w_0()
            .pt(px(38.))
            .child(header)
            .when_some(problem, |d, problem| d.child(div().px_3().py_1().text_sm().text_color(error).child(problem)))
            .child(tab_bar)
            .child(div().flex_1().min_h_0().child(body))
    }
}

#[cfg(test)]
mod tests {
    use super::parse_forward;

    #[test]
    fn forwards_need_ports_and_a_host() {
        let spec = parse_forward(" 8080", "localhost ", "3000").unwrap();
        assert_eq!((spec.local_port, spec.remote_host.as_str(), spec.remote_port), (8080, "localhost", 3000));
        assert!(parse_forward("", "localhost", "3000").is_none());
        assert!(parse_forward("8080", "", "3000").is_none());
        assert!(parse_forward("8080", "localhost", "99999").is_none());
    }
}
