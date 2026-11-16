//! "Add Child Workspace". Port of `CreateChildDialog.tsx`: a new branch from the parent's
//! current branch, an existing or recently used worktree, an existing local or remote
//! branch, or an open pull request — each a tab, opened on "Open Branch" as Electron's
//! session panel opens it. The tabs, their lists and the submit are
//! [`treeterm_model::child_dialog`]; this view keeps the text fields.

use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::DropdownMenu as _;
use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, Entity, FontWeight, SharedString, Subscription, Window, div, px, rgb, rgba,
};
use treeterm_model::child_dialog::{
    AppChoice, ChildDialogEvent, ChildDialogModel, ChildTab, Listing, RowState, Submitted, name_problem,
};
use treeterm_model::session_store::SessionStore;

use crate::theme::colors;
use crate::ui;

/// Called with the new child's id once creation has started (or the worktree is open).
pub type OnCreated = Rc<dyn Fn(String, &mut Window, &mut App)>;

pub struct ChildDialog {
    model: Entity<ChildDialogModel>,
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    search: Entity<InputState>,
    settings_expanded: bool,
    on_created: OnCreated,
    /// The dialog has been closed; a later event must not close the dialog under it.
    closed: bool,
    _subscriptions: Vec<Subscription>,
}

impl ChildDialog {
    fn new(
        session: Entity<SessionStore>,
        parent_id: String,
        on_created: OnCreated,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let model = cx.new(|cx| ChildDialogModel::new(session, parent_id, ChildTab::Branch, cx));
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Workspace name..."));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search branches..."));
        let description = cx.new(|cx| TextareaState::new(window, cx).placeholder("Optional description..."));
        // Enter goes to the whole dialog: `enter`.
        let on_input = |_: &mut Self, event: &InputEvent, cx: &mut Context<Self>| match event {
            InputEvent::Change => cx.notify(),
            InputEvent::PressEnter { .. } | InputEvent::Focus | InputEvent::Blur => {}
        };
        let subscriptions = vec![
            cx.observe(&model, |_, _, cx| cx.notify()),
            cx.subscribe_in(&model, window, |dialog, _, event: &ChildDialogEvent, window, cx| match event {
                ChildDialogEvent::Created(id) => {
                    if dialog.close(window, cx) {
                        (dialog.on_created)(id.clone(), window, cx);
                    }
                }
                ChildDialogEvent::ParentGone => {
                    dialog.close(window, cx);
                }
            }),
            cx.subscribe(&name, move |dialog, _, event, cx| on_input(dialog, event, cx)),
            cx.subscribe(&search, move |dialog, _, event, cx| on_input(dialog, event, cx)),
        ];
        Self {
            model,
            name,
            description,
            search,
            settings_expanded: false,
            on_created,
            closed: false,
            _subscriptions: subscriptions,
        }
    }

    /// Close the dialog, once. Returns whether it was still open.
    fn close(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.closed {
            return false;
        }
        self.closed = true;
        window.close_dialog(cx);
        true
    }

    fn switch(&mut self, tab: ChildTab, window: &mut Window, cx: &mut Context<Self>) {
        // The tab already shown keeps its search text and focus (its loader is not remounted).
        if !self.model.update(cx, |m, cx| m.switch(tab, cx)) {
            return;
        }
        let (name_placeholder, search_placeholder) = match tab {
            ChildTab::Existing => ("Leave empty to use branch name...", ""),
            ChildTab::Remote => ("Workspace name...", "Search remote branches..."),
            ChildTab::Pr => ("Workspace name...", "Search pull requests..."),
            ChildTab::Create | ChildTab::Recent | ChildTab::Branch => ("Workspace name...", "Search branches..."),
        };
        self.name.update(cx, |input, cx| input.set_placeholder(name_placeholder, window, cx));
        // Each tab's search box is new.
        self.search.update(cx, |input, cx| {
            input.set_placeholder(search_placeholder, window, cx);
            input.set_value("", window, cx);
        });
        // `autoFocus` on the Create tab's name.
        if tab == ChildTab::Create {
            self.name.update(cx, |input, cx| input.focus(window, cx));
        }
    }

    fn name_text(&self, cx: &App) -> String {
        self.name.read(cx).value().to_string()
    }

    fn description_text(&self, cx: &App) -> String {
        self.description.read(cx).value().to_string()
    }

    /// Picking a row; the worktree tabs fill in the name and description they remember.
    fn pick(&mut self, key: String, window: &mut Window, cx: &mut Context<Self>) {
        let (name, description) = (self.name_text(cx), self.description_text(cx));
        let fill = self.model.update(cx, |m, cx| m.pick(&key, &name, &description, cx));
        if let Some(name) = fill.name {
            self.name.update(cx, |input, cx| input.set_value(name, window, cx));
        }
        if let Some(description) = fill.description {
            self.description.update(cx, |input, cx| input.set_value(description, window, cx));
        }
    }

    /// The tab's submit handler; the dialog closes on the model's `Created`.
    fn submit(&mut self, cx: &mut Context<Self>) {
        let (name, description) = (self.name_text(cx), self.description_text(cx));
        self.model.update(cx, |m, cx| m.submit(&name, &description, cx));
    }

    /// `handleKeyDown`: Enter in any field or on any focused control runs the tab's submit,
    /// unless the tab has nothing to submit (no pick) or a worktree is being opened. Once the
    /// child is added or being opened the key goes no further; otherwise the focused control
    /// goes on with it, as after Electron's handler: the description breaks the line, a
    /// focused tab, Settings or Cancel presses itself.
    fn enter(&mut self, _: &mut Window, cx: &mut Context<Self>) -> ui::Entered {
        let (name, description) = (self.name_text(cx), self.description_text(cx));
        match self.model.update(cx, |m, cx| m.press_enter(&name, &description, cx)) {
            Submitted::Started => ui::Entered::Taken,
            Submitted::Ignored | Submitted::Refused => ui::Entered::PassedOn,
        }
    }

    /// `.create-child-tabs`: 0 20px padding, a bottom border; tabs 12px 16px, the active
    /// one primary with a 2px accent underline. Each is a `<button>` ([`ui::key_button`]).
    fn tabs(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (primary, secondary, accent) = (c.text_primary, c.text_secondary, c.accent);
        let shown = self.model.read(cx).tab();
        let mut row = div().flex().px(px(20.)).border_b_1().border_color(c.border);
        for tab in ChildTab::ALL {
            let active = shown == tab;
            let [first, second] = tab.label();
            let id = SharedString::from(format!("child-tab-{}", first.to_owned() + second));
            row = row.child(
                ui::key_button(id, 0., window, cx)
                    .flex_shrink_0()
                    .flex_col()
                    .items_center()
                    .px(px(16.))
                    .pt(px(12.))
                    .pb(px(10.))
                    .border_b_2()
                    .text_size(px(ui::BODY))
                    .line_height(px(15.5))
                    .cursor_pointer()
                    .when(active, |d| d.border_color(accent).text_color(primary))
                    .when(!active, |d| d.border_color(gpui_kit::transparent_black()).text_color(secondary).hover(move |d| d.text_color(primary)))
                    .child(first)
                    .child(second)
                    .on_click(cx.listener(move |dialog, _, window, cx| dialog.switch(tab, window, cx))),
            );
        }
        row.into_any_element()
    }

    /// `.create-child-existing-list`: the picker's rows, or its loading/empty message.
    fn list(&self, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let search = self.search.read(cx).value().to_string();
        let (tab, rows, selected) = {
            let model = self.model.read(cx);
            (model.tab(), model.rows(&search, cx), model.selected_key())
        };
        let (loading, empty) = match tab {
            ChildTab::Create => return div().into_any_element(),
            ChildTab::Existing => ("Loading worktrees...", "No available child worktrees found"),
            ChildTab::Recent => ("Loading recent workspaces...", "No recent workspaces found"),
            ChildTab::Branch | ChildTab::Remote | ChildTab::Pr => {
                let searching = !search.trim().is_empty();
                match (tab, searching) {
                    (ChildTab::Pr, true) => ("Loading pull requests...", "No pull requests match your search"),
                    (ChildTab::Pr, false) => ("Loading pull requests...", "No open pull requests found"),
                    (ChildTab::Remote, true) => ("Loading remote branches...", "No branches match your search"),
                    (ChildTab::Remote, false) => ("Loading remote branches...", "No remote branches found"),
                    (_, true) => ("Loading branches...", "No branches match your search"),
                    (_, false) => ("Loading branches...", "No local branches found"),
                }
            }
        };
        let message = |text: &'static str| {
            div()
                .p(px(16.))
                .flex()
                .justify_center()
                .text_size(px(ui::BODY))
                .text_color(c.text_secondary)
                .child(text)
                .into_any_element()
        };
        // `.worktree-branch`: at most 180px wide; longer text wraps (no `nowrap`).
        let second_line = |text: String| {
            div().max_w(px(180.)).overflow_hidden().text_size(px(ui::SMALL)).text_color(c.accent).child(text)
        };
        let body: Vec<AnyElement> = match rows {
            Listing::Loading => vec![message(loading)],
            Listing::Loaded(rows) if rows.is_empty() => vec![message(empty)],
            Listing::Loaded(rows) => {
                let count = rows.len();
                let (hover, active) = (c.bg_hover, c.bg_active);
                rows.into_iter()
                    .enumerate()
                    .map(|(index, row)| {
                        let is_selected = selected.as_deref() == Some(row.key.as_str());
                        let key = row.key.clone();
                        let item = div()
                            .id(SharedString::from(format!("child-row-{index}")))
                            .flex()
                            .justify_between()
                            .items_center()
                            .px(px(12.))
                            .py(px(10.))
                            .when(index + 1 < count, |d| d.border_b_1().border_color(c.border))
                            .when(is_selected, |d| d.bg(active))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(ui::BODY))
                                    .line_height(px(15.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(c.text_primary)
                                    .child(row.name),
                            )
                            .when_some(row.branch, |d, branch| d.child(second_line(branch)))
                            .when_some(row.description, |d, description| d.child(second_line(description).opacity(0.7)));
                        match row.state {
                            RowState::Available => item
                                .cursor_pointer()
                                .when(!is_selected, |d| d.hover(move |d| d.bg(hover)))
                                .on_click(cx.listener(move |dialog, _, window, cx| dialog.pick(key.clone(), window, cx))),
                            RowState::Unavailable { badge, tooltip } => item
                                .opacity(0.5)
                                .cursor_not_allowed()
                                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(tooltip).build(window, cx))
                                .child(
                                    div()
                                        .ml(px(8.))
                                        .flex_shrink_0()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(3.))
                                        .bg(c.bg_tertiary)
                                        .text_size(px(10.))
                                        .line_height(px(12.))
                                        .text_color(c.text_secondary)
                                        .child(badge),
                                ),
                        }
                        .into_any_element()
                    })
                    .collect()
            }
        };
        div()
            .id("child-list")
            .flex()
            .flex_col()
            .max_h(px(200.))
            .overflow_y_scroll()
            .border_1()
            .border_color(c.border)
            .rounded(px(4.))
            .bg(c.bg_primary)
            .children(body)
            .into_any_element()
    }

    /// `.create-child-dialog-field`: a 13px secondary label 6px above its control, 16px below.
    fn field(label: &'static str, control: impl IntoElement, cx: &App) -> gpui_kit::Div {
        div()
            .mb(px(16.))
            .child(div().mb(px(6.)).text_size(px(ui::BODY)).text_color(colors(cx).text_secondary).child(label))
            .child(control)
    }

    /// The dialog's text inputs: bg-primary (`.create-child-dialog-field input`).
    fn input(state: &Entity<InputState>, size: f32, cx: &App) -> gpui_kit::component::input::Input {
        let height = if size > 13. { 35. } else { 33.5 };
        gpui_kit::Styled::h(ui::text_input(state, cx), px(height)).text_size(px(size)).bg(colors(cx).bg_primary)
    }

    /// The collapsible "Settings" section: the inherited default application and a custom one.
    /// The toggle is a full-width `<button>`, the custom application a checkbox and a
    /// `<select>`: Tab reaches each (not while disabled).
    fn settings_section(&self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let c = colors(cx);
        let (primary, secondary, border) = (c.text_primary, c.text_secondary, c.border);
        let toggle = ui::key_button("child-settings-toggle", 0., window, cx)
            .w_full()
            .justify_start()
            .gap(px(8.))
            .py(px(4.))
            .text_size(px(ui::BODY))
            .line_height(px(16.))
            .text_color(secondary)
            .cursor_pointer()
            .hover(move |d| d.text_color(primary))
            .child(div().text_size(px(15.)).child(if self.settings_expanded { "▼" } else { "▶" }))
            .child("Settings")
            .on_click(cx.listener(|dialog, _, _, cx| {
                dialog.settings_expanded = !dialog.settings_expanded;
                cx.notify();
            }));
        let content = self.settings_expanded.then(|| {
            let model = self.model.read(cx);
            let choice = model.app_choice().clone();
            let inherited = model.inherited_app(cx).unwrap_or_else(|| "Global Default".to_owned());
            let apps = model.available_apps(cx);
            let processing = model.processing().is_some();
            // `.settings-checkbox-label`: Space toggles it; Enter is the dialog's (`enter`).
            let custom = {
                let (model, inherited) = (self.model.clone(), choice == AppChoice::Inherited);
                let label = "Use custom default application";
                let toggle = move |custom: bool, _: &mut Window, cx: &mut App| {
                    let choice = if custom { AppChoice::Custom(None) } else { AppChoice::Inherited };
                    model.update(cx, |m, cx| m.set_app_choice(choice, cx));
                };
                ui::key_checkbox_label("child-custom-app", !inherited, processing, label, toggle, window, cx)
            };
            let picker = match choice {
                AppChoice::Inherited => None,
                AppChoice::Custom(selected) => {
                    let current = selected
                        .as_ref()
                        .and_then(|id| apps.iter().find(|(app, _)| app == id).map(|(_, name)| name.clone()))
                        .unwrap_or_else(|| "Select an application...".to_owned());
                    let model = self.model.clone();
                    // A disabled `<select>`: dimmed, no tab stop, and it opens no menu.
                    let select = if processing {
                        ui::select_box("child-app", current, cx)
                            .h(px(31.))
                            .pl(px(10.))
                            .opacity(0.5)
                            .cursor_default()
                            .into_any_element()
                    } else {
                        let focus = ui::keyed_focus("child-app", window, cx);
                        let boxed = ui::key_select_box("child-app", current, &focus, cx).h(px(31.)).pl(px(10.));
                        ui::Trigger::new(boxed)
                            .dropdown_menu(move |mut menu, _, _| {
                                let none = model.clone();
                                menu = menu.item(
                                    ui::menu_item("Select an application...").checked(selected.is_none()).on_click(
                                        move |_, _, cx| none.update(cx, |m, cx| m.set_app_choice(AppChoice::Custom(None), cx)),
                                    ),
                                );
                                for (id, name) in &apps {
                                    let (model, id) = (model.clone(), id.clone());
                                    menu = menu.item(
                                        ui::menu_item(name.clone()).checked(selected.as_ref() == Some(&id)).on_click(
                                            move |_, _, cx| {
                                                model.update(cx, |m, cx| {
                                                    m.set_app_choice(AppChoice::Custom(Some(id.clone())), cx)
                                                })
                                            },
                                        ),
                                    );
                                }
                                menu.min_w(px(200.))
                            })
                            .into_any_element()
                    };
                    Some(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .child(div().text_size(px(12.)).text_color(secondary).child("Default Application"))
                            .child(select),
                    )
                }
            };
            div()
                .mt(px(12.))
                .pl(px(16.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .mb(px(12.))
                        .text_size(px(ui::BODY))
                        .text_color(secondary)
                        .child(div().font_weight(FontWeight::MEDIUM).child("Inherited:"))
                        .child(div().text_color(primary).child(inherited)),
                )
                .child(div().mb(px(12.)).child(custom))
                .children(picker)
        });
        div()
            .mt(px(16.))
            .pt(px(16.))
            .border_t_1()
            .border_color(border)
            .child(toggle)
            .children(content)
            .into_any_element()
    }
}

