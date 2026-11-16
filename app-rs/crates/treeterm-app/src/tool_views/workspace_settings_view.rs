//! The workspace Settings tab (`WorkspaceSettings.tsx`).

use std::rc::Rc;

use gpui_kit::component::input::{InputEvent, InputState, TextareaState};
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    App, Axis, ClickEvent, Context, Div, Entity, FocusHandle, FontWeight, ScrollHandle, Stateful, Subscription, Window, div, px,
    white,
};
use treeterm_model::tool_tabs::workspace_settings::{default_app_choices, default_app_label};
use treeterm_model::workspace_store::WorkspaceStore;

use crate::theme::colors;
use crate::ui::{
    FieldStyle, MONO, PlainText, ReadingOrder, document_orders, field, normal_line, overflows, pane_select_box, pane_text_area,
    scrollbar,
};

pub struct WorkspaceSettingsView {
    workspace: Entity<WorkspaceStore>,
    /// The page's own focus, as Electron's `document.body`: Enter in a field moves it here
    /// (`blur()`), and so does a mouse-down on the page outside the fields (Electron blurs the
    /// field). Cmd/Ctrl+C then reaches the Kit root's Copy of the selected text rather than a
    /// field's own Copy, which takes the keystroke even with nothing selected.
    focus: FocusHandle,
    fields: Fields,
    /// Where the [`ReadingOrder`] of the page's text starts.
    first_order: u64,
    json: JsonPane,
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
        Self { workspace, focus, fields, first_order: document_orders(), json: JsonPane::Folded, _observe: observe }
    }
}

/// The Raw Workspace JSON (`jsonExpanded`): folded, or open with where its box is scrolled
/// to. A box opened again starts at the top, as Electron's new `<pre>` does.
enum JsonPane {
    Folded,
    Open(ScrollHandle),
}

