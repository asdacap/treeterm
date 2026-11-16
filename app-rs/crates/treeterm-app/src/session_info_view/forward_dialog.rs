//! "Add Port Forward" (`PortForwardDialog.tsx`): local port, remote host and port; the
//! forward is saved with the connection (`persist: true`). The ports are
//! `<input type="number" min=1 max=65535>`.

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, EventEmitter, FontWeight, Subscription, Window, div, px, rgb, rgba};
use treeterm_model::session_info::{SessionInfo, parse_forward_fields};

use crate::number_field::{NumberField, NumberSpec, number_value};
use crate::theme::colors;
use crate::ui::{DialogButton, FieldStyle, pane_dialog_button, dialog_close, dialog_shadow, field, normal_line};

/// The port fields' `min=1 max=65535` (and the default `step`, 1).
const PORT: NumberSpec = NumberSpec { min: 1., max: 65535., step: 1. };

/// What a port field reports as its value (`e.target.value`): its text if that is a valid
/// number, otherwise nothing (`-`, `5.`).
fn port_value(text: &str) -> &str {
    match number_value(text) {
        Some(_) => text,
        None => "",
    }
}

/// The dialog is done (cancelled or the forward started).
pub struct Closed;

impl EventEmitter<Closed> for ForwardDialog {}

pub struct ForwardDialog {
    info: Entity<SessionInfo>,
    local_port: Entity<InputState>,
    remote_host: Entity<InputState>,
    remote_port: Entity<InputState>,
    error: Option<String>,
    _subscriptions: Vec<Subscription>,
}

impl ForwardDialog {
    pub fn new(info: Entity<SessionInfo>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut input = |placeholder: &str, value: &str| {
            let (placeholder, value) = (placeholder.to_owned(), value.to_owned());
            cx.new(|cx| InputState::new(window, cx).placeholder(placeholder).default_value(value))
        };
        let (local_port, remote_host, remote_port) = (input("8080", ""), input("localhost", "localhost"), input("3000", ""));
        let mut subscriptions = Vec::new();
        for state in [&local_port, &remote_host, &remote_port] {
            subscriptions.push(cx.subscribe(state, |dialog, _, event: &InputEvent, cx| match event {
                InputEvent::PressEnter { .. } => dialog.submit(cx),
                // The hint under the fields repeats them.
                InputEvent::Change => cx.notify(),
                InputEvent::Focus | InputEvent::Blur => {}
            }));
        }
        local_port.update(cx, |input, cx| input.focus(window, cx));
        Self { info, local_port, remote_host, remote_port, error: None, _subscriptions: subscriptions }
    }

    pub fn close(&mut self, cx: &mut Context<Self>) {
        cx.emit(Closed);
    }

    fn text(state: &Entity<InputState>, cx: &gpui_kit::App) -> String {
        state.read(cx).value().to_string()
    }

    /// A port field's value ([`port_value`]).
    fn port(state: &Entity<InputState>, cx: &gpui_kit::App) -> String {
        port_value(&state.read(cx).value()).to_owned()
    }

    fn submit(&mut self, cx: &mut Context<Self>) {
        let (local, host, remote) =
            (Self::port(&self.local_port, cx), Self::text(&self.remote_host, cx), Self::port(&self.remote_port, cx));
        let spec = match parse_forward_fields(&local, &host, &remote) {
            Ok(spec) => spec,
            Err(error) => {
                self.error = Some(error.message().to_owned());
                cx.notify();
                return;
            }
        };
        self.info.update(cx, |info, cx| info.add_forward(spec, true, cx));
        match self.info.read(cx).error() {
            Some(error) => {
                self.error = Some(error.to_owned());
                cx.notify();
            }
            None => cx.emit(Closed),
        }
    }
}

