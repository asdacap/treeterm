//! The workspace Settings tab (`WorkspaceSettings.tsx`).

use std::rc::Rc;

use gpui_kit::base::SelectableText;
use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, FocusHandle, FontWeight, Subscription, Window, div, px};
use treeterm_model::tool_tabs::workspace_settings::{default_app_choices, default_app_label};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;
use crate::ui::{FieldStyle, MONO, field, normal_line, pane_select_box, pane_text_area};

pub struct WorkspaceSettingsView {
    workspace: Entity<WorkspaceStore>,
    /// The page's own focus, as Electron's `document.body`: Enter in a field moves it here
    /// (`blur()`), and so does a mouse-down on the page outside the fields (Electron blurs the
    /// field). Cmd/Ctrl+C then reaches the Kit root's Copy of the selected text rather than a
    /// field's own Copy, which takes the keystroke even with nothing selected.
    focus: FocusHandle,
    fields: Fields,
    json_open: bool,
    _observe: Subscription,
}

impl WorkspaceSettingsView {
    pub fn new(workspace: Entity<WorkspaceStore>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (name, description) = {
            let file = workspace.read(cx).file();
            (file.display_name().to_owned(), file.metadata.get("description").cloned().unwrap_or_default())
        };
        let save = |view: &mut Self, field: Field, text: &str, cx: &mut Context<Self>| {
            view.workspace.update(cx, |w, cx| match field {
                Field::Name => w.set_display_name(text, cx),
                Field::Description => w.set_description(text, cx),
            })
        };
        let focus = cx.focus_handle();
        let fields = Fields::new(name, description, focus.clone(), save, window, cx);
        let observe = cx.observe(&workspace, |_, _, cx| cx.notify());
        Self { workspace, focus, fields, json_open: false, _observe: observe }
    }
}

/// Which of [`Fields`] is saved.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Field {
    Name,
    Description,
}

/// The Name `<input>` and the Description `<textarea>`, seeded once, as Electron's `useState`.
/// Each is saved when it loses focus (`onBlur`), and Enter saves it and leaves it for `page`
/// (`blur()`). Shift+Enter types a newline in Description and saves nothing.
struct Fields {
    name: Entity<InputState>,
    description: Entity<TextareaState>,
    _subscriptions: [Subscription; 2],
}

impl Fields {
    fn new<V: 'static>(
        name: String,
        description: String,
        page: FocusHandle,
        save: impl Fn(&mut V, Field, &str, &mut Context<V>) + 'static,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).default_value(name));
        let description = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(3)
                .submit_on_enter(true)
                .placeholder("Add a description...")
                .default_value(description)
        });
        let save = Rc::new(save);
        // Enter saves at once, not only through the blur it causes: GPUI reports focus changes
        // only in an active window. The blur then saves the same text, which writes nothing.
        let on_name = {
            let (save, page) = (save.clone(), page.clone());
            cx.subscribe_in(&name, window, move |view, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => {
                    save(view, Field::Name, &input.read(cx).value(), cx);
                    window.focus(&page, cx);
                }
                InputEvent::Blur => save(view, Field::Name, &input.read(cx).value(), cx),
                InputEvent::Change | InputEvent::Focus => {}
            })
        };
        // With `submit_on_enter`, Enter and Cmd/Ctrl+Enter type nothing, and Shift+Enter has
        // typed its newline by the time it is reported.
        let on_description =
            cx.subscribe_in(&description, window, move |view, input, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { shift: false, .. } => {
                    save(view, Field::Description, &input.read(cx).value(), cx);
                    window.focus(&page, cx);
                }
                InputEvent::Blur => save(view, Field::Description, &input.read(cx).value(), cx),
                InputEvent::PressEnter { shift: true, .. } | InputEvent::Change | InputEvent::Focus => {}
            });
        Self { name, description, _subscriptions: [on_name, on_description] }
    }
}

