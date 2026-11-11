//! The workspace Settings tab (`WorkspaceSettings.tsx`).

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, FontWeight, Subscription, Window, div, px};
use treeterm_model::tool_tabs::workspace_settings::{default_app_choices, default_app_label};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;
use crate::ui::{FieldStyle, MONO, field, normal_line, pane_select_box, pane_text_area};

pub struct WorkspaceSettingsView {
    workspace: Entity<WorkspaceStore>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    json_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl WorkspaceSettingsView {
    pub fn new(workspace: Entity<WorkspaceStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        // Seeded once, as Electron's `useState`; saved on blur or Enter.
        let (name_text, description_text) = {
            let file = workspace.read(cx).file();
            (file.display_name().to_owned(), file.metadata.get("description").cloned().unwrap_or_default())
        };
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name_text));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .submit_on_enter(true)
                .placeholder("Add a description...")
                .default_value(description_text)
        });
        let save_name = cx.subscribe(&name, |view, input, event: &InputEvent, cx| match event {
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                let text = input.read(cx).value().to_string();
                view.workspace.update(cx, |w, cx| w.set_display_name(&text, cx));
            }
            InputEvent::Change | InputEvent::Focus => {}
        });
        let save_description = cx.subscribe(&description, |view, input, event: &InputEvent, cx| match event {
            InputEvent::Blur | InputEvent::PressEnter { .. } => {
                let text = input.read(cx).value().to_string();
                view.workspace.update(cx, |w, cx| w.set_description(&text, cx));
            }
            InputEvent::Change | InputEvent::Focus => {}
        });
        let observe = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self {
            workspace,
            name,
            description,
            json_open: false,
            _subscriptions: vec![save_name, save_description, observe],
        }
    }
}

impl Render for WorkspaceSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, primary, border, tertiary) = (c.text_secondary, c.text_primary, c.border, c.bg_tertiary);
        let store = self.workspace.read(cx);
        let choices = default_app_choices(store.registry());
        let current = store.file().settings.default_application_id.clone();
        let json = match store.workspace_json() {
            Ok(json) => json,
            Err(error) => format!("Cannot show the workspace: {error}"),
        };
        // `.settings-input` / `.settings-select`.
        let style = |height: f32| FieldStyle {
            bg: tertiary,
            border,
            radius: 4.,
            pad: (8., 12.),
            size: 13.,
            mono: false,
            height,
        };
        let group = |label: &'static str| {
            v_flex().gap(px(6.)).child(
                div()
                    .text_size(px(13.))
                    .line_height(normal_line(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(primary)
                    .child(label),
            )
        };
        let workspace = self.workspace.clone();
        let picker = pane_select_box("default-app", default_app_label(&choices, &current), style(35.), false, cx)
            .w_full()
            .dropdown_menu(move |mut menu, _, _| {
                for (index, choice) in choices.iter().enumerate() {
                    if index == 1 {
                        menu = menu.separator();
                    }
                    let (workspace, id) = (workspace.clone(), choice.id.clone());
                    menu = menu.item(
                        PopupMenuItem::new(choice.label.clone())
                            .checked(choice.id == current)
                            .on_click(move |_, _, cx| workspace.update(cx, |w, cx| w.set_default_application(&id, cx))),
                    );
                }
                menu
            });
        v_flex()
            .id("workspace-settings")
            .size_full()
            .max_w(px(600.))
            .p(px(20.))
            .gap(px(20.))
            .overflow_y_scroll()
            .text_color(primary)
            .child(
                div()
                    .text_size(px(16.))
                    .line_height(normal_line(16.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Workspace Settings"),
            )
            .child(group("Name").child(field(&self.name, style(34.), window, cx)))
            // A `<textarea>` keeps Chromium's monospace font: `.settings-input` sets none.
            .child(group("Description").child(pane_text_area(&self.description, FieldStyle { mono: true, ..style(63.) }, window, cx)))
            .child(
                group("Default Application for Children").child(picker).child(
                    div().mt(px(2.)).text_size(px(11.)).line_height(normal_line(11.)).text_color(secondary).child(
                        "The default application to open when creating child workspaces. If not set, inherits from the nearest ancestor that sets one, or uses the global default.",
                    ),
                ),
            )
            .child(
                v_flex()
                    .gap(px(6.))
                    .child(
                        h_flex()
                            .id("workspace-json-toggle")
                            .gap(px(6.))
                            .text_size(px(13.))
                            .line_height(normal_line(13.))
                            .text_color(secondary)
                            .cursor_pointer()
                            .hover(move |d| d.text_color(primary))
                            .child(if self.json_open { "▾" } else { "▸" })
                            .child("Raw Workspace JSON")
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.json_open = !view.json_open;
                                cx.notify();
                            })),
                    )
                    .when(self.json_open, |d| {
                        d.child(
                            div()
                                .id("workspace-json")
                                .mt(px(8.))
                                .max_h(px(400.))
                                .overflow_y_scroll()
                                .p(px(12.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(border)
                                .bg(tertiary)
                                .font_family(MONO)
                                .text_size(px(12.))
                                .line_height(normal_line(12.))
                                .text_color(primary)
                                .children(json.lines().map(|line| div().whitespace_nowrap().child(line.to_owned()))),
                        )
                    }),
            )
    }
}
