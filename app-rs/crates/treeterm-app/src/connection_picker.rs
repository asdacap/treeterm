//! "SSH Connections": connect to a saved host or a new one (saved for next time). Port of
//! `ConnectionPicker.tsx`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Window, div, px};
use treeterm_core::connection::OutdatedDaemon;
use treeterm_core::settings::{ConnectionLabel, IdentityFile, PortForwardSpec, SshConnectionConfig};
use treeterm_model::app_store::AppStore;

use crate::theme::colors;

/// What the form holds, as typed.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct Fields {
    host: String,
    user: String,
    port: String,
    identity_file: String,
    label: String,
}

/// A connection from the form (`handleConnect`): host and user required, port defaults to
/// 22, the id is new.
fn build_config(fields: &Fields, port_forwards: Vec<PortForwardSpec>, now_ms: u128) -> Result<SshConnectionConfig, String> {
    let (host, user) = (fields.host.trim(), fields.user.trim());
    if host.is_empty() || user.is_empty() {
        return Err("Host and user are required".to_owned());
    }
    let port = match fields.port.trim() {
        "" => 22,
        text => text.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("Invalid port “{text}”"))?,
    };
    let identity = fields.identity_file.trim();
    let label = fields.label.trim();
    Ok(SshConnectionConfig {
        id: format!("ssh-{host}-{now_ms}"),
        host: host.to_owned(),
        user: user.to_owned(),
        port,
        identity_file: if identity.is_empty() { IdentityFile::SshDefault } else { IdentityFile::Path(identity.to_owned()) },
        label: if label.is_empty() { ConnectionLabel::UserAtHost } else { ConnectionLabel::Custom(label.to_owned()) },
        port_forwards,
        extra: Default::default(),
    })
}

/// A port forward from the small form; ports 1–65535 and a remote host are required.
fn build_forward(local: &str, remote_host: &str, remote_port: &str) -> Option<PortForwardSpec> {
    let port = |text: &str| text.trim().parse::<u16>().ok().filter(|p| *p > 0);
    let remote_host = remote_host.trim();
    if remote_host.is_empty() {
        return None;
    }
    Some(PortForwardSpec {
        local_port: port(local)?,
        remote_host: remote_host.to_owned(),
        remote_port: port(remote_port)?,
        extra: Default::default(),
    })
}

pub struct ConnectionPicker {
    store: Entity<AppStore>,
    host: Entity<InputState>,
    user: Entity<InputState>,
    port: Entity<InputState>,
    identity_file: Entity<InputState>,
    label: Entity<InputState>,
    forward_local: Entity<InputState>,
    forward_host: Entity<InputState>,
    forward_remote: Entity<InputState>,
    port_forwards: Vec<PortForwardSpec>,
    refresh_daemon: bool,
    allow_outdated: bool,
    error: Option<SharedString>,
}

impl ConnectionPicker {
    fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = |placeholder: &str, value: &str| {
            let placeholder = placeholder.to_owned();
            let value = value.to_owned();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        Self {
            host: input("example.com", ""),
            user: input("user", ""),
            port: input("22", "22"),
            identity_file: input("~/.ssh/id_ed25519 (optional)", ""),
            label: input("Label (optional)", ""),
            forward_local: input("Local port", ""),
            forward_host: input("Remote host", "localhost"),
            forward_remote: input("Remote port", ""),
            store,
            port_forwards: Vec::new(),
            refresh_daemon: false,
            allow_outdated: false,
            error: None,
        }
    }

    fn outdated(&self) -> OutdatedDaemon {
        match (self.refresh_daemon, self.allow_outdated) {
            (true, _) => OutdatedDaemon::Refresh,
            (false, true) => OutdatedDaemon::Accept,
            (false, false) => OutdatedDaemon::Report,
        }
    }

    fn saved(&self, cx: &App) -> Vec<SshConnectionConfig> {
        self.store.read(cx).settings().read(cx).settings().ssh.saved_connections.clone()
    }

    fn connect_saved(&mut self, config: SshConnectionConfig, cx: &mut Context<Self>) {
        let outdated = self.outdated();
        self.store.update(cx, |s, cx| s.connect_remote(config, outdated, cx));
    }

