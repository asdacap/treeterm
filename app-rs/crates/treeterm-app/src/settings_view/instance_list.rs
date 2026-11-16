//! An editable list of application variants (Electron's `applications-list`): one
//! `.application-item` per instance with its icon and text fields, toggles, and Delete;
//! "+ Add" below.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, BorderStyle, Context, Entity, SharedString, Subscription, Window, div, px, rgb, rgba};

use super::form::{SandboxSupport, new_id};
use super::instance::{InstanceForm, fit_icon};
use super::{SettingsView, widgets};
use crate::theme::colors;
use crate::ui;

struct Row<T: InstanceForm> {
    /// Id, toggles and the keys this version does not know. The text is in the inputs.
    instance: T,
    icon: Entity<InputState>,
    fields: Vec<(T::Field, Entity<InputState>)>,
    /// Re-render on typing, for the row's problem and the default-application choice.
    _changes: Vec<Subscription>,
}

impl<T: InstanceForm> Row<T> {
    fn new(instance: T, window: &mut Window, cx: &mut Context<SettingsView>) -> Self {
        let icon_text = instance.icon().to_owned();
        let icon = cx.new(|cx| InputState::new(window, cx).default_value(icon_text));
        let fields: Vec<(T::Field, Entity<InputState>)> = T::FIELDS
            .iter()
            .map(|&field| {
                let text = instance.text(field).to_owned();
                let state = cx.new(|cx| InputState::new(window, cx).placeholder(T::placeholder(field)).default_value(text));
                (field, state)
            })
            .collect();
        let mut changes = vec![limit_icon(&icon, window, cx)];
        changes.extend(fields.iter().map(|(_, state)| {
            cx.subscribe(state, |_, _, event: &InputEvent, cx| match event {
                InputEvent::Change => cx.notify(),
                InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
            })
        }));
        Self { instance, icon, fields, _changes: changes }
    }

    /// The instance with what is typed now.
    fn current(&self, cx: &App) -> T {
        let mut instance = self.instance.clone();
        *instance.icon_mut() = self.icon.read(cx).value().to_string();
        for (field, state) in &self.fields {
            *instance.text_mut(*field) = state.read(cx).value().to_string();
        }
        instance
    }
}

/// Holds `icon` to Electron's `maxLength={2}`: each edit is cut to fit (`fit_icon`), where the
/// Kit's `validate` would reject a long paste whole. Re-renders the view on each change.
fn limit_icon<V: 'static>(icon: &Entity<InputState>, window: &mut Window, cx: &mut Context<V>) -> Subscription {
    let mut accepted = icon.read(cx).value().to_string();
    cx.subscribe_in(icon, window, move |_, icon, event: &InputEvent, window, cx| match event {
        InputEvent::Change => {
            let (value, caret) = {
                let state = icon.read(cx);
                (state.value(), state.cursor())
            };
            let (fitted, caret) = fit_icon(&accepted, &value, caret);
            if fitted != value.as_ref() {
                // `set_value` emits no `Change`, so this handler does not run again.
                icon.update(cx, |state, cx| {
                    state.set_value(fitted.clone(), window, cx);
                    state.set_selected_range(caret..caret, cx);
                });
            }
            accepted = fitted;
            cx.notify();
        }
        InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
    })
}

pub struct InstanceList<T: InstanceForm> {
    rows: Vec<Row<T>>,
}

impl<T: InstanceForm> InstanceList<T> {
    pub fn new(instances: Vec<T>, window: &mut Window, cx: &mut Context<SettingsView>) -> Self {
        Self { rows: instances.into_iter().map(|instance| Row::new(instance, window, cx)).collect() }
    }

    /// The instances with what is typed now, in display order.
    pub fn current(&self, cx: &App) -> Vec<T> {
        self.rows.iter().map(|row| row.current(cx)).collect()
    }

    fn add(&mut self, window: &mut Window, cx: &mut Context<SettingsView>) {
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).expect("the clock is after 1970").as_millis();
        let id = new_id(T::ID_PREFIX, now_ms, |id| self.rows.iter().any(|row| row.instance.id() == id));
        self.rows.push(Row::new(T::added(id), window, cx));
    }
}

/// Electron puts a variant's "Default" checkbox (`.default-checkbox`) beside its fields;
/// its other checkboxes (and the AI harness's "Start by Default") stack under the fields.
fn beside_fields(label: &str) -> bool {
    label == "Default"
}

