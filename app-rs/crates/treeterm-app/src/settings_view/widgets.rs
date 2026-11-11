//! Building blocks shared by the sections, styled after Electron's `settings-*` classes.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Div, SharedString, div};
use treeterm_core::settings::Settings;

use super::SettingsView;
use crate::theme::colors;

/// A section's fields, one group per setting.
pub fn section() -> Div {
    v_flex().gap_4()
}

/// A label above its control, with an optional hint below (`settings-group`).
pub fn group(label: &'static str, control: impl IntoElement, hint: Option<&'static str>, cx: &App) -> Div {
    v_flex()
        .gap_1()
        .child(div().text_sm().child(label))
        .child(control)
        .when_some(hint, |group, text| group.child(self::hint(text, cx)))
}

/// A control with an optional hint below and no label (checkboxes carry their own).
pub fn unlabeled(control: impl IntoElement, hint: Option<&'static str>, cx: &App) -> Div {
    v_flex().gap_1().child(control).when_some(hint, |group, text| group.child(self::hint(text, cx)))
}

pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_xs().text_color(colors(cx).text_secondary).child(text.into())
}

pub fn error_text(text: impl Into<SharedString>, cx: &App) -> Div {
    div().text_xs().text_color(colors(cx).error).child(text.into())
}

pub fn heading(text: &'static str, cx: &App) -> Div {
    div().pt_2().text_xs().text_color(colors(cx).text_secondary).child(text.to_uppercase())
}

/// A checkbox bound to a boolean of the draft settings.
pub fn toggle(
    id: &'static str,
    label: &'static str,
    checked: bool,
    disabled: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<SettingsView>,
) -> Checkbox {
    Checkbox::new(id).label(label).checked(checked).disabled(disabled).small().on_click(cx.listener(
        move |view, checked: &bool, _, cx| {
            set(&mut view.draft, *checked);
            cx.notify();
        },
    ))
}

/// One button per option, the selected one highlighted (Electron's `<select>`s with a
/// handful of fixed options).
pub fn choices<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &[(T, &'static str)],
    selected: T,
    set: fn(&mut Settings, T),
    cx: &mut Context<SettingsView>,
) -> Div {
    let mut row = h_flex().gap_1();
    for (index, (value, label)) in options.iter().copied().enumerate() {
        let button = Button::new((id, index)).label(label).small();
        let button = if value == selected { button.primary() } else { button.ghost() };
        row = row.child(button.on_click(cx.listener(move |view, _, _, cx| {
            set(&mut view.draft, value);
            cx.notify();
        })));
    }
    row
}
