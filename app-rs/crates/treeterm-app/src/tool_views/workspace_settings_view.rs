//! The workspace Settings tab (`WorkspaceSettings.tsx`).

use gpui_kit::component::button::Button;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{Context, Entity, Subscription, Window, div, px};
use treeterm_model::tool_tabs::workspace_settings::{default_app_choices, default_app_label};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;

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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, border, code_bg) = (c.text_secondary, c.border, c.bg_secondary);
        let store = self.workspace.read(cx);
        let choices = default_app_choices(store.registry());
        let current = store.file().settings.default_application_id.clone();
        let json = match store.workspace_json() {
            Ok(json) => json,
            Err(error) => format!("Cannot show the workspace: {error}"),
        };
        let label = |text: &'static str| div().text_sm().child(text);
        let workspace = self.workspace.clone();
        let picker = Button::new("default-app").small().outline().label(default_app_label(&choices, &current)).dropdown_menu(
            move |mut menu, _, _| {
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
            },
        );
        v_flex()
            .id("workspace-settings")
            .size_full()
            .p_4()
            .gap_4()
            .max_w(px(640.))
            .overflow_y_scroll()
            .child(div().text_lg().child("Workspace Settings"))
            .child(v_flex().gap_1().child(label("Name")).child(Input::new(&self.name)))
            .child(v_flex().gap_1().child(label("Description")).child(Textarea::new(&self.description)))
            .child(
                v_flex()
                    .gap_1()
                    .child(label("Default Application for Children"))
                    .child(h_flex().child(picker))
                    .child(div().text_xs().text_color(secondary).child(
                        "The default application to open when creating child workspaces. If not set, inherits from the nearest ancestor that sets one, or uses the global default.",
                    )),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .id("workspace-json-toggle")
                            .text_sm()
                            .cursor_pointer()
                            .child(format!("{} Raw Workspace JSON", if self.json_open { "▾" } else { "▸" }))
                            .on_click(cx.listener(|view, _, _, cx| {
                                view.json_open = !view.json_open;
                                cx.notify();
                            })),
                    )
                    .when(self.json_open, |d| {
                        d.child(
                            div()
                                .p_2()
                                .rounded_md()
                                .border_1()
                                .border_color(border)
                                .bg(code_bg)
                                .font_family("Menlo")
                                .text_xs()
                                .whitespace_normal()
                                .children(json.lines().map(|line| div().child(line.to_owned()))),
                        )
                    }),
            )
    }
}
