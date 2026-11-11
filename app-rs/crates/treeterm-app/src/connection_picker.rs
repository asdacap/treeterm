//! "SSH Connections": connect to a saved host or a new one (saved for next time). Port of
//! `ConnectionPicker.tsx`.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Div, Entity, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::connection::OutdatedDaemon;
use treeterm_core::settings::{ConnectionLabel, IdentityFile, PortForwardSpec, SshConnectionConfig};
use treeterm_model::app_store::AppStore;

use crate::ui;

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
    /// Enter connects (`handleKeyDown`); typing host or user re-renders Connect.
    _subscriptions: Vec<Subscription>,
}

impl ConnectionPicker {
    fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = |placeholder: &str, value: &str| {
            let placeholder = placeholder.to_owned();
            let value = value.to_owned();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let (host, user, port) = (input("hostname or IP", ""), input("username", ""), input("22", "22"));
        let (identity_file, label) = (input("~/.ssh/id_rsa", ""), input("display name", ""));
        let forward_local = input("local port", "");
        let forward_host = input("remote host", "localhost");
        let forward_remote = input("remote port", "");
        let _subscriptions = [&host, &user, &port, &identity_file, &label, &forward_local, &forward_host, &forward_remote]
            .into_iter()
            .map(|state| {
                cx.subscribe_in(state, window, |picker, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => {
                        if picker.connect_new(cx) {
                            window.close_dialog(cx);
                        }
                    }
                    InputEvent::Change => cx.notify(),
                    InputEvent::Focus | InputEvent::Blur => {}
                })
            })
            .collect();
        Self {
            host,
            user,
            port,
            identity_file,
            label,
            forward_local,
            forward_host,
            forward_remote,
            store,
            port_forwards: Vec::new(),
            refresh_daemon: false,
            allow_outdated: false,
            error: None,
            _subscriptions,
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

/// `ConnectionPicker.tsx`'s inline styles (Catppuccin colours, unlike the other dialogs).
mod look {
    pub const BG: u32 = 0x1e1e2e;
    pub const BORDER: u32 = 0x333333;
    pub const TEXT: u32 = 0xcdd6f4;
    pub const SUBTLE: u32 = 0xa6adc8;
    pub const FIELD: u32 = 0x313244;
    pub const FIELD_BORDER: u32 = 0x45475a;
    pub const DANGER: u32 = 0xf44336;
    pub const CONNECT: u32 = 0x89b4fa;
}

/// `inputStyle`: 6px 8px padding, #313244 on #45475a, 13px; no focus style.
fn field_input(state: &Entity<InputState>) -> Input {
    gpui_kit::Styled::h(Input::new(state), px(30.))
        .focus_bordered(false)
        .px(px(8.))
        .py(px(6.))
        .text_size(px(13.))
        .bg(rgb(look::FIELD))
        .border_color(rgb(look::FIELD_BORDER))
        .rounded(px(4.))
}

/// `labelStyle`: 12px, 4px above the input.
fn labelled(label: &'static str, input: Input) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .child(div().mb(px(4.)).text_size(px(12.)).line_height(px(14.)).text_color(rgb(look::SUBTLE)).child(label))
        .child(input)
}

/// `smallButtonStyle`: 2px 8px, transparent with a #45475a border, 12px.
fn small_button(id: impl Into<gpui_kit::ElementId>, label: &'static str, color: u32) -> gpui_kit::Stateful<Div> {
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .border_1()
        .border_color(rgb(look::FIELD_BORDER))
        .rounded(px(4.))
        .text_size(px(12.))
        .line_height(px(14.))
        .text_color(rgb(color))
        .cursor_pointer()
        .child(label)
}

/// The checkboxes: a 13px box, 8px gap, 13px label.
fn check_row(id: &'static str, checked: bool, disabled: bool, label: &'static str) -> gpui_kit::Stateful<Div> {
    div()
        .id(id)
        .mb(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(13.))
        .line_height(px(15.))
        .text_color(rgb(look::SUBTLE))
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| d.cursor_pointer())
        .child(ui::checkbox_box(checked, disabled, 13.))
        .child(label)
}

impl Render for ConnectionPicker {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let saved = self.saved(cx);
        let mut saved_list = div();
        for (i, config) in saved.iter().cloned().enumerate() {
            let forwards = config.port_forwards.len();
            let (connect, fill, remove) = (config.clone(), config.clone(), config.id.clone());
            saved_list = saved_list.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(4.))
                    .px(px(12.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .bg(rgb(look::FIELD))
                    .child(
                        div()
                            .flex()
                            .items_baseline()
                            .min_w_0()
                            .child(div().truncate().child(config.display_name()))
                            .when(forwards > 0, |d| {
                                d.child(div().ml(px(6.)).text_size(px(11.)).text_color(rgb(look::SUBTLE)).child(format!(
                                    "({forwards} port fwd{})",
                                    if forwards > 1 { "s" } else { "" }
                                )))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(4.))
                            .child(
                                small_button(SharedString::from(format!("fill-{i}")), "Fill", look::TEXT)
                                    .on_click(cx.listener(move |picker, _, window, cx| picker.fill(&fill, window, cx))),
                            )
                            .child(small_button(SharedString::from(format!("connect-{i}")), "Connect", look::TEXT).on_click(
                                cx.listener(move |picker, _, window, cx| {
                                    picker.connect_saved(connect.clone(), cx);
                                    window.close_dialog(cx);
                                }),
                            ))
                            .child(
                                small_button(SharedString::from(format!("remove-{i}")), "Remove", look::DANGER)
                                    .on_click(cx.listener(move |picker, _, _, cx| picker.remove_saved(remove.clone(), cx))),
                            ),
                    ),
            );
        }
        let heading = |text: &'static str| {
            div()
                .mb(px(8.))
                .text_size(px(14.))
                .line_height(px(17.))
                .font_weight(gpui_kit::FontWeight::BOLD)
                .text_color(rgb(look::SUBTLE))
                .child(text)
        };
        let mut forwards = div();
        for (i, forward) in self.port_forwards.iter().enumerate() {
            forwards = forwards.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .mb(px(4.))
                    .text_size(px(12.))
                    .child(div().flex_1().child(format!(
                        "localhost:{} → {}:{}",
                        forward.local_port, forward.remote_host, forward.remote_port
                    )))
                    .child(small_button(SharedString::from(format!("pf-remove-{i}")), "x", look::DANGER).px(px(6.)).py_0().on_click(
                        cx.listener(move |picker, _, _, cx| {
                            picker.port_forwards.remove(i);
                            cx.notify();
                        }),
                    )),
            );
        }
        let sep = |text: &'static str| div().text_size(px(12.)).text_color(rgb(look::SUBTLE)).child(text);
        let (refresh, allow) = (self.refresh_daemon, self.allow_outdated);
        let missing = self.host.read(cx).value().is_empty() || self.user.read(cx).value().is_empty();
        ui::dialog_root(cx)
            .p(px(24.))
            .text_color(rgb(look::TEXT))
            .text_size(px(16.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .mb(px(16.))
                    .child(
                        div()
                            .text_size(px(18.))
                            .line_height(px(22.))
                            .font_weight(gpui_kit::FontWeight::BOLD)
                            .child("SSH Connections"),
                    )
                    .child(
                        div()
                            .id("ssh-close")
                            .px(px(4.))
                            .text_size(px(18.))
                            .line_height(px(21.))
                            .text_color(rgb(0x666666))
                            .cursor_pointer()
                            .child("x")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .when(!saved.is_empty(), |d| d.child(div().mb(px(16.)).child(heading("Saved Connections")).child(saved_list)))
            .child(heading("New Connection"))
            // A two-column grid, 8px gaps.
            .child(
                div()
                    .mb(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(labelled("User", field_input(&self.user)))
                            .child(labelled("Host", field_input(&self.host))),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(labelled("Port", field_input(&self.port)))
                            .child(labelled("Label (optional)", field_input(&self.label))),
                    )
                    .child(labelled("Identity File (optional)", field_input(&self.identity_file))),
            )
            .child(
                div()
                    .mb(px(12.))
                    .child(div().mb(px(4.)).text_size(px(12.)).line_height(px(14.)).text_color(rgb(look::SUBTLE)).child("Port Forwards"))
                    .child(forwards)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(div().w(px(80.)).child(field_input(&self.forward_local)))
                            .child(sep("→"))
                            .child(div().w(px(120.)).child(field_input(&self.forward_host)))
                            .child(sep(":"))
                            .child(div().w(px(80.)).child(field_input(&self.forward_remote)))
                            .child(small_button("pf-add", "Add", look::TEXT).on_click(
                                cx.listener(|picker, _, window, cx| picker.add_forward(window, cx)),
                            )),
                    ),
            )
            .child(check_row("refresh-daemon", refresh, false, "Refresh remote daemon").on_click(cx.listener(
                move |picker, _, _, cx| {
                    picker.refresh_daemon = !refresh;
                    cx.notify();
                },
            )))
            .child(check_row("allow-outdated", allow, refresh, "Allow outdated daemon").when(!refresh, |d| {
                d.on_click(cx.listener(move |picker, _, _, cx| {
                    picker.allow_outdated = !allow;
                    cx.notify();
                }))
            }))
            .when_some(self.error.clone(), |d, message| {
                d.child(div().mb(px(8.)).text_size(px(13.)).text_color(rgb(look::DANGER)).child(message))
            })
            .child(
                div()
                    .id("ssh-connect")
                    .flex()
                    .justify_center()
                    .px(px(16.))
                    .py(px(8.))
                    .rounded(px(4.))
                    // `opacity: 0.5` fades the button as one layer: the fill over the dialog,
                    // the text unchanged.
                    .bg(rgb(if missing { 0x54_69_94 } else { look::CONNECT }))
                    .text_color(rgb(look::BG))
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .when(!missing, |d| d.cursor_pointer())
                    .child("Connect")
                    .when(!missing, |d| {
                        d.on_click(cx.listener(|picker, _, window, cx| {
                            if picker.connect_new(cx) {
                                window.close_dialog(cx);
                            }
                        }))
                    }),
            )
    }
}

pub fn open(store: Entity<AppStore>, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| ConnectionPicker::new(store, window, cx));
    // `autoFocus` on User, once the dialog has taken the focus.
    let user = view.read(cx).user.clone();
    window.defer(cx, move |window, cx| user.update(cx, |state, cx| state.focus(window, cx)));
    window.open_dialog(cx, move |dialog, _window, cx| {
        ui::dialog_frame(dialog, 500., cx)
            .bg(rgb(look::BG))
            .border_1()
            .border_color(rgb(look::BORDER))
            .child(view.clone())
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