impl JsonPane {
    /// The pane after a click on its toggle.
    fn toggled(&self) -> Self {
        match self {
            Self::Folded => Self::Open(ScrollHandle::new()),
            Self::Open(_) => Self::Folded,
        }
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

/// `.workspace-settings-json pre`: bordered, at most 400px tall, scrolling both ways
/// (`overflow: auto`) over lines that do not wrap, with Chromium's 10px scrollbars inside the
/// border: one on the right while the lines are taller than the box, one at the bottom while
/// one is wider, and the white square Chromium paints where the two meet (the page styles
/// no `::-webkit-scrollbar-corner`). Its `json` is selectable page text.
fn json_box(json: PlainText, scroll: &ScrollHandle, cx: &App) -> Div {
    let c = colors(cx);
    let corner = overflows(scroll, Axis::Vertical) && overflows(scroll, Axis::Horizontal);
    v_flex()
        .debug_selector(|| "workspace-json".to_owned())
        .mt(px(8.))
        .max_h(px(400.))
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
                .flex()
                .min_h_0()
                .child(
                    div()
                        .id("workspace-json")
                        .debug_selector(|| "workspace-json-viewport".to_owned())
                        .flex_1()
                        .min_w_0()
                        .overflow_scroll()
                        .track_scroll(scroll)
                        // GPUI scrolls as far as the children's boxes reach, and a block child
                        // is only as wide as the box: in a row, the text keeps the width of its
                        // longest line.
                        .flex()
                        .items_start()
                        .p(px(12.))
                        .child(div().flex_none().debug_selector(|| "workspace-json-text".to_owned()).child(json)),
                )
                .children(scrollbar("workspace-json", Axis::Vertical, scroll, cx)),
        )
        .child(
            div()
                .flex()
                .child(div().flex_1().min_w_0().children(scrollbar("workspace-json", Axis::Horizontal, scroll, cx)))
                .when(corner, |d| {
                    d.child(
                        div()
                            .debug_selector(|| "workspace-json-corner".to_owned())
                            .size(px(10.))
                            .flex_shrink_0()
                            .rounded_br(px(3.))
                            .bg(white()),
                    )
                }),
        )
}

/// `.settings-input` / `.settings-select`, `height` tall.
fn field_style(height: f32, cx: &App) -> FieldStyle {
    let c = colors(cx);
    FieldStyle {
        bg: c.bg_tertiary,
        border: c.border,
        radius: 4.,
        pad: (8., 12.),
        size: 13.,
        mono: false,
        height,
    }
}

/// The page (`.workspace-settings`) down to the Default Application `picker` and its hint,
/// its text taking the next places in `text`.
fn settings_page(
    text: &mut ReadingOrder,
    fields: &Fields,
    picker: impl IntoElement,
    window: &mut Window,
    cx: &mut App,
) -> Stateful<Div> {
    let c = colors(cx);
    let (secondary, primary) = (c.text_secondary, c.text_primary);
    // A `<textarea>` keeps Chromium's monospace font: `.settings-input` sets none.
    let (name_style, description_style) = (field_style(34., cx), FieldStyle { mono: true, ..field_style(63., cx) });
    // The heading, labels and hint are ordinary page text: selectable, as the JSON is.
    let group = |label: PlainText| {
        v_flex().gap(px(6.)).child(
            div()
                .text_size(px(13.))
                .line_height(normal_line(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(primary)
                .child(label),
        )
    };
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
                .child(text.text("workspace-settings-title", "Workspace Settings")),
        )
        .child(group(text.text("Name", "Name")).child(field(&fields.name, name_style, window, cx)))
        .child(
            group(text.text("Description", "Description"))
                .child(pane_text_area(&fields.description, description_style, window, cx)),
        )
        .child(
            group(text.text("Default Application for Children", "Default Application for Children")).child(picker).child(
                div().mt(px(2.)).text_size(px(11.)).line_height(normal_line(11.)).text_color(secondary).child(text.text(
                    "default-app-hint",
                    "The default application to open when creating child workspaces. If not set, inherits from the nearest ancestor that sets one, or uses the global default.",
                )),
            ),
        )
}

/// The Raw Workspace JSON's toggle (`.workspace-settings-json-toggle`, which cannot be
/// selected) and, while `pane` is open, `json` in its box, its text taking the next place in
/// `text`.
fn raw_json(
    text: &mut ReadingOrder,
    pane: &JsonPane,
    json: String,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> Div {
    let c = colors(cx);
    let (secondary, primary) = (c.text_secondary, c.text_primary);
    let toggle = h_flex()
        .id("workspace-json-toggle")
        .debug_selector(|| "workspace-json-toggle".to_owned())
        .gap(px(6.))
        .text_size(px(13.))
        .line_height(normal_line(13.))
        .text_color(secondary)
        .cursor_pointer()
        .hover(move |d| d.text_color(primary))
        .child(match pane {
            JsonPane::Folded => "▸",
            JsonPane::Open(_) => "▾",
        })
        .child("Raw Workspace JSON")
        .on_click(on_toggle);
    let section = v_flex().gap(px(6.)).child(toggle);
    match pane {
        JsonPane::Folded => section,
        JsonPane::Open(scroll) => section.child(json_box(text.text("workspace-json-text", json), scroll, cx)),
    }
}

impl Render for WorkspaceSettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.workspace.read(cx);
        let choices = default_app_choices(store.registry());
        let current = store.file().settings.default_application_id.clone();
        let json = match store.workspace_json() {
            Ok(json) => json,
            Err(error) => format!("Cannot show the workspace: {error}"),
        };
        let workspace = self.workspace.clone();
        let picker = pane_select_box("default-app", default_app_label(&choices, &current), field_style(35., cx), false, cx)
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
        let on_toggle = cx.listener(|view, _, _, cx| {
            view.json = view.json.toggled();
            cx.notify();
        });
        let mut text = ReadingOrder::after(self.first_order);
        let page = settings_page(&mut text, &self.fields, picker, window, cx);
        let page = page.child(raw_json(&mut text, &self.json, json, on_toggle, cx));
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

    /// The Settings tab's page, its fields recording what they save, with `json` as the Raw
    /// Workspace JSON (open at first) and, for the Default Application picker, a box of its
    /// height (its menu needs a workspace). The column is 600px wide, so the JSON box 560px.
    struct Page {
        focus: FocusHandle,
        fields: Fields,
        json: String,
        pane: JsonPane,
        saved: Vec<(Field, String)>,
    }

    impl Render for Page {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let on_toggle = cx.listener(|page, _, _, cx| {
                page.pane = page.pane.toggled();
                cx.notify();
            });
            let mut text = ReadingOrder::after(0);
            let page = settings_page(&mut text, &self.fields, div().h(px(35.)), window, cx);
            let page = page.child(raw_json(&mut text, &self.pane, self.json.clone(), on_toggle, cx));
            div().size_full().track_focus(&self.focus).child(page)
        }
    }

    /// Where the focus is.
    #[derive(Debug, PartialEq)]
    enum Focused {
        Field(Field),
        Page,
        Nothing,
    }

    /// Draws the window, and the frames it asks for after (a scrollbar shows in the second).
    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        crate::ui::draw_settled(cx);
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
                    Page { focus, fields, json, pane: JsonPane::Open(ScrollHandle::new()), saved: Vec::new() }
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
        cx.simulate_click(point(px(800.), px(300.)), Modifiers::none());
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

    /// `rows` lines, the second `wide` characters long.
    fn json_lines(rows: usize, wide: usize) -> String {
        let mut lines = vec!["{".to_owned(), format!("  \"reviewComments\": \"{}\",", "x".repeat(wide))];
        lines.extend((0..rows - 3).map(|n| format!("  \"key{n}\": {n},")));
        lines.push("}".to_owned());
        lines.join("\n")
    }

    /// 60 lines, the second far wider than the box.
    fn long_json() -> String {
        json_lines(60, 200)
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
        assert_eq!(outer.size.width, px(560.));
        assert!(text.size.width > px(1000.), "the long line keeps its width: {text:?}");
        assert!(text.size.height > px(400.), "every line is laid out: {text:?}");

        scroll(outer.center(), -150., 0., cx);
        let moved = bounds("workspace-json-text", cx);
        assert_eq!(moved.origin, point(text.origin.x - px(150.), text.origin.y), "scrolled sideways");

        scroll(outer.center(), 0., -100., cx);
        let moved = bounds("workspace-json-text", cx);
        assert_eq!(moved.origin, point(text.origin.x - px(150.), text.origin.y - px(100.)), "scrolled down");

        // As far as the end of the long line, its 12px of padding and the 10px scrollbar
        // from the box's right border, and no further.
        scroll(outer.center(), -5000., 0., cx);
        let end = bounds("workspace-json-text", cx);
        assert_eq!(end.right(), outer.right() - px(1. + 10. + 12.), "the line's end is in the box: {end:?} in {outer:?}");
    }

    /// Chromium's 10px scrollbars, inside the box's border: on the right while the JSON is
    /// taller than the box, at the bottom while a line is wider, and Chromium's white square
    /// where both meet. Each takes its 10px from what the box shows.
    #[gpui_kit::test]
    fn the_json_box_draws_a_scrollbar_along_each_way_it_overflows(cx: &mut TestAppContext) {
        let (view, cx) = page(&json_lines(5, 10), cx);
        let shown = |view: &Entity<Page>, json: String, cx: &mut VisualTestContext| {
            view.update(cx, |page, cx| {
                page.json = json;
                cx.notify();
            });
            draw(cx);
            let corner = cx.debug_bounds("workspace-json-corner").is_some();
            (bounds("workspace-json", cx), bounds("workspace-json-viewport", cx), corner)
        };
        let (outer, viewport, corner) = shown(&view, json_lines(5, 10), cx);
        assert_eq!(viewport.size.width, px(558.), "fits: no bar");
        assert_eq!(outer.size.height, viewport.size.height + px(2.));
        assert!(!corner);

        let (outer, viewport, corner) = shown(&view, json_lines(60, 10), cx);
        assert_eq!((viewport.size.width, viewport.size.height), (px(548.), px(398.)), "the bar on the right");
        assert_eq!(outer.size.height, px(400.));
        assert!(!corner);

        let (outer, viewport, corner) = shown(&view, json_lines(5, 200), cx);
        assert_eq!(viewport.size.width, px(558.));
        assert_eq!(outer.size.height, viewport.size.height + px(2. + 10.), "the bar at the bottom");
        assert!(!corner);

        let (_, viewport, corner) = shown(&view, json_lines(60, 200), cx);
        assert_eq!((viewport.size.width, viewport.size.height), (px(548.), px(388.)), "both bars");
        assert!(corner, "the corner between them");
    }

    /// The bars are the app's: a press on the right one's track below its thumb pages the
    /// JSON down by 87.5% of the 388px shown.
    #[gpui_kit::test]
    fn a_press_on_the_json_boxs_track_pages_it(cx: &mut TestAppContext) {
        let (_, cx) = page(&long_json(), cx);
        let (outer, text) = (bounds("workspace-json", cx), bounds("workspace-json-text", cx));
        let track = point(outer.right() - px(1. + 5.), outer.bottom() - px(1. + 10. + 5.));
        cx.simulate_mouse_down(track, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(track, MouseButton::Left, Modifiers::none());
        draw(cx);
        assert_eq!(bounds("workspace-json-text", cx).origin, point(text.origin.x, text.origin.y - px(339.)));
    }

    /// The box's scrollbars are part of it, as Chromium's are part of the `<pre>`: a wheel over
    /// the right one scrolls the JSON, not the page around it, until the JSON is at its end;
    /// the wheel then goes on to the page.
    #[gpui_kit::test]
    fn a_wheel_over_the_json_boxs_bar_scrolls_the_json_not_the_page(cx: &mut TestAppContext) {
        let (_, cx) = page(&long_json(), cx);
        // Too short for the whole page.
        cx.simulate_resize(gpui_kit::size(px(800.), px(600.)));
        draw(cx);
        // How far the JSON is scrolled in its box (less its padding).
        let json_scrolled = |cx: &mut VisualTestContext| bounds("workspace-json", cx).top() - bounds("workspace-json-text", cx).top();
        let (outer, top, toggle) = (bounds("workspace-json", cx), json_scrolled(cx), bounds("workspace-json-toggle", cx));
        let bar = point(outer.right() - px(1. + 5.), outer.top() + px(30.));
        assert!(bar.y < px(600.), "the bar is in the window: {outer:?}");

        scroll(bar, 0., -100., cx);
        assert_eq!(json_scrolled(cx), top + px(100.), "the JSON");
        assert_eq!(bounds("workspace-json-toggle", cx), toggle, "not the page");

        scroll(bar, 0., -5000., cx);
        assert_eq!(bounds("workspace-json-toggle", cx), toggle, "the JSON to its end, and not the page");
        scroll(bar, 0., -50., cx);
        let moved = bounds("workspace-json-toggle", cx);
        assert_eq!(moved.origin, point(toggle.origin.x, toggle.origin.y - px(50.)), "at its end, the page");
    }

    /// Electron mounts a new `<pre>` each time the JSON is shown: opened again, it is back at
    /// the top.
    #[gpui_kit::test]
    fn the_json_box_opened_again_starts_at_the_top(cx: &mut TestAppContext) {
        let (_, cx) = page(&long_json(), cx);
        let (outer, text) = (bounds("workspace-json", cx), bounds("workspace-json-text", cx));
        scroll(outer.center(), 0., -100., cx);
        assert_ne!(bounds("workspace-json-text", cx).origin, text.origin);
        let toggle = bounds("workspace-json-toggle", cx).center();
        for _ in 0..2 {
            cx.simulate_click(toggle, Modifiers::none());
            draw(cx);
        }
        assert_eq!(bounds("workspace-json-text", cx).origin, text.origin);
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

    /// A drag over the whole page copies its text in reading order, as Chromium copies it:
    /// the heading, the labels, the hint, then the JSON (the toggle cannot be selected).
    #[gpui_kit::test]
    fn a_drag_over_the_page_copies_its_text_in_reading_order(cx: &mut TestAppContext) {
        let json = "{\n  \"id\": \"ws-1\"\n}";
        let (_, cx) = page(json, cx);
        let text = bounds("workspace-json-text", cx);
        let (from, to) = (point(px(21.), px(21.)), point(text.right() - px(1.), text.bottom() - px(1.)));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to, MouseButton::Left, Modifiers::none());
        draw(cx);
        let hint = "The default application to open when creating child workspaces. If not set, inherits from the nearest ancestor that sets one, or uses the global default.";
        assert_eq!(
            cx.update(TextSelection::selected_text),
            format!("Workspace Settings\nName\nDescription\nDefault Application for Children\n{hint}\n{json}")
        );
    }
}