impl Render for ForwardDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border) = (c.text_secondary, c.text_primary, c.border);
        // `.port-forward-dialog-row input`.
        let style = FieldStyle {
            bg: c.bg_primary,
            border,
            radius: 4.,
            pad: (6., 10.),
            size: 13.,
            mono: false,
            height: 30.,
        };
        let row = |label: &'static str, input: gpui_kit::AnyElement| {
            v_flex()
                .gap(px(4.))
                .child(div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).text_color(secondary).child(label))
                .child(input)
        };
        let port = |state: &Entity<InputState>| NumberField::field(state, PORT, style).into_any_element();
        let or_unknown = |text: String| if text.is_empty() { "?".to_owned() } else { text };
        let (local, host, remote) = (
            or_unknown(Self::port(&self.local_port, cx)),
            or_unknown(Self::text(&self.remote_host, cx)),
            or_unknown(Self::port(&self.remote_port, cx)),
        );
        // "Forwards <strong>localhost:L</strong> on this machine to <strong>H:R</strong> via the
        // server.", wrapping as one paragraph.
        let hint = {
            let parts = [
                ("Forwards ".to_owned(), false),
                (format!("localhost:{local}"), true),
                (" on this machine to ".to_owned(), false),
                (format!("{host}:{remote}"), true),
                (" via the server.".to_owned(), false),
            ];
            let mut text = String::new();
            let mut runs = Vec::new();
            for (part, bold) in parts {
                let start = text.len();
                text.push_str(&part);
                if bold {
                    let style = gpui_kit::HighlightStyle { font_weight: Some(FontWeight::BOLD), ..Default::default() };
                    runs.push((start..text.len(), style));
                }
            }
            gpui_kit::StyledText::new(text).with_highlights(runs)
        };
        v_flex()
            .id("port-forward-dialog")
            .occlude()
            .w(px(420.))
            .rounded(px(8.))
            .border_1()
            .border_color(border)
            .bg(c.bg_secondary)
            .shadow(dialog_shadow())
            .text_color(primary)
            .capture_key_down(cx.listener(|dialog, event: &gpui_kit::KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    cx.stop_propagation();
                    dialog.close(cx);
                }
            }))
            .child(
                h_flex()
                    .justify_between()
                    .px(px(20.))
                    .pt(px(16.))
                    .pb(px(12.))
                    .border_b_1()
                    .border_color(border)
                    .child(div().text_size(px(16.)).font_weight(FontWeight::SEMIBOLD).child("Add Port Forward"))
                    .child(dialog_close("pf-dialog-close", "✕", cx).on_click(cx.listener(|d, _, _, cx| d.close(cx)))),
            )
            .child(
                v_flex()
                    .px(px(20.))
                    .py(px(16.))
                    .gap(px(12.))
                    .child(row("Local Port", port(&self.local_port)))
                    .child(row("Remote Host", field(&self.remote_host, style, window, cx).into_any_element()))
                    .child(row("Remote Port", port(&self.remote_port)))
                    .child(
                        div()
                            .text_size(px(12.))
                            .line_height(normal_line(12.))
                            .text_color(secondary)
                            .child(hint),
                    )
                    .when_some(self.error.clone(), |d, error| {
                        d.child(
                            div()
                                .px(px(8.))
                                .py(px(6.))
                                .rounded(px(4.))
                                .bg(rgba(0xf443361a))
                                .text_size(px(12.))
                                .text_color(rgb(0xf44336))
                                .child(error),
                        )
                    }),
            )
            .child(
                h_flex()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .pt(px(12.))
                    .pb(px(16.))
                    .border_t_1()
                    .border_color(border)
                    .child(
                        pane_dialog_button("pf-cancel", "Cancel", DialogButton::Secondary, false, cx)
                            .on_click(cx.listener(|d, _, _, cx| d.close(cx))),
                    )
                    .child(
                        pane_dialog_button("pf-start", "Start Forward", DialogButton::Primary, false, cx)
                            .on_click(cx.listener(|d, _, _, cx| d.submit(cx))),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::InputState;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    use super::super::tests::{Session, dialog, draw, open_dialog, pane};
    use super::{ForwardDialog, port_value};

    #[test]
    fn a_port_field_is_empty_unless_it_holds_a_number() {
        for text in ["8080", "1.5", "1e3", "-5"] {
            assert_eq!(port_value(text), text);
        }
        for text in ["", "-", "5.", "."] {
            assert_eq!(port_value(text), "", "{text:?}");
        }
    }

    fn text(field: &Entity<InputState>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| field.read(cx).value().to_string())
    }

    /// `field` focused, holding `value`.
    fn focus(field: &Entity<InputState>, value: &str, cx: &mut VisualTestContext) {
        let value = value.to_owned();
        cx.update(|window, cx| {
            field.update(cx, |input, cx| {
                input.set_value(value, window, cx);
                input.focus(window, cx);
            })
        });
        draw(cx);
    }

    /// The open dialog of a remote session's pane.
    fn open<'a>(cx: &'a mut TestAppContext, dir: &std::path::Path) -> (Entity<ForwardDialog>, &'a mut VisualTestContext) {
        let (view, _, cx) = pane(cx, dir, Session::Remote);
        open_dialog(&view, cx);
        (dialog(&view, cx).expect("open"), cx)
    }

    /// `<input type="number" min=1 max=65535>`: the ports take only numbers (the remote host
    /// takes anything), and ArrowUp/ArrowDown step them by 1 within 1–65535.
    #[gpui_kit::test]
    fn the_ports_are_number_fields(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("temp dir");
        let (dialog, cx) = open(cx, dir.path());
        let (local, host, remote) = cx.update(|_, cx| {
            let d = dialog.read(cx);
            (d.local_port.clone(), d.remote_host.clone(), d.remote_port.clone())
        });

        // The local port is focused on open.
        cx.simulate_input("8a0x");
        assert_eq!(text(&local, cx), "80");
        cx.simulate_keystrokes("up up");
        assert_eq!(text(&local, cx), "82");
        cx.simulate_keystrokes("down");
        assert_eq!(text(&local, cx), "81");

        focus(&remote, "", cx);
        cx.simulate_keystrokes("up");
        assert_eq!(text(&remote, cx), "1", "an empty field steps to min");
        cx.simulate_keystrokes("down");
        assert_eq!(text(&remote, cx), "1");
        focus(&remote, "65535", cx);
        cx.simulate_keystrokes("up");
        assert_eq!(text(&remote, cx), "65535");
        focus(&remote, "70000", cx);
        cx.simulate_keystrokes("down");
        assert_eq!(text(&remote, cx), "65535", "back into range");

        focus(&host, "", cx);
        cx.simulate_input("db-1");
        assert_eq!(text(&host, cx), "db-1");
    }

    /// A port holding no number (`5.`) is empty to Start Forward, as `e.target.value` is.
    #[gpui_kit::test]
    fn a_port_holding_no_number_is_refused(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().expect("temp dir");
        let (dialog, cx) = open(cx, dir.path());
        let (local, remote) = cx.update(|_, cx| (dialog.read(cx).local_port.clone(), dialog.read(cx).remote_port.clone()));
        focus(&remote, "3000", cx);
        focus(&local, "5.", cx);
        cx.update(|_, cx| dialog.update(cx, |dialog, cx| dialog.submit(cx)));
        let error = cx.update(|_, cx| dialog.read(cx).error.clone());
        assert_eq!(error.as_deref(), Some("Local port must be a number between 1 and 65535"));
    }
}
