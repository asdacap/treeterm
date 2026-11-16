//! "SSH Connections": connect to a saved host or a new one (saved for next time). Port of
//! `ConnectionPicker.tsx`, which stays mounted while closed: each window keeps one picker,
//! so what was typed, the port forwards, both options and the error are there on reopening.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, Context, Div, ElementId, Entity, SharedString, Subscription, Window, div, px, rgb};
use treeterm_core::settings::SshConnectionConfig;
use treeterm_model::app_store::AppStore;
use treeterm_model::ssh_connection_form::{ConnectionFields, ConnectionForm};

use crate::ui;

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
    form: ConnectionForm,
    /// Enter connects (`handleKeyDown`); typing host or user re-renders Connect.
    _subscriptions: Vec<Subscription>,
}

impl ConnectionPicker {
    pub fn new(store: Entity<AppStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let blank = ConnectionFields::default();
        let mut input = |placeholder: &str, value: &str| {
            let placeholder = placeholder.to_owned();
            let value = value.to_owned();
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let (host, user, port) =
            (input("hostname or IP", &blank.host), input("username", &blank.user), input("22", &blank.port));
        let (identity_file, label) = (input("~/.ssh/id_rsa", &blank.identity_file), input("display name", &blank.label));
        let forward_local = input("local port", "");
        let forward_host = input("remote host", "localhost");
        let forward_remote = input("remote port", "");
        let _subscriptions = [&host, &user, &port, &identity_file, &label, &forward_local, &forward_host, &forward_remote]
            .into_iter()
            .map(|state| {
                cx.subscribe_in(state, window, |picker, _, event: &InputEvent, window, cx| match event {
                    InputEvent::PressEnter { .. } => picker.connect_new(window, cx),
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
            form: ConnectionForm::default(),
            _subscriptions,
        }
    }

    fn saved(&self, cx: &App) -> Vec<SshConnectionConfig> {
        self.store.read(cx).settings().read(cx).settings().ssh.saved_connections.clone()
    }

    /// `connectAndCreateSession`: close, and the window shows the session pane while it
    /// connects.
    fn connect(&mut self, config: SshConnectionConfig, window: &mut Window, cx: &mut Context<Self>) {
        let outdated = self.form.outdated();
        self.store.update(cx, |s, cx| s.start_remote_connect(config, outdated, cx));
        window.close_dialog(cx);
        cx.notify();
    }

    /// `handleConnectSaved`.
    fn connect_saved(&mut self, config: SshConnectionConfig, window: &mut Window, cx: &mut Context<Self>) {
        self.form.connect_saved();
        self.connect(config, window, cx);
    }

    /// `handleConnect`: validate the form, connect, save the connection (`saveConnection`)
    /// and empty the form; or show why not.
    fn connect_new(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().to_string();
        let fields = ConnectionFields {
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
        let Some(config) = self.form.submit(&fields, now_ms) else {
            cx.notify();
            return;
        };
        self.connect(config.clone(), window, cx);
        let settings = self.store.read(cx).settings().clone();
        settings.update(cx, |store, cx| store.save_ssh_connection(config, cx));
        self.set_fields(&ConnectionFields::default(), window, cx);
    }

    fn set_fields(&self, fields: &ConnectionFields, window: &mut Window, cx: &mut Context<Self>) {
        let inputs = [
            (&self.host, &fields.host),
            (&self.user, &fields.user),
            (&self.port, &fields.port),
            (&self.identity_file, &fields.identity_file),
            (&self.label, &fields.label),
        ];
        for (state, value) in inputs {
            state.update(cx, |input, cx| input.set_value(value.clone(), window, cx));
        }
    }

    fn remove_saved(&mut self, id: String, cx: &mut Context<Self>) {
        let settings = self.store.read(cx).settings().clone();
        settings.update(cx, |store, cx| store.update(move |s| s.ssh.saved_connections.retain(|c| c.id != id), cx));
        cx.notify();
    }

    /// "Fill": copy a saved connection into the form.
    fn fill(&mut self, config: &SshConnectionConfig, window: &mut Window, cx: &mut Context<Self>) {
        let fields = self.form.fill(config);
        self.set_fields(&fields, window, cx);
        cx.notify();
    }

    /// "Add": the forward inputs go back to empty, `localhost` and empty.
    fn add_forward(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let text = |state: &Entity<InputState>, cx: &App| state.read(cx).value().to_string();
        let (local, host, remote) = (text(&self.forward_local, cx), text(&self.forward_host, cx), text(&self.forward_remote, cx));
        if !self.form.add_forward(&local, &host, &remote) {
            return;
        }
        self.forward_local.update(cx, |input, cx| input.set_value("", window, cx));
        self.forward_host.update(cx, |input, cx| input.set_value("localhost", window, cx));
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
fn small_button(
    id: impl Into<ElementId>,
    label: &'static str,
    color: u32,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Button {
    ui::key_button(id, 4., window, cx)
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .border_1()
        .border_color(rgb(look::FIELD_BORDER))
        .text_size(px(12.))
        .line_height(px(14.))
        .text_color(rgb(color))
        .cursor_pointer()
        .child(label)
}

/// The checkboxes: a 13px box, 8px gap, 13px label.
fn check_row(
    id: &'static str,
    checked: bool,
    disabled: bool,
    label: &'static str,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Checkbox {
    ui::key_checkbox(id, checked, disabled, 13., window, cx)
        .mb(px(8.))
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(13.))
        .line_height(px(15.))
        .text_color(rgb(look::SUBTLE))
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| d.cursor_pointer())
        .child(label)
}

impl Render for ConnectionPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                        // The name wraps, as Electron's row lets it.
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .items_baseline()
                            .child(div().min_w_0().child(config.display_name()))
                            .when(forwards > 0, |d| {
                                d.child(div().flex_shrink_0().ml(px(6.)).text_size(px(11.)).text_color(rgb(look::SUBTLE)).child(
                                    format!("({forwards} port fwd{})", if forwards > 1 { "s" } else { "" }),
                                ))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_shrink_0()
                            .gap(px(4.))
                            .child(
                                small_button(SharedString::from(format!("fill-{i}")), "Fill", look::TEXT, window, cx)
                                    .on_click(cx.listener(move |picker, _, window, cx| picker.fill(&fill, window, cx))),
                            )
                            .child(
                                small_button(SharedString::from(format!("connect-{i}")), "Connect", look::TEXT, window, cx)
                                    .on_click(cx.listener(move |picker, _, window, cx| {
                                        picker.connect_saved(connect.clone(), window, cx)
                                    })),
                            )
                            .child(
                                small_button(SharedString::from(format!("remove-{i}")), "Remove", look::DANGER, window, cx)
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
        for (i, forward) in self.form.port_forwards().iter().enumerate() {
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
                    .child(
                        small_button(SharedString::from(format!("pf-remove-{i}")), "x", look::DANGER, window, cx)
                            .px(px(6.))
                            .py_0()
                            .on_click(cx.listener(move |picker, _, _, cx| {
                                picker.form.remove_forward(i);
                                cx.notify();
                            })),
                    ),
            );
        }
        let sep = |text: &'static str| div().text_size(px(12.)).text_color(rgb(look::SUBTLE)).child(text);
        let (refresh, allow) = (self.form.refresh_daemon(), self.form.allow_outdated());
        let missing = self.host.read(cx).value().is_empty() || self.user.read(cx).value().is_empty();
        // Space toggles a focused checkbox; Enter connects, as anywhere in Electron's dialog.
        let toggle_refresh = cx.listener(|picker, event: &ClickEvent, window, cx| {
            if ui::pressed_enter(event) {
                return picker.connect_new(window, cx);
            }
            picker.form.toggle_refresh_daemon();
            cx.notify();
        });
        let toggle_allow = cx.listener(|picker, event: &ClickEvent, window, cx| {
            if ui::pressed_enter(event) {
                return picker.connect_new(window, cx);
            }
            picker.form.toggle_allow_outdated();
            cx.notify();
        });
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
                        ui::key_button("ssh-close", 0., window, cx)
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
                            .child(
                                small_button("pf-add", "Add", look::TEXT, window, cx)
                                    .on_click(cx.listener(|picker, _, window, cx| picker.add_forward(window, cx))),
                            ),
                    ),
            )
            .child(
                check_row("refresh-daemon", refresh, false, "Refresh remote daemon", window, cx)
                    .on_change(move |_, event, window, cx| toggle_refresh(event, window, cx)),
            )
            .child(
                check_row("allow-outdated", allow, refresh, "Allow outdated daemon", window, cx)
                    .on_change(move |_, event, window, cx| toggle_allow(event, window, cx)),
            )
            .when_some(self.form.error().map(SharedString::from), |d, message| {
                d.child(div().mb(px(8.)).text_size(px(13.)).text_color(rgb(look::DANGER)).child(message))
            })
            .child(
                ui::key_button("ssh-connect", 4., window, cx)
                    .disabled(missing)
                    .w_full()
                    .px(px(16.))
                    .py(px(8.))
                    // `opacity: 0.5` fades the button as one layer: the fill over the dialog,
                    // the text unchanged.
                    .bg(rgb(if missing { 0x54_69_94 } else { look::CONNECT }))
                    .text_color(rgb(look::BG))
                    .text_size(px(14.))
                    .line_height(px(16.))
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .when(!missing, |d| d.cursor_pointer())
                    .child("Connect")
                    .on_click(cx.listener(|picker, _, window, cx| picker.connect_new(window, cx))),
            )
    }
}

/// Show the window's picker as it was left, User focused (`autoFocus`, as its inputs mount
/// again in Electron).
pub fn open(view: Entity<ConnectionPicker>, window: &mut Window, cx: &mut App) {
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