    /// Validate the form, save the connection (`saveConnection`) and connect. Returns
    /// whether the dialog may close.
    fn connect_new(&mut self, cx: &mut Context<Self>) -> bool {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().to_string();
        let fields = Fields {
            host: text(&self.host, cx),
            user: text(&self.user, cx),
            port: text(&self.port, cx),
            identity_file: text(&self.identity_file, cx),
            label: text(&self.label, cx),
        };
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let config = match build_config(&fields, self.port_forwards.clone(), now_ms) {
            Ok(config) => config,
            Err(message) => {
                self.error = Some(message.into());
                cx.notify();
                return false;
            }
        };
        let settings = self.store.read(cx).settings().clone();
        let saved = config.clone();
        settings.update(cx, |store, cx| {
            store.update(
                move |s| {
                    s.ssh.saved_connections.retain(|c| c.id != saved.id);
                    s.ssh.saved_connections.push(saved);
                },
                cx,
            )
        });
        self.connect_saved(config, cx);
        true
    }

    fn remove_saved(&mut self, id: String, cx: &mut Context<Self>) {
        let settings = self.store.read(cx).settings().clone();
        settings.update(cx, |store, cx| store.update(move |s| s.ssh.saved_connections.retain(|c| c.id != id), cx));
        cx.notify();
    }

    /// "Fill": copy a saved connection into the form.
    fn fill(&mut self, config: &SshConnectionConfig, window: &mut Window, cx: &mut Context<Self>) {
        let set = |state: &Entity<InputState>, value: String, window: &mut Window, cx: &mut Context<Self>| {
            state.update(cx, |input, cx| input.set_value(value, window, cx));
        };
        set(&self.host, config.host.clone(), window, cx);
        set(&self.user, config.user.clone(), window, cx);
        set(&self.port, config.port.to_string(), window, cx);
        let identity = match &config.identity_file {
            IdentityFile::SshDefault => String::new(),
            IdentityFile::Path(path) => path.clone(),
        };
        set(&self.identity_file, identity, window, cx);
        let label = match &config.label {
            ConnectionLabel::UserAtHost => String::new(),
            ConnectionLabel::Custom(label) => label.clone(),
        };
        set(&self.label, label, window, cx);
        self.port_forwards = config.port_forwards.clone();
        cx.notify();
    }

    fn add_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().to_string();
        let Some(forward) = build_forward(
            &text(&self.forward_local, cx),
            &text(&self.forward_host, cx),
            &text(&self.forward_remote, cx),
        ) else {
            return;
        };
        self.port_forwards.push(forward);
        self.forward_local.update(cx, |input, cx| input.set_value("", window, cx));
        self.forward_remote.update(cx, |input, cx| input.set_value("", window, cx));
        cx.notify();
    }
}