/// The section of `list`; `pick` finds the same list in the view for the click handlers.
pub fn render<T: InstanceForm>(
    list: &InstanceList<T>,
    pick: fn(&mut SettingsView) -> &mut InstanceList<T>,
    sandbox: SandboxSupport,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let c = colors(cx);
    let (item_bg, hover_bg, border) = (c.bg_tertiary, c.bg_hover, c.border);
    let view = cx.entity().downgrade();
    // `.applications-list`: 12px between items, 16px below.
    let mut rows = div().flex().flex_col().gap(px(12.)).mb(px(16.));
    for (index, row) in list.rows.iter().enumerate() {
        let current = row.current(cx);
        let mut fields = div().flex_1().min_w_0().flex().flex_col().gap(px(6.));
        for (_, state) in &row.fields {
            fields = fields.child(ui::text_input(state, cx));
        }
        if let Some(problem) = current.problem() {
            fields = fields.child(widgets::error_text(problem, cx));
        }
        let mut stacked = div().flex().flex_col();
        let mut beside = None;
        for (toggle_index, toggle) in T::TOGGLES.iter().copied().enumerate() {
            let id = SharedString::from(format!("{}-{index}-toggle-{toggle_index}", T::ID_PREFIX));
            let view = view.clone();
            let label = T::toggle_label(toggle);
            let checkbox = ui::checkbox_label(
                id,
                current.toggle(toggle),
                !current.toggle_enabled(toggle, sandbox),
                label,
                move |on, _, cx| {
                    if let Err(error) = view.update(cx, |view, cx| {
                        // A click on a row deleted since the last frame finds nothing to change.
                        if let Some(row) = pick(view).rows.get_mut(index) {
                            *row.instance.toggle_mut(toggle) = on;
                            cx.notify();
                        }
                    }) {
                        tracing::debug!(%error, "settings closed");
                    }
                },
            );
            if beside_fields(label) {
                beside = Some(checkbox.whitespace_nowrap());
            } else {
                stacked = stacked.child(checkbox);
            }
        }
        let has_stacked = T::TOGGLES.iter().any(|toggle| !beside_fields(T::toggle_label(*toggle)));
        let delete_id = SharedString::from(format!("{}-{index}-delete", T::ID_PREFIX));
        // `.application-delete`: transparent, a border, #f14c4c 12px text, 4px 8px padding.
        let delete = div()
            .id(delete_id)
            .flex_shrink_0()
            .px(px(8.))
            .py(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(border)
            .text_size(px(12.))
            .line_height(px(14.))
            .text_color(rgb(0xf14c4c))
            .cursor_pointer()
            .hover(|d| d.bg(rgba(0xf14c4c1a)))
            .child("Delete")
            .on_click(cx.listener(move |view, _, _, cx| {
                let rows = &mut pick(view).rows;
                if index < rows.len() {
                    rows.remove(index);
                    cx.notify();
                }
            }));
        // `.application-item`: centred row, 12px apart, 12px padding, bg-tertiary, 6px radius.
        rows = rows.child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .p(px(12.))
                .rounded(px(6.))
                .bg(item_bg)
                // `.icon-input`: 40px wide, 16px centred text.
                .child(
                    div().w(px(40.)).flex_shrink_0().child(
                        Styled::h(ui::text_input(&row.icon, cx), px(36.5)).px(px(12.)).text_size(px(16.)).text_center(),
                    ),
                )
                .child(fields.when(has_stacked, |fields| fields.child(stacked)))
                .children(beside)
                .child(delete),
        );
    }
    let add_id = SharedString::from(format!("{}-add", T::ID_PREFIX));
    // `.settings-btn.add-app`: full width, dashed border (solid on hover), 12px below.
    let add = div()
        .id(add_id)
        .flex()
        .justify_center()
        .mb(px(12.))
        .px(px(16.))
        .py(px(8.))
        .rounded(px(4.))
        .border_1()
        .border_dashed()
        .border_color(border)
        .bg(item_bg)
        .text_size(px(ui::BODY))
        .line_height(px(16.))
        .cursor_pointer()
        .hover(move |mut d| {
            d.border_style = Some(BorderStyle::Solid);
            d.bg(hover_bg)
        })
        .child(T::ADD_LABEL)
        .on_click(cx.listener(move |view, _, window, cx| {
            pick(view).add(window, cx);
            cx.notify();
        }));
    widgets::section()
        .child(widgets::hint(T::INTRO, cx))
        .child(rows)
        .child(add)
        .child(widgets::hint(T::OUTRO, cx))
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::input::Input;
    use gpui_kit::{ClipboardItem, Render, TestAppContext, VisualTestContext};

    use super::*;

    /// An icon field held to Electron's `maxLength={2}`.
    struct IconField {
        icon: Entity<InputState>,
        _limit: Subscription,
    }

    impl Render for IconField {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div().w(px(200.)).child(Input::new(&self.icon))
        }
    }

    fn paste(text: &str, cx: &mut VisualTestContext) {
        cx.write_to_clipboard(ClipboardItem::new_string(text.to_owned()));
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-v" } else { "ctrl-v" });
    }

    /// The text with `|` at the caret.
    fn shown(icon: &Entity<InputState>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| {
            let (text, caret) = (icon.read(cx).value(), icon.read(cx).cursor());
            format!("{}|{}", &text[..caret], &text[caret..])
        })
    }

    #[gpui_kit::test]
    fn the_icon_field_cuts_what_is_typed_or_pasted_to_two_utf16_units(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        let (field, cx) = cx.add_window_view(|window, cx| {
            let icon = cx.new(|cx| InputState::new(window, cx).default_value("a"));
            let limit = limit_icon(&icon, window, cx);
            IconField { icon, _limit: limit }
        });
        let icon = cx.update(|_, cx| field.read(cx).icon.clone());
        cx.update(|window, cx| {
            icon.update(cx, |icon, cx| {
                icon.focus(window, cx);
                icon.set_selected_range(0..0, cx);
            })
        });
        cx.run_until_parked();

        // Pasted before "a": one unit is left, so "b" goes in and the caret after it.
        paste("bc", cx);
        assert_eq!(shown(&icon, cx), "b|a");
        // Full: typing changes nothing, and the field takes typing again once there is room.
        cx.simulate_input("x");
        assert_eq!(shown(&icon, cx), "b|a");
        cx.simulate_keystrokes("backspace");
        cx.simulate_input("xy");
        assert_eq!(shown(&icon, cx), "x|a");
        // Over everything: an emoji is two units, and the second does not fit.
        cx.update(|window, cx| icon.update(cx, |icon, cx| icon.select_all(window, cx)));
        paste("🤖🤖", cx);
        assert_eq!(shown(&icon, cx), "🤖|");
    }
}
