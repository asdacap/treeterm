//! An editable list of application variants (Electron's `applications-list`): one row per
//! instance with its icon and text fields, toggles, and Delete; "+ Add" below.

use std::time::{SystemTime, UNIX_EPOCH};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{Disableable as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, App, Context, Entity, SharedString, Subscription, Window, div, px};

use super::form::{SandboxSupport, new_id};
use super::instance::{InstanceForm, icon_fits};
use super::{SettingsView, widgets};
use crate::theme::colors;

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
        let icon = cx.new(|cx| InputState::new(window, cx).validate(|text, _| icon_fits(text)).default_value(icon_text));
        let fields: Vec<(T::Field, Entity<InputState>)> = T::FIELDS
            .iter()
            .map(|&field| {
                let text = instance.text(field).to_owned();
                let state = cx.new(|cx| InputState::new(window, cx).placeholder(T::placeholder(field)).default_value(text));
                (field, state)
            })
            .collect();
        let _changes = std::iter::once(&icon)
            .chain(fields.iter().map(|(_, state)| state))
            .map(|state| {
                cx.subscribe(state, |_, _, event: &InputEvent, cx| match event {
                    InputEvent::Change => cx.notify(),
                    InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
                })
            })
            .collect();
        Self { instance, icon, fields, _changes }
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

/// The section of `list`; `pick` finds the same list in the view for the click handlers.
pub fn render<T: InstanceForm>(
    list: &InstanceList<T>,
    pick: fn(&mut SettingsView) -> &mut InstanceList<T>,
    sandbox: SandboxSupport,
    cx: &mut Context<SettingsView>,
) -> AnyElement {
    let border = colors(cx).border;
    let mut rows = v_flex().gap_2();
    for (index, row) in list.rows.iter().enumerate() {
        let current = row.current(cx);
        let mut fields = v_flex().flex_1().gap_1();
        for (_, state) in &row.fields {
            fields = fields.child(Input::new(state).small());
        }
        let mut toggles = h_flex().flex_wrap().gap_3();
        for (toggle_index, toggle) in T::TOGGLES.iter().copied().enumerate() {
            let id = SharedString::from(format!("{}-{index}-toggle-{toggle_index}", T::ID_PREFIX));
            toggles = toggles.child(
                Checkbox::new(id)
                    .label(T::toggle_label(toggle))
                    .checked(current.toggle(toggle))
                    .disabled(!current.toggle_enabled(toggle, sandbox))
                    .small()
                    .on_click(cx.listener(move |view, checked: &bool, _, cx| {
                        // A click on a row deleted since the last frame finds nothing to change.
                        if let Some(row) = pick(view).rows.get_mut(index) {
                            *row.instance.toggle_mut(toggle) = *checked;
                            cx.notify();
                        }
                    })),
            );
        }
        let delete_id = SharedString::from(format!("{}-{index}-delete", T::ID_PREFIX));
        rows = rows.child(
            h_flex()
                .gap_2()
                .items_start()
                .p_2()
                .border_1()
                .border_color(border)
                .rounded_md()
                .child(div().w(px(48.)).child(Input::new(&row.icon).small()))
                .child(
                    fields
                        .when_some(current.problem(), |fields, problem| fields.child(widgets::error_text(problem, cx)))
                        .child(toggles),
                )
                .child(Button::new(delete_id).label("Delete").small().ghost().on_click(cx.listener(
                    move |view, _, _, cx| {
                        let rows = &mut pick(view).rows;
                        if index < rows.len() {
                            rows.remove(index);
                            cx.notify();
                        }
                    },
                ))),
        );
    }
    let add_id = SharedString::from(format!("{}-add", T::ID_PREFIX));
    widgets::section()
        .child(widgets::hint(T::INTRO, cx))
        .child(rows)
        .child(
            div().child(Button::new(add_id).label(T::ADD_LABEL).small().outline().on_click(cx.listener(
                move |view, _, window, cx| {
                    pick(view).add(window, cx);
                    cx.notify();
                },
            ))),
        )
        .child(widgets::hint(T::OUTRO, cx))
        .into_any_element()
}