impl Render for ConnectionPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error, row_bg) = (c.text_secondary, c.error, c.bg_tertiary);
        let heading = move |text: &'static str| div().text_sm().text_color(secondary).child(text);
        let mut saved_list = v_flex().gap_1();
        for (i, config) in self.saved(cx).into_iter().enumerate() {
            let forwards = config.port_forwards.len();
            let (connect, fill, remove) = (config.clone(), config.clone(), config.id.clone());
            saved_list = saved_list.child(
                h_flex()
                    .px_3()
                    .py_1()
                    .gap_2()
                    .rounded_sm()
                    .bg(row_bg)
                    .items_center()
                    .child(div().flex_1().truncate().child(config.display_name()))
                    .when(forwards > 0, |d| {
                        d.child(div().text_xs().text_color(secondary).child(format!(
                            "({forwards} port fwd{})",
                            if forwards > 1 { "s" } else { "" }
                        )))
                    })
                    .child(Button::new(SharedString::from(format!("fill-{i}"))).xsmall().ghost().label("Fill").on_click(
                        cx.listener(move |picker, _, window, cx| picker.fill(&fill, window, cx)),
                    ))
                    .child(
                        Button::new(SharedString::from(format!("connect-{i}")))
                            .xsmall()
                            .primary()
                            .label("Connect")
                            .on_click(cx.listener(move |picker, _, window, cx| {
                                picker.connect_saved(connect.clone(), cx);
                                window.close_dialog(cx);
                            })),
                    )
                    .child(Button::new(SharedString::from(format!("remove-{i}"))).xsmall().ghost().label("×").on_click(
                        cx.listener(move |picker, _, _, cx| picker.remove_saved(remove.clone(), cx)),
                    )),
            );
        }
        let has_saved = !self.saved(cx).is_empty();
        let mut forwards = v_flex().gap_1();
        for (i, forward) in self.port_forwards.iter().enumerate() {
            forwards = forwards.child(
                h_flex()
                    .gap_2()
                    .text_sm()
                    .child(format!("localhost:{} → {}:{}", forward.local_port, forward.remote_host, forward.remote_port))
                    .child(Button::new(SharedString::from(format!("pf-remove-{i}"))).xsmall().ghost().label("×").on_click(
                        cx.listener(move |picker, _, _, cx| {
                            picker.port_forwards.remove(i);
                            cx.notify();
                        }),
                    )),
            );
        }
        v_flex()
            .gap_3()
            .when(has_saved, |d| d.child(v_flex().gap_1().child(heading("Saved Connections")).child(saved_list)))
            .child(heading("New Connection"))
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_1().child(Input::new(&self.user).small()))
                    .child("@")
                    .child(div().flex_1().child(Input::new(&self.host).small()))
                    .child(div().w(px(70.)).child(Input::new(&self.port).small())),
            )
            .child(Input::new(&self.identity_file).small())
            .child(Input::new(&self.label).small())
            .child(heading("Port Forwards"))
            .child(forwards)
            .child(
                h_flex()
                    .gap_2()
                    .child(div().w(px(90.)).child(Input::new(&self.forward_local).small()))
                    .child("→")
                    .child(div().flex_1().child(Input::new(&self.forward_host).small()))
                    .child(div().w(px(90.)).child(Input::new(&self.forward_remote).small()))
                    .child(Button::new("pf-add").xsmall().ghost().label("Add").on_click(
                        cx.listener(|picker, _, window, cx| picker.add_forward(window, cx)),
                    )),
            )
            .child(
                h_flex()
                    .gap_4()
                    .child(Checkbox::new("refresh-daemon").label("Refresh remote daemon").checked(self.refresh_daemon).on_click(
                        cx.listener(|picker, checked: &bool, _, cx| {
                            picker.refresh_daemon = *checked;
                            cx.notify();
                        }),
                    ))
                    .child(Checkbox::new("allow-outdated").label("Allow outdated daemon").checked(self.allow_outdated).on_click(
                        cx.listener(|picker, checked: &bool, _, cx| {
                            picker.allow_outdated = *checked;
                            cx.notify();
                        }),
                    )),
            )
            .when_some(self.error.clone(), |d, message| d.child(div().text_sm().text_color(error).child(message)))
    }
}

pub fn open(store: Entity<AppStore>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ConnectionPicker::new(store, window, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let connect = view.clone();
        dialog
            .title("SSH Connections")
            .w(px(560.))
            .child(view.clone())
            .footer(h_flex().justify_end().child(Button::new("ssh-connect").primary().label("Connect").on_click(
                move |_, window, cx| {
                    if connect.update(cx, |picker, cx| picker.connect_new(cx)) {
                        window.close_dialog(cx);
                    }
                },
            )))
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(host: &str, user: &str, port: &str) -> Fields {
        Fields { host: host.into(), user: user.into(), port: port.into(), ..Fields::default() }
    }

    #[test]
    fn host_and_user_are_required() {
        assert_eq!(build_config(&fields("", "me", "22"), vec![], 1).unwrap_err(), "Host and user are required");
        assert_eq!(build_config(&fields("box", " ", "22"), vec![], 1).unwrap_err(), "Host and user are required");
    }

    #[test]
    fn a_new_connection_gets_defaults_and_a_fresh_id() {
        let config = build_config(&fields(" box.lan ", "me", ""), vec![], 42).unwrap();
        assert_eq!(config.id, "ssh-box.lan-42");
        assert_eq!(config.port, 22);
        assert_eq!(config.identity_file, IdentityFile::SshDefault);
        assert_eq!(config.label, ConnectionLabel::UserAtHost);
        assert_eq!(config.display_name(), "me@box.lan");
        assert!(build_config(&fields("box", "me", "70000"), vec![], 1).is_err());
    }

    #[test]
    fn port_forwards_need_valid_ports_and_a_host() {
        assert_eq!(build_forward("8080", "localhost", "3000").map(|f| (f.local_port, f.remote_port)), Some((8080, 3000)));
        assert!(build_forward("0", "localhost", "3000").is_none());
        assert!(build_forward("8080", " ", "3000").is_none());
        assert!(build_forward("8080", "localhost", "x").is_none());
    }
}
