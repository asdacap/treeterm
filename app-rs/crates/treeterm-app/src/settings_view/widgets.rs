//! Building blocks shared by the sections, sized as Electron's `settings-*` classes are
//! (measured from its computed styles).

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenu};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Div, FontWeight, SharedString, Stateful, div, px};
use treeterm_core::settings::Settings;

use super::SettingsView;
use crate::theme::colors;
use crate::ui;

/// `.settings-section`: a section's fields, 16px apart.
pub fn section() -> Div {
    div().flex().flex_col().gap(px(16.))
}

/// `.settings-label`: 13px, weight 500.
pub fn label(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    div()
        .text_size(px(ui::BODY))
        .line_height(px(15.5))
        .font_weight(FontWeight::MEDIUM)
        .text_color(colors(cx).text_primary)
        .child(text)
}

/// `.settings-group`: a label above its control (6px apart), an optional hint below.
pub fn group(label_text: &'static str, control: impl IntoElement, hint: Option<&'static str>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(label(label_text, cx))
        .child(control)
        .when_some(hint, |group, text| group.child(self::hint(text, cx)))
}

/// A `.settings-group` without a label (checkboxes carry their own).
pub fn unlabeled(control: impl IntoElement, hint: Option<&'static str>, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap(px(6.))
        .child(control)
        .when_some(hint, |group, text| group.child(self::hint(text, cx)))
}

/// `.settings-hint`: 11px secondary text, 2px below what precedes it.
pub fn hint(text: impl Into<SharedString>, cx: &App) -> Div {
    let text: SharedString = text.into();
    div()
        .mt(px(2.))
        .text_size(px(ui::SMALL))
        .line_height(px(13.))
        .text_color(colors(cx).text_secondary)
        .child(text)
}

/// `.settings-hint.webapp-error`: a problem with an instance, in #f44336.
pub fn error_text(text: impl Into<SharedString>, cx: &App) -> Div {
    hint(text, cx).px(px(8.)).text_color(gpui_kit::rgb(0xf44336))
}

/// `<h3 class="settings-section-title">`: Chromium's h3 (18.72px bold), 24px above.
pub fn heading(text: &'static str, cx: &App) -> Div {
    div()
        .mt(px(24.))
        .text_size(px(18.72))
        .line_height(px(22.))
        .font_weight(FontWeight::BOLD)
        .text_color(colors(cx).text_primary)
        .child(text)
}

/// `<hr class="settings-divider">`.
pub fn divider(cx: &App) -> Div {
    div().my(px(8.)).h(px(1.)).bg(colors(cx).border)
}

/// `.settings-reset-btn`: 11px secondary text, 4px 8px padding, 3px radius, a border, 4px
/// below what precedes it; disabled at half opacity.
pub fn reset_button(id: impl Into<gpui_kit::ElementId>, text: &'static str, disabled: bool, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover_bg, primary) = (c.bg_hover, c.text_primary);
    div()
        .id(id)
        .mt(px(4.))
        .px(px(8.))
        .py(px(4.))
        .rounded(px(3.))
        .border_1()
        .border_color(c.border)
        .bg(c.bg_tertiary)
        .text_size(px(ui::SMALL))
        .line_height(px(13.))
        .text_color(c.text_secondary)
        .flex()
        .justify_center()
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover_bg).text_color(primary)))
        .child(text)
}

/// `.settings-checkbox-label` bound to a boolean of the draft settings.
pub fn toggle(
    id: &'static str,
    text: &'static str,
    checked: bool,
    disabled: bool,
    set: fn(&mut Settings, bool),
    cx: &mut Context<SettingsView>,
) -> Stateful<Div> {
    let view = cx.entity().downgrade();
    ui::checkbox_label(id, checked, disabled, text, move |on, _, cx| {
        if let Err(error) = view.update(cx, |view, cx| {
            set(&mut view.draft, on);
            cx.notify();
        }) {
            tracing::debug!(%error, "settings closed");
        }
    })
}

/// A `.settings-select` (`<select>`): the chosen option's label in the box; the options in
/// a menu below it, the chosen one ticked.
pub fn select<T: Copy + PartialEq + 'static>(
    id: &'static str,
    options: &'static [(T, &'static str)],
    selected: T,
    set: fn(&mut Settings, T),
    cx: &mut Context<SettingsView>,
) -> impl IntoElement + use<T> {
    let current = options.iter().find(|(value, _)| *value == selected).map_or("", |(_, label)| *label);
    let view = cx.entity().downgrade();
    select_menu(ui::select_box(id, current, cx), move |mut menu| {
        for (value, text) in options.iter().copied() {
            let view = view.clone();
            menu = menu.item(ui::menu_item(text).checked(value == selected).on_click(move |_, _, cx| {
                if let Err(error) = view.update(cx, |view, cx| {
                    set(&mut view.draft, value);
                    cx.notify();
                }) {
                    tracing::debug!(%error, "settings closed");
                }
            }));
        }
        menu
    })
}

/// A select box opening `build`'s menu, at least as wide as Electron's popup.
pub fn select_menu(boxed: Stateful<Div>, build: impl Fn(PopupMenu) -> PopupMenu + 'static) -> impl IntoElement {
    ui::Trigger::new(boxed).dropdown_menu(move |menu, _, _| build(menu.min_w(px(200.))))
}