impl Render for ChildDialog {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (primary, secondary, tertiary, bg_primary) = {
            let c = colors(cx);
            (c.text_primary, c.text_secondary, c.bg_tertiary, c.bg_primary)
        };
        let (tab, parent_name, detached, error, processing) = {
            let model = self.model.read(cx);
            let error = model.error().map(SharedString::from);
            (model.tab(), model.parent_name(cx), model.detached(), error, model.processing())
        };
        // `disabled={isProcessing}` on the inputs, checkboxes, select and Cancel.
        let busy = processing.is_some();
        let info = div()
            .flex()
            .items_center()
            .gap(px(8.))
            .mb(px(16.))
            .px(px(12.))
            .py(px(8.))
            .rounded(px(4.))
            .bg(tertiary)
            .text_size(px(ui::BODY))
            .child(div().text_color(secondary).child("Parent:"))
            .child(div().text_color(primary).font_weight(FontWeight::MEDIUM).child(parent_name));
        let search = || div().mb(px(12.)).child(Self::input(&self.search, ui::BODY, cx).disabled(busy));
        let name_input = || Self::input(&self.name, 14., cx).disabled(busy);
        let name = self.name_text(cx);
        let body: Vec<AnyElement> = match tab {
            ChildTab::Create => {
                let problem = name_problem(&name);
                vec![
                    div()
                        .mb(px(16.))
                        .child(div().mb(px(6.)).text_size(px(ui::BODY)).text_color(secondary).child("Name"))
                        .child(name_input())
                        .when_some(problem, |d, problem| {
                            d.child(div().mt(px(4.)).text_size(px(12.)).text_color(rgb(0xf14c4c)).child(problem))
                        })
                        .into_any_element(),
                ]
            }
            ChildTab::Existing => {
                vec![Self::field("Name (optional)", name_input(), cx).into_any_element(), self.list(cx)]
            }
            ChildTab::Recent => vec![Self::field("Name", name_input(), cx).into_any_element(), self.list(cx)],
            ChildTab::Branch | ChildTab::Remote | ChildTab::Pr => vec![search().into_any_element(), self.list(cx)],
        };
        // `rows={2}` and `resize: vertical`: resizable even while disabled, as in Chromium.
        let description = (tab != ChildTab::Pr).then(|| {
            let area = ui::text_area(&self.description, cx).bg(bg_primary).disabled(busy);
            Self::field("Description", ui::rows_text_area("child-description", area, 2, window, cx), cx)
        });
        // `.create-child-detached-checkbox`: a 14px checkbox, secondary label; its 12px top
        // margin collapses into the 16px above it. Space toggles it; Enter is the dialog's
        // (`enter`), as a native checkbox ignores Enter.
        let detached = (!matches!(tab, ChildTab::Existing | ChildTab::Recent)).then(|| {
            let toggle = cx.listener(move |dialog, event: &ClickEvent, _, cx| {
                if !ui::pressed_enter(event) {
                    dialog.model.update(cx, |m, cx| m.set_detached(!detached, cx));
                }
            });
            ui::key_checkbox("child-detached", detached, busy, 14., window, cx)
                .mt(px(if description.is_some() { 0. } else { 12. }))
                .mb(px(4.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(ui::BODY))
                .text_color(secondary)
                .cursor_pointer()
                .child("Detached worktree (no merge, only close and clean)")
                .on_change(move |_, event, window, cx| toggle(event, window, cx))
        });
        let error = error.map(|error| {
            div()
                .mt(px(12.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(4.))
                .bg(rgba(0xf14c4c1a))
                .text_color(rgb(0xf14c4c))
                .text_size(px(ui::BODY))
                .child(error)
        });
        // `.create-child-processing` has no style of its own: the page's 16px text.
        let processing = processing.map(|message| div().text_size(px(16.)).text_color(primary).child(message));
        let (action, disabled) = {
            let model = self.model.read(cx);
            (model.submit_label(), model.submit_disabled(&name))
        };
        // `<button>`s: Tab reaches them unless disabled, Enter or Space presses them.
        let submit = ui::dialog_key_button("child-submit", action, ui::ButtonKind::Primary, disabled, window, cx)
            .on_click(cx.listener(|dialog, _, _, cx| dialog.submit(cx)));
        // The × and Escape still close while processing; the adopt goes on.
        let cancel = ui::dialog_key_button("child-cancel", "Cancel", ui::ButtonKind::Secondary, busy, window, cx)
            .on_click(cx.listener(|dialog, _, window, cx| {
                dialog.close(window, cx);
            }));
        let tabs = self.tabs(window, cx);
        let settings = self.settings_section(window, cx);
        ui::dialog_enter(ui::dialog_root(cx), cx, Self::enter)
            .child(ui::dialog_header("Add Child Workspace", "x", primary, cx))
            .child(tabs)
            .child(
                v_flex()
                    .p(px(20.))
                    .child(info)
                    .children(body)
                    .children(description)
                    .children(detached)
                    .child(settings)
                    .children(error)
                    .children(processing),
            )
            .child(ui::dialog_actions(cx).child(cancel).child(submit))
    }
}

pub fn open(
    session: Entity<SessionStore>,
    parent_id: String,
    _parent_label: SharedString,
    on_created: OnCreated,
    window: &mut Window,
    cx: &mut App,
) {
    let view = cx.new(|cx| ChildDialog::new(session, parent_id, on_created, window, cx));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 400., cx).child(view.clone()));
}