/// `.workspace-settings-json pre`: at most 400px tall, scrolling both ways (`overflow: auto`)
/// over lines that do not wrap. Its text can be selected and copied, as the rest of the page's.
fn json_box(json: String, cx: &App) -> impl IntoElement {
    let c = colors(cx);
    div()
        .id("workspace-json")
        .debug_selector(|| "workspace-json".to_owned())
        .mt(px(8.))
        .max_h(px(400.))
        .overflow_scroll()
        // GPUI scrolls as far as the children's boxes reach, and a block child is only as wide
        // as the box: in a row, the text keeps the width of its longest line.
        .flex()
        .items_start()
        .p(px(12.))
        .rounded(px(4.))
        .border_1()
        .border_color(c.border)
        .bg(c.bg_tertiary)
        .font_family(MONO)
        .text_size(px(12.))
        .line_height(normal_line(12.))
        .text_color(c.text_primary)
        .whitespace_nowrap()
        .child(
            div()
                .flex_none()
                .debug_selector(|| "workspace-json-text".to_owned())
                .child(SelectableText::new("workspace-json-text", json)),
        )
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
        // The heading, labels and hint are ordinary page text: selectable, as the JSON is.
        let group = |label: &'static str| {
            v_flex().gap(px(6.)).child(
                div()
                    .text_size(px(13.))
                    .line_height(normal_line(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(primary)
                    .child(SelectableText::new(label, label)),
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
        let page = v_flex()
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
                    .child(SelectableText::new("workspace-settings-title", "Workspace Settings")),
            )
            .child(group("Name").child(field(&self.fields.name, style(34.), window, cx)))
            // A `<textarea>` keeps Chromium's monospace font: `.settings-input` sets none.
            .child(
                group("Description")
                    .child(pane_text_area(&self.fields.description, FieldStyle { mono: true, ..style(63.) }, window, cx)),
            )
            .child(
                group("Default Application for Children").child(picker).child(
                    div().mt(px(2.)).text_size(px(11.)).line_height(normal_line(11.)).text_color(secondary).child(
                        SelectableText::new(
                            "default-app-hint",
                            "The default application to open when creating child workspaces. If not set, inherits from the nearest ancestor that sets one, or uses the global default.",
                        ),
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
                    .when(self.json_open, |d| d.child(json_box(json, cx))),
            );
        // The whole tab, also beside the 600px column, takes the focus from a field.
        div().size_full().track_focus(&self.focus).child(page)
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::base::TextSelection;
    use gpui_kit::{
        Bounds, Focusable as _, KeyDownEvent, KeyUpEvent, Keystroke, Modifiers, MouseButton, Pixels, Point, ScrollDelta,
        ScrollEvent, ScrollSource, TestAppContext, TouchPhase, VisualTestContext, WindowOptions, point,
    };

    use super::*;

    const COPY: &str = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
    const SUBMIT: &str = if cfg!(target_os = "macos") { "cmd-enter" } else { "ctrl-enter" };

    /// The Settings tab's page, 300px wide: its fields, recording what they save, over the
    /// Raw Workspace JSON.
    struct Page {
        focus: FocusHandle,
        fields: Fields,
        json: String,
        saved: Vec<(Field, String)>,
    }

    impl Render for Page {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let c = colors(cx);
            let style = FieldStyle {
                bg: c.bg_tertiary,
                border: c.border,
                radius: 4.,
                pad: (8., 12.),
                size: 13.,
                mono: false,
                height: 34.,
            };
            let description = FieldStyle { mono: true, height: 63., ..style };
            div().size_full().track_focus(&self.focus).child(
                v_flex()
                    .w(px(300.))
                    .gap(px(20.))
                    .child(field(&self.fields.name, style, window, cx))
                    .child(pane_text_area(&self.fields.description, description, window, cx))
                    .child(json_box(self.json.clone(), cx)),
            )
        }
    }

    /// Where the focus is.
    #[derive(Debug, PartialEq)]
    enum Focused {
        Field(Field),
        Page,
        Nothing,
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    /// A window showing the page with empty fields over `json`. The window is inactive, as
    /// one driven by automation from behind other apps.
    fn inactive_page<'a>(json: &str, cx: &'a mut TestAppContext) -> (Entity<Page>, &'a mut VisualTestContext) {
        let json = json.to_owned();
        let (window, view) = cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| {
                    // As the store, which writes nothing for an unchanged value.
                    let save = |page: &mut Page, field, text: &str, _: &mut Context<Page>| {
                        let last = page.saved.iter().rev().find(|(saved, _)| *saved == field);
                        if last.is_none_or(|(_, saved)| saved != text) {
                            page.saved.push((field, text.to_owned()));
                        }
                    };
                    let focus = cx.focus_handle();
                    let fields = Fields::new(String::new(), String::new(), focus.clone(), save, window, cx);
                    Page { focus, fields, json, saved: Vec::new() }
                })
            })
            .expect("open a window")
        });
        let cx = VisualTestContext::from_window(window, cx).into_mut();
        draw(cx);
        (view, cx)
    }

    /// [`inactive_page`] in the active window, the only one GPUI reports focus changes in.
    fn page<'a>(json: &str, cx: &'a mut TestAppContext) -> (Entity<Page>, &'a mut VisualTestContext) {
        let (view, cx) = inactive_page(json, cx);
        cx.update(|window, _| window.activate_window());
        draw(cx);
        (view, cx)
    }

    fn saved(view: &Entity<Page>, cx: &mut VisualTestContext) -> Vec<(Field, String)> {
        cx.update(|_, cx| view.read(cx).saved.clone())
    }

    fn focus(view: &Entity<Page>, field: Field, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let fields = &view.read(cx).fields;
            match field {
                Field::Name => fields.name.clone().update(cx, |input, cx| input.focus(window, cx)),
                Field::Description => fields.description.clone().update(cx, |input, cx| input.focus(window, cx)),
            }
        });
        draw(cx);
    }

    fn focused(view: &Entity<Page>, cx: &mut VisualTestContext) -> Focused {
        cx.update(|window, cx| {
            let page = view.read(cx);
            if page.fields.name.focus_handle(cx).is_focused(window) {
                Focused::Field(Field::Name)
            } else if page.fields.description.focus_handle(cx).is_focused(window) {
                Focused::Field(Field::Description)
            } else if page.focus.is_focused(window) {
                Focused::Page
            } else {
                Focused::Nothing
            }
        })
    }

    fn description(view: &Entity<Page>, cx: &mut VisualTestContext) -> String {
        cx.update(|_, cx| view.read(cx).fields.description.read(cx).value().to_string())
    }

    /// Press and release `key` as a platform reports it: a key no binding takes types nothing
    /// (`simulate_keystrokes` would type Enter's newline, as an IME).
    fn press(key: &str, cx: &mut VisualTestContext) {
        let keystroke = cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
        cx.simulate_event(KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false });
        cx.simulate_event(KeyUpEvent { keystroke });
        draw(cx);
    }

    /// Electron: Enter in Name `blur()`s it, and the blur saves; leaving it any other way,
    /// by a mouse-down on the page too, saves it as well.
    #[gpui_kit::test]
    fn enter_leaves_the_name_field_which_saves_it(cx: &mut TestAppContext) {
        let (view, cx) = page("{}", cx);
        focus(&view, Field::Name, cx);
        cx.simulate_input("Renamed");
        draw(cx);
        assert_eq!(saved(&view, cx), vec![], "typing saves nothing");

        press("enter", cx);
        assert_eq!(focused(&view, cx), Focused::Page, "Enter left the field");
        assert_eq!(saved(&view, cx), vec![(Field::Name, "Renamed".to_owned())]);

        focus(&view, Field::Name, cx);
        cx.simulate_input(" again");
        focus(&view, Field::Description, cx);
        assert_eq!(saved(&view, cx).last(), Some(&(Field::Name, "Renamed again".to_owned())), "leaving saves");

        focus(&view, Field::Name, cx);
        cx.simulate_input("!");
        cx.simulate_click(point(px(150.), px(300.)), Modifiers::none());
        draw(cx);
        assert_eq!(focused(&view, cx), Focused::Page, "a click on the page left the field");
        assert_eq!(saved(&view, cx).last(), Some(&(Field::Name, "Renamed again!".to_owned())));
    }

    /// Electron: Shift+Enter in Description types a newline and saves nothing (it saves on
    /// blur only); Enter, or Cmd/Ctrl+Enter, types nothing and `blur()`s it, which saves.
    #[gpui_kit::test]
    fn shift_enter_types_a_newline_in_the_description_and_enter_leaves_it(cx: &mut TestAppContext) {
        let (view, cx) = page("{}", cx);
        focus(&view, Field::Description, cx);
        cx.simulate_input("first");
        press("shift-enter", cx);
        cx.simulate_input("second");
        draw(cx);
        assert_eq!(description(&view, cx), "first\nsecond");
        assert_eq!(focused(&view, cx), Focused::Field(Field::Description), "Shift+Enter stays in the field");
        assert_eq!(saved(&view, cx), vec![], "Shift+Enter saves nothing");

        press("enter", cx);
        assert_eq!(description(&view, cx), "first\nsecond", "Enter types no newline");
        assert_eq!(focused(&view, cx), Focused::Page, "Enter left the field");
        assert_eq!(saved(&view, cx), vec![(Field::Description, "first\nsecond".to_owned())]);

        focus(&view, Field::Description, cx);
        cx.simulate_input(" third");
        press(SUBMIT, cx);
        assert_eq!(description(&view, cx), "first\nsecond third", "Cmd/Ctrl+Enter types no newline");
        assert_eq!(focused(&view, cx), Focused::Page, "Cmd/Ctrl+Enter left the field");
        assert_eq!(saved(&view, cx).last(), Some(&(Field::Description, "first\nsecond third".to_owned())));

        focus(&view, Field::Description, cx);
        cx.simulate_input("!");
        focus(&view, Field::Name, cx);
        assert_eq!(
            saved(&view, cx).last(),
            Some(&(Field::Description, "first\nsecond third!".to_owned())),
            "leaving saves"
        );
    }

    /// Enter saves at once, also where no blur is reported (an inactive window, as
    /// automation drives).
    #[gpui_kit::test]
    fn enter_saves_without_a_reported_blur(cx: &mut TestAppContext) {
        let (view, cx) = inactive_page("{}", cx);
        focus(&view, Field::Name, cx);
        cx.simulate_input("Renamed");
        press("enter", cx);
        focus(&view, Field::Description, cx);
        cx.simulate_input("what it is for");
        press("enter", cx);
        assert_eq!(focused(&view, cx), Focused::Page);
        let expected = vec![(Field::Name, "Renamed".to_owned()), (Field::Description, "what it is for".to_owned())];
        assert_eq!(saved(&view, cx), expected);
    }

    /// 60 lines, the second far wider than the box.
    fn long_json() -> String {
        let mut lines = vec!["{".to_owned(), format!("  \"reviewComments\": \"{}\",", "x".repeat(200))];
        lines.extend((0..57).map(|n| format!("  \"key{n}\": {n},")));
        lines.push("}".to_owned());
        lines.join("\n")
    }

    fn bounds(selector: &'static str, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    /// A wheel or trackpad scroll by (`dx`, `dy`) over `at`.
    fn scroll(at: Point<Pixels>, dx: f32, dy: f32, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(at, None, Modifiers::none());
        cx.simulate_event(ScrollEvent {
            position: at,
            delta: ScrollDelta::Pixels(point(px(dx), px(dy))),
            modifiers: Modifiers::none(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        draw(cx);
    }

    /// `overflow: auto` with `max-height: 400px`: a line wider than the box is laid out
    /// whole and scrolled to sideways, inside the box, and the lines below 400px are
    /// scrolled to downwards.
    #[gpui_kit::test]
    fn the_json_box_scrolls_long_lines_sideways_and_its_lines_downwards(cx: &mut TestAppContext) {
        let (_, cx) = page(&long_json(), cx);
        let (outer, text) = (bounds("workspace-json", cx), bounds("workspace-json-text", cx));
        assert_eq!(outer.size.height, px(400.), "at most 400px tall");
        assert_eq!(outer.size.width, px(300.));
        assert!(text.size.width > px(1000.), "the long line keeps its width: {text:?}");
        assert!(text.size.height > px(400.), "every line is laid out: {text:?}");

        scroll(outer.center(), -150., 0., cx);
        let moved = bounds("workspace-json-text", cx);
        assert_eq!(moved.origin, point(text.origin.x - px(150.), text.origin.y), "scrolled sideways");

        scroll(outer.center(), 0., -100., cx);
        let moved = bounds("workspace-json-text", cx);
        assert_eq!(moved.origin, point(text.origin.x - px(150.), text.origin.y - px(100.)), "scrolled down");

        // As far as the end of the long line, in view, and no further.
        scroll(outer.center(), -5000., 0., cx);
        let end = bounds("workspace-json-text", cx);
        assert!(
            end.right() < outer.right() && end.right() > outer.right() - px(24.),
            "the line's end is in the box: {end:?} in {outer:?}"
        );
    }

    /// The JSON is ordinary page text, as in Electron's `<pre>`: after an edit in a field, a
    /// drag over it leaves the field (which saves it), selects the JSON across lines, and
    /// Cmd/Ctrl+C copies it, not the field's text.
    #[gpui_kit::test]
    fn a_drag_over_the_json_selects_it_and_copy_copies_it(cx: &mut TestAppContext) {
        let json = "{\n  \"id\": \"ws-1\",\n  \"path\": \"/srv/app\"\n}";
        let (view, cx) = page(json, cx);
        focus(&view, Field::Name, cx);
        cx.simulate_input("Renamed");
        draw(cx);

        let text = bounds("workspace-json-text", cx);
        let from = point(text.left() + px(1.), text.top() + px(1.));
        let to = point(text.right() - px(1.), text.bottom() - px(1.));
        cx.simulate_mouse_move(from, None, Modifiers::none());
        assert!(cx.update(|window, cx| TextSelection::is_selectable_at(from, window, cx)), "the JSON is selectable");
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        assert_eq!(focused(&view, cx), Focused::Page, "the drag left the field");
        assert_eq!(saved(&view, cx), vec![(Field::Name, "Renamed".to_owned())]);

        cx.simulate_keystrokes(COPY);
        assert_eq!(cx.read_from_clipboard().and_then(|item| item.text()).as_deref(), Some(json));
    }
}
