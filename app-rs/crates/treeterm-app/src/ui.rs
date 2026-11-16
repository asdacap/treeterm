//! Building blocks shared by the views, sized and coloured as `src/renderer/styles/index.css`
//! draws them, so the native chrome matches Electron pixel for pixel.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    App, ClickEvent, Div, ElementId, FocusHandle, Hsla, KeyBinding, KeyboardButton, KeyboardClickEvent, NoAction, Stateful,
    Window, div, px, rgb,
};

use crate::theme::colors;

/// The UI font size most chrome text uses (`font-size: 11px`).
pub const SMALL: f32 = 11.;
/// Tree rows, titles (`font-size: 13px`).
pub const BODY: f32 = 13.;

/// A Lucide icon (`<Name size={size} />`), drawn in `color`.
pub fn icon(name: IconName, size: f32, color: impl Into<Hsla>) -> Icon {
    Icon::new(name).size(px(size)).text_color(color.into())
}

/// A 24×24 square, rounded 4px, its content centred: a square button's box.
fn square() -> Div {
    div().size(px(24.)).flex_shrink_0().flex().items_center().justify_center().rounded(px(4.))
}

/// A 24×24 square button body, rounded 4px; callers add colours and the hover style
/// (GPUI allows one hover style per element).
fn square_button(id: impl Into<ElementId>) -> Stateful<Div> {
    square().id(id).cursor_pointer()
}

/// `.add-button`: a 24×24 borderless button, rounded 4px, filled on hover.
pub fn icon_button(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let hover = c.bg_hover;
    square_button(id).text_color(c.text_primary).hover(move |d| d.bg(hover))
}

/// `.tree-item-action`: as [`icon_button`], secondary until hovered.
pub fn item_action(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    square_button(id).text_color(c.text_secondary).hover(move |d| d.bg(hover).text_color(primary))
}

/// [`item_action`] on a row that takes no pointer events (`pointer-events: none`): the same
/// box and colour, with no cursor and no hover.
pub fn item_action_inert(cx: &App) -> Div {
    square().text_color(colors(cx).text_secondary)
}

/// `.workspace-action-btn`: the workspace header's 24px-high buttons (11px text, 4px 12px
/// padding, 1px border); hover darkens and shows the accent border.
pub fn action_button(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover_bg, accent) = (c.bg_primary, c.accent);
    div()
        .id(id)
        .h(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .gap(px(4.))
        .px(px(12.))
        .rounded(px(4.))
        .border_1()
        .border_color(c.border)
        .bg(c.bg_hover)
        .text_size(px(SMALL))
        .line_height(px(14.))
        .text_color(c.text_primary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover_bg).border_color(accent))
}

/// `.tree-title`: 11px semibold uppercase secondary text with 0.5px tracking.
pub fn caps_title(text: impl Into<gpui_kit::SharedString>, cx: &App) -> Div {
    let text: gpui_kit::SharedString = text.into();
    div()
        .text_size(px(SMALL))
        .font_weight(gpui_kit::FontWeight::SEMIBOLD)
        .text_color(colors(cx).text_secondary)
        .child(gpui_kit::SharedString::from(text.to_uppercase()))
}

/// A rounded pill (`.workspace-branch`, `.activity-state-badge`, …) around `content`.
pub fn pill(content: impl IntoElement, fg: impl Into<Hsla>, bg: impl Into<Hsla>) -> Div {
    div()
        .flex_shrink_0()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .text_size(px(SMALL))
        .text_color(fg.into())
        .bg(bg.into())
        .child(content)
}

/// `.terminal-toggle`: a 28×14 slider (green when on) and an 11px label.
pub fn toggle(
    id: impl Into<ElementId>,
    on: bool,
    label: &'static str,
    on_click: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    toggle_owned(id, on, label, on_click)
}

/// [`toggle`] with a computed label.
pub fn toggle_owned(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<gpui_kit::SharedString>,
    on_click: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: gpui_kit::SharedString = label.into();
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .gap(px(6.))
        .cursor_pointer()
        .text_size(px(SMALL))
        .text_color(rgb(0x999999))
        .child(
            div()
                .relative()
                .w(px(28.))
                .h(px(14.))
                .rounded(px(7.))
                .bg(rgb(if on { 0x0dbc79 } else { 0x555555 }))
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(if on { 16. } else { 2. }))
                        .size(px(10.))
                        .rounded_full()
                        .bg(rgb(0xcccccc)),
                ),
        )
        .child(div().whitespace_nowrap().child(label))
        .on_click(move |_: &ClickEvent, window, cx| on_click(!on, window, cx))
}

/// A styled element usable as a dropdown trigger (`dropdown_menu` needs `Selectable`), so
/// header buttons keep their CSS look instead of GPUI Kit's button styles.
pub struct Trigger {
    inner: Stateful<Div>,
    selected: bool,
}

impl Trigger {
    pub fn new(inner: Stateful<Div>) -> Self {
        Self { inner, selected: false }
    }
}

impl gpui_kit::component::Selectable for Trigger {
    fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.selected
    }
}

impl Styled for Trigger {
    fn style(&mut self) -> &mut gpui_kit::StyleRefinement {
        self.inner.style()
    }
}

impl InteractiveElement for Trigger {
    fn interactivity(&mut self) -> &mut gpui_kit::Interactivity {
        self.inner.interactivity()
    }
}

impl gpui_kit::component::menu::DropdownMenu for Trigger {}

impl IntoElement for Trigger {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        self.inner
    }
}

/// A two-line menu item (`.abandon-menu-item`: 13px label, 11px secondary hint).
pub fn menu_item_with_hint(label: &'static str, hint: &'static str) -> gpui_kit::component::menu::PopupMenuItem {
    gpui_kit::component::menu::PopupMenuItem::element(move |_, cx| {
        gpui_kit::component::v_flex()
            .py(px(2.))
            .child(div().text_size(px(BODY)).child(label))
            .child(div().mt(px(2.)).text_size(px(SMALL)).text_color(colors(cx).text_secondary).child(hint))
    })
}

/// The `.activity-*` colour of `state`; idle has none and keeps `current`, the colour of the
/// text around it.
pub fn activity_color(state: treeterm_model::workspace_store::ActivityState, current: impl Into<Hsla>) -> Hsla {
    use treeterm_model::workspace_store::ActivityState;
    match state {
        ActivityState::Idle => current.into(),
        ActivityState::Working => rgb(0xe5e510).into(),
        ActivityState::UserInputRequired => rgb(0x00bcd4).into(),
        ActivityState::PermissionRequest => rgb(0xcd6600).into(),
        ActivityState::SafePermissionRequested => rgb(0x0dbc79).into(),
        ActivityState::Completed => rgb(0x23d18b).into(),
        ActivityState::ApplicationError => rgb(0xd16969).into(),
        ActivityState::Error => rgb(0xf44747).into(),
    }
}

/// `ActivityIndicator`: the state's glyph in its `.activity-*` colour (idle keeps `idle`'s
/// colour, the text around it); working spins.
pub fn activity_glyph(
    state: treeterm_model::workspace_store::ActivityState,
    size: f32,
    idle: impl Into<Hsla>,
) -> gpui_kit::AnyElement {
    use treeterm_model::workspace_store::ActivityState;
    let color = activity_color(state, idle);
    let glyph = match state {
        ActivityState::Idle => "○",
        ActivityState::Working => {
            use gpui_kit::component::Sizable as _;
            return gpui_kit::component::spinner::Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .with_size(px(size))
                .color(color)
                .into_any_element();
        }
        ActivityState::UserInputRequired => "▶",
        ActivityState::PermissionRequest | ActivityState::SafePermissionRequested | ActivityState::Error => "●",
        ActivityState::Completed => "✓",
        ActivityState::ApplicationError => "✗",
    };
    div().text_size(px(size)).text_color(color).child(glyph).into_any_element()
}

/// `.tree-item-flash`: a wash of the flashing state's colour over a row or rail icon (absolute,
/// inset 0, no pointer events, the parent's `radius`) at the flash's opacity now, from
/// `WorkspaceStore::attention_flash`. The parent is `relative`; added after the row's content,
/// it paints over it, as the positioned span did. Idle and working never flash.
pub fn attention_flash(
    (state, opacity): (treeterm_model::workspace_store::ActivityState, f32),
    radius: f32,
) -> Div {
    let color = activity_color(state, gpui_kit::transparent_black());
    div().absolute().inset_0().rounded(px(radius)).bg(color.opacity(opacity))
}

/// `.workspace-unread-marker`: the accent "●" after a tree or favourite row's name while it
/// has unread activity. Where it takes the pointer, its tooltip is [`unread_hint`].
pub fn unread_marker(id: impl Into<ElementId>, accent: impl Into<Hsla>) -> Stateful<Div> {
    div().id(id).mx(px(4.)).flex_shrink_0().text_size(px(10.)).text_color(accent.into()).child("●")
}

/// The unread marker's `title`: how to clear it.
pub fn unread_hint(window: &mut Window, cx: &mut App) -> gpui_kit::AnyView {
    gpui_kit::component::tooltip::Tooltip::new("New activity — open workspace to mark as read").build(window, cx)
}

/// An element's `title`: its text on hover, wrapped at 400px as the system draws a long one.
pub fn title_tooltip(text: impl Into<SharedString>) -> impl Fn(&mut Window, &mut App) -> gpui_kit::AnyView + 'static {
    let text = text.into();
    move |window, cx| {
        let text = text.clone();
        gpui_kit::component::tooltip::Tooltip::element(move |_, _| div().max_w(px(400.)).child(text.clone())).build(window, cx)
    }
}

/// The 12px spinner after the unread marker while an unread-marker edit is not yet saved
/// (`Loader2` "Saving workspace attention"), in the row's text colour.
pub fn attention_saving(color: impl Into<Hsla>) -> gpui_kit::AnyElement {
    use gpui_kit::component::Sizable as _;
    gpui_kit::component::spinner::Spinner::new()
        .icon(Icon::new(IconName::LoaderCircle))
        .with_size(px(12.))
        .color(color.into())
        .into_any_element()
}

/// A CSS border spinner (`loading.html`'s `.spinner`): a `size`px ring, 2px wide, in `track`,
/// its top quarter in `arc`, turning once every 0.8 s at a steady speed.
pub fn ring_spinner(id: impl Into<ElementId>, size: f32, track: Hsla, arc: Hsla) -> gpui_kit::AnyElement {
    use gpui_kit::{Animation, AnimationExt as _, PathBuilder, point};
    use std::f32::consts::{FRAC_PI_4, PI, TAU};
    // The middle of the 2px border, where both strokes run. Both are paths: a div's border
    // is painted over its children.
    let radius = size / 2. - 1.;
    div().size(px(size)).flex_shrink_0().with_animation(
        id,
        Animation::new(std::time::Duration::from_millis(800)).repeat(),
        move |spinner, turn| {
            spinner.child(
                gpui_kit::canvas(
                    |_, _, _| {},
                    move |bounds, (), window, _| {
                        // An angle clockwise from the top, as `rotate()` turns.
                        let center = bounds.center();
                        let at = |angle: f32| {
                            point(center.x + px(radius * angle.sin()), center.y - px(radius * angle.cos()))
                        };
                        let radii = point(px(radius), px(radius));
                        let mut paint = |path: PathBuilder, color: Hsla| match path.build() {
                            Ok(path) => window.paint_path(path, color),
                            Err(error) => tracing::error!(%error, "cannot draw the spinner"),
                        };
                        let mut ring = PathBuilder::stroke(px(2.));
                        ring.move_to(at(0.));
                        ring.arc_to(radii, px(0.), false, true, at(PI));
                        ring.arc_to(radii, px(0.), false, true, at(TAU));
                        paint(ring, track);
                        // The top quarter, turned by `turn` of a full circle.
                        let middle = turn * TAU;
                        let mut quarter = PathBuilder::stroke(px(2.));
                        quarter.move_to(at(middle - FRAC_PI_4));
                        quarter.arc_to(radii, px(0.), false, true, at(middle + FRAC_PI_4));
                        paint(quarter, arc);
                    },
                )
                .size_full(),
            )
        },
    )
    .into_any_element()
}

// --- dialogs ---
// Electron's modal dialogs (`.dialog-overlay` around `.settings-dialog`,
// `.create-child-dialog`, …) and their form controls, measured from its computed styles.

use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::{Sizable as _, Size, WindowExt as _};
use gpui_kit::{Context, Entity, FontWeight, SharedString, relative};

/// The dialog box: bg-secondary, 8px radius, no border or padding, centred, shown at once
/// (no entrance animation). Callers add [`dialog_header`], their content and
/// [`dialog_actions`] as children.
pub fn dialog_frame(dialog: Dialog, width: f32, cx: &App) -> Dialog {
    dialog
        // Enter in a single-line input also fires the Kit's `Confirm`, whose default closes
        // the dialog, and the view is released before the input's `PressEnter` reaches it:
        // Enter would close without submitting. The dialogs handle Enter themselves.
        .on_ok(|_, _, _| false)
        .w(px(width))
        .close_button(false)
        .centered()
        .entrance(DialogEntrance::None)
        .p_0()
        .gap_0()
        .border_0()
        .rounded(px(8.))
        .bg(colors(cx).bg_secondary)
}

/// Dialog content root: GPUI Kit renders dialogs outside the app root, so set the text
/// metrics the app root sets (Chromium's `line-height: normal`).
pub fn dialog_root(cx: &App) -> Div {
    gpui_kit::component::v_flex().w_full().line_height(relative(1.2)).text_color(colors(cx).text_primary)
}

/// `.xxx-dialog-header`: 16px 20px padding, a bottom border, the 16px/500 title and the
/// close button (`.dialog-close`, whose glyph is "×" or "x" depending on the dialog).
pub fn dialog_header(title: impl Into<SharedString>, close_glyph: &'static str, title_color: impl Into<Hsla>, cx: &App) -> Div {
    let close = dialog_close_look(div().id("dialog-close").rounded(px(4.)), close_glyph, cx)
        .on_click(|_, window, cx| window.close_dialog(cx));
    dialog_header_with(title, title_color, close, cx)
}

/// [`dialog_header`] whose close button is a `<button>` ([`key_button`]): Tab reaches it, and
/// Enter or Space closes the dialog.
pub fn dialog_key_header(
    title: impl Into<SharedString>,
    close_glyph: &'static str,
    title_color: impl Into<Hsla>,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let close = dialog_close_look(key_button("dialog-close", 4., window, cx), close_glyph, cx)
        .on_click(|_, window, cx| window.close_dialog(cx));
    dialog_header_with(title, title_color, close, cx)
}

fn dialog_header_with(
    title: impl Into<SharedString>,
    title_color: impl Into<Hsla>,
    close: impl IntoElement,
    cx: &App,
) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .px(px(20.))
        .py(px(16.))
        .border_b_1()
        .border_color(colors(cx).border)
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(19.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(title_color.into())
                .child(title.into()),
        )
        .child(close)
}

/// `.dialog-close` in a dialog header: 18px, 4px 8px padding, secondary until hovered.
fn dialog_close_look<B: Styled + InteractiveElement + ParentElement>(button: B, glyph: &'static str, cx: &App) -> B {
    let c = colors(cx);
    let (hover_bg, primary) = (c.bg_hover, c.text_primary);
    button
        .px(px(8.))
        .py(px(4.))
        .text_size(px(18.))
        .line_height(px(21.))
        .text_color(c.text_secondary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover_bg).text_color(primary))
        .child(glyph)
}

/// `.xxx-dialog-actions`: right-aligned buttons, 8px apart, 16px 20px padding, a top border.
pub fn dialog_actions(cx: &App) -> Div {
    div()
        .flex()
        .flex_shrink_0()
        .justify_end()
        .gap(px(8.))
        .px(px(20.))
        .py(px(16.))
        .border_t_1()
        .border_color(colors(cx).border)
}

/// The `.dialog-btn` / `.settings-btn` variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ButtonKind {
    /// `.cancel` / `.secondary`: bg-tertiary, hover bg-hover.
    Secondary,
    /// `.create` / `.primary` / `.save`: accent, white text, hover #006cbf.
    Primary,
    /// `.close-anyway` / `.fork-anyway`: #b36200, white text, hover #cc7000.
    Warning,
}

/// `.dialog-btn`: 8px 16px padding, 4px radius, 13px text, no border; disabled at half
/// opacity without hover.
pub fn dialog_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    disabled: bool,
    cx: &App,
) -> Stateful<Div> {
    dialog_button_look(div().id(id), label, kind, disabled, cx)
}

/// [`dialog_button`] as a `<button>` ([`key_button`]): Tab reaches it unless it is disabled,
/// and Enter or Space presses it.
pub fn dialog_key_button(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    disabled: bool,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Button {
    let button = key_button(id, 4., window, cx).disabled(disabled);
    dialog_button_look(button, label, kind, disabled, cx)
}

fn dialog_button_look<B: Styled + InteractiveElement + ParentElement + IntoElement>(
    button: B,
    label: impl Into<SharedString>,
    kind: ButtonKind,
    disabled: bool,
    cx: &App,
) -> B {
    let c = colors(cx);
    let (bg, fg, hover): (Hsla, Hsla, Hsla) = match kind {
        ButtonKind::Secondary => (c.bg_tertiary, c.text_primary, c.bg_hover),
        ButtonKind::Primary => (c.accent, gpui_kit::white(), rgb(0x006cbf).into()),
        ButtonKind::Warning => (rgb(0xb36200).into(), gpui_kit::white(), rgb(0xcc7000).into()),
    };
    let label: SharedString = label.into();
    button
        .flex_shrink_0()
        .px(px(16.))
        .py(px(8.))
        .rounded(px(4.))
        .text_size(px(BODY))
        .line_height(px(15.5))
        .bg(bg)
        .text_color(fg)
        .when(disabled, |d| d.opacity(0.5).cursor_not_allowed())
        .when(!disabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
        .child(label)
}

/// `.settings-input`: bg-tertiary, 1px border (accent when focused), 4px radius, 8px 12px
/// padding, 13px text, 34px tall. Callers restyle it for the other dialogs' inputs.
pub fn text_input(state: &Entity<InputState>, cx: &App) -> Input {
    let c = colors(cx);
    // `Styled::h`: `Input::h` sizes only multi-line inputs.
    Styled::h(Input::new(state), px(33.5))
        .px(px(12.))
        .py(px(8.))
        .text_size(px(BODY))
        .bg(c.bg_tertiary)
        .border_color(c.border)
        .rounded(px(4.))
}

/// `<textarea class="settings-input">`: as [`text_input`], several lines tall.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    let c = colors(cx);
    // Size::Medium pads the editor 8px 10px; 2px more on each side makes Electron's 12px.
    Textarea::new(state)
        .with_size(Size::Medium)
        .px(px(2.))
        .text_size(px(BODY))
        .line_height(px(15.5))
        .bg(c.bg_tertiary)
        .border_color(c.border)
        .rounded(px(4.))
}

/// A native (Chromium, macOS) checkbox: white with a #767676 border, #0075ff with a white
/// tick when checked; disabled ones grey.
pub fn checkbox_box(checked: bool, disabled: bool, size: f32) -> Div {
    let (fill, border, tick): (u32, u32, u32) = match (checked, disabled) {
        (true, false) => (0x0075ff, 0x0075ff, 0xffffff),
        (true, true) => (0xa2a2a2, 0x767676, 0xdadada),
        (false, false) => (0xffffff, 0x767676, 0xffffff),
        (false, true) => (0x8f8f8f, 0x767676, 0x8f8f8f),
    };
    let tick_size = (size * 0.75).round();
    let offset = (size - 2. - tick_size) / 2.;
    let mut checkbox = div()
        .relative()
        .flex_shrink_0()
        .size(px(size))
        .rounded(px((size / 6.).round()))
        .bg(rgb(fill))
        .border_1()
        .border_color(rgb(border));
    if checked {
        // Chromium's tick is about twice as thick as Lucide's stroke: draw it three times,
        // half a pixel apart.
        for (dx, dy) in [(0., 0.), (0.5, 0.), (0., 0.5)] {
            checkbox = checkbox.child(
                div()
                    .absolute()
                    .left(px(offset + dx))
                    .top(px(offset + dy))
                    .child(icon(IconName::Check, tick_size, rgb(tick))),
            );
        }
    }
    checkbox
}

/// `.settings-checkbox-label`: a 16px checkbox, 8px gap, 13px label; the whole row toggles.
pub fn checkbox_label(
    id: impl Into<ElementId>,
    checked: bool,
    disabled: bool,
    label: impl Into<SharedString>,
    on_click: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(BODY))
        .line_height(px(16.))
        .when(!disabled, |d| d.cursor_pointer())
        .child(checkbox_box(checked, disabled, 16.))
        .child(label)
        .when(!disabled, |d| d.on_click(move |_: &ClickEvent, window, cx| on_click(!checked, window, cx)))
}

/// [`checkbox_label`] as a native checkbox in its `<label>` ([`key_checkbox`]): Tab reaches it
/// unless `disabled`, and Space toggles it, but Enter does not. The row keeps the pointer
/// cursor while disabled (`.settings-checkbox-label`), though a click on it does nothing.
pub fn key_checkbox_label(
    id: impl Into<ElementId>,
    checked: bool,
    disabled: bool,
    label: impl Into<SharedString>,
    on_click: impl Fn(bool, &mut Window, &mut App) + 'static,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Checkbox {
    let label: SharedString = label.into();
    key_checkbox(id, checked, disabled, 16., window, cx)
        .flex()
        .items_center()
        .gap(px(8.))
        .text_size(px(BODY))
        .line_height(px(16.))
        .cursor_pointer()
        .child(label)
        .on_change(move |next, event, window, cx| {
            if !pressed_enter(event) {
                on_click(next == gpui_kit::base::CheckboxState::Checked, window, cx);
            }
        })
}

/// `.settings-select`: a `<select>` box (as the input, 35px tall) with the chosen option and
/// Chromium's chevron at the right. Wrap it in [`Trigger`] for its menu.
pub fn select_box(id: impl Into<ElementId>, label: impl Into<SharedString>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let label: SharedString = label.into();
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_between()
        .gap(px(4.))
        .h(px(35.))
        .pl(px(16.))
        .pr(px(8.))
        .bg(c.bg_tertiary)
        .border_1()
        .border_color(c.border)
        .rounded(px(4.))
        .text_size(px(BODY))
        .text_color(c.text_primary)
        .cursor_pointer()
        .child(div().truncate().child(label))
        .child(icon(IconName::ChevronDown, 12., c.text_primary))
}

/// [`select_box`] as a `<select>` that takes the keyboard ([`key_select`]); focused, its
/// border is accent (`.settings-select:focus`), however it took the focus.
pub fn key_select_box(
    id: impl Into<ElementId>,
    label: impl Into<SharedString>,
    focus: &FocusHandle,
    cx: &App,
) -> Stateful<Div> {
    let accent = colors(cx).accent;
    key_select(select_box(id, label, cx), focus).focus(move |d| d.border_color(accent))
}

/// A select box (a [`Trigger`] for its menu) as a `<select>` on macOS: Tab reaches it, and
/// Space, ArrowUp or ArrowDown open its menu, but Enter does not. `focus` is its focus handle,
/// which a `<label>` linked to it focuses ([`keyed_focus`] when none is).
pub fn key_select(boxed: Stateful<Div>, focus: &FocusHandle) -> Stateful<Div> {
    boxed.track_focus(&focus.clone().tab_stop(true)).key_context(KEY_SELECT).role(gpui_kit::Role::ComboBox)
}

/// A `.context-menu-item` for GPUI Kit's popup menu: 13px text on a 31.5px pitch (the menu
/// adds the 12px left padding and 2px between items itself).
pub fn menu_item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label: SharedString = label.into();
    PopupMenuItem::element(move |_, _| div().h(px(29.5)).flex().items_center().text_size(px(BODY)).child(label.clone()))
}

/// Fill a popup menu from `model`, and fill it again whenever `model` notifies while the menu
/// is open, as a React menu re-renders when its store changes (GPUI Kit builds a dropdown's
/// menu once per open).
///
/// The observation belongs to the menu and ends when the menu is released. The observer is
/// stored under `model`, so it gets the model as an argument instead of holding it: a strong
/// handle there would keep the model alive for good.
pub fn menu_following<M: 'static>(
    menu: PopupMenu,
    model: &Entity<M>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
    fill: impl Fn(PopupMenu, &Entity<M>, &mut Context<PopupMenu>) -> PopupMenu + 'static,
) -> PopupMenu {
    let menu = fill(menu, model, cx);
    let observation = cx.observe_in(model, window, move |menu, model, window, cx| {
        menu.rebuild(window, cx, |menu, _, cx| fill(menu, &model, cx));
    });
    cx.on_release(move |_, _| drop(observation)).detach();
    menu
}

/// Electron's `window.confirm(message)`: the platform's message box with OK and Cancel;
/// `on_ok` runs on OK.
pub fn confirm(
    message: impl Into<SharedString>,
    window: &mut Window,
    cx: &mut App,
    on_ok: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    let message: SharedString = message.into();
    let answer = window.prompt(gpui_kit::PromptLevel::Info, &message, None, &["OK", "Cancel"], cx);
    window
        .spawn(cx, async move |cx| {
            if let Ok(0) = answer.await
                && let Err(error) = cx.update(|window, cx| on_ok(window, cx))
            {
                tracing::debug!(%error, "the window closed before the confirmation was handled");
            }
        })
        .detach();
}

/// Electron's `alert(message)`: the platform's message box with OK.
pub fn alert(message: impl Into<SharedString>, window: &mut Window, cx: &mut App) {
    let message: SharedString = message.into();
    let answer = window.prompt(gpui_kit::PromptLevel::Info, &message, None, &["OK"], cx);
    window
        .spawn(cx, async move |_| match answer.await {
            Ok(_) | Err(_) => {} // dismissed
        })
        .detach();
}

/// `if (result.status === Failure) alert(result.error)`: once `outcome` resolves, its
/// failure is alerted.
pub fn alert_failure(outcome: gpui_kit::Task<Result<(), String>>, window: &mut Window, cx: &mut App) {
    window
        .spawn(cx, async move |cx| {
            if let Err(message) = outcome.await
                && let Err(error) = cx.update(|window, cx| alert(message.clone(), window, cx))
            {
                tracing::warn!(%error, %message, "the window closed before the failure could be shown");
            }
        })
        .detach();
}

/// The app's `::-webkit-scrollbar` (10px gutter, transparent track, a border-colour thumb
/// with 5px radius) for a scroll area `viewport` tall showing `content` of height, scrolled
/// as `handle` says. Place it absolutely over the area's right edge; the area reserves
/// the 10px.
pub fn scrollbar_thumb(handle: &gpui_kit::ScrollHandle, viewport: f32, content: f32, cx: &App) -> Div {
    let thumb = (viewport * viewport / content).max(20.).min(viewport);
    let max_offset = (content - viewport).max(1.);
    let offset = (-f32::from(handle.offset().y)).clamp(0., max_offset);
    let top = offset / max_offset * (viewport - thumb);
    div()
        .absolute()
        .right_0()
        .top(px(top))
        .w(px(10.))
        .h(px(thumb))
        .rounded(px(5.))
        .bg(colors(cx).border)
}

// --- panes ---

/// Chromium's `font-family: monospace` on macOS.
pub const MONO: &str = "Menlo";

/// Chromium's `line-height: normal` for a font size (system font and Menlo alike): 14px at
/// 12px, 15px at 13px, 13px at 11px.
pub fn normal_line(size: f32) -> gpui_kit::Pixels {
    px((size * 1.2).floor())
}

/// `.ssh-pane-tab`: secondary text that turns primary on hover, over a 2px bottom rule in the
/// accent colour when `active`. `sub` is the `.ssh-pane-subtabs` size (11px, 6px 12px)
/// instead of 12px, 8px 16px.
pub fn pane_tab(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    active: bool,
    sub: bool,
    cx: &App,
) -> Stateful<Div> {
    let c = colors(cx);
    let primary = c.text_primary;
    pane_button(id, label, sub, if active { primary } else { c.text_secondary })
        .when(active, |d| d.border_color(c.accent))
        .hover(move |d| d.text_color(primary))
}

/// A `.ssh-pane-tab` used as a button, coloured by an inline `style={{ color }}` (Retry,
/// Remove, Restart…) that wins over its hover colour.
pub fn pane_button(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    sub: bool,
    color: impl Into<Hsla>,
) -> Stateful<Div> {
    let label: gpui_kit::SharedString = label.into();
    // Chromium's `line-height: normal` for the system font: 14px at 12px, 13px at 11px.
    let (size, line, x, y) = if sub { (SMALL, 13., 12., 6.) } else { (12., 14., 16., 8.) };
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .px(px(x))
        .pt(px(y))
        .pb(px(y))
        .border_b_2()
        .border_color(gpui_kit::transparent_black())
        .text_size(px(size))
        .line_height(px(line))
        .whitespace_nowrap()
        .text_color(color.into())
        .cursor_pointer()
        .child(label)
}

/// `.ssh-pane-tabs`: a row of [`pane_tab`]s over a 1px rule.
pub fn pane_tabs(cx: &App) -> Div {
    div().flex().flex_shrink_0().border_b_1().border_color(colors(cx).border)
}

/// `.ssh-pane-subtabs`: [`pane_tabs`] inset 4px (SSH sub-tabs; a Web App's or remote
/// Browser's Browser / Shell / Port Forward).
pub fn pane_subtabs(cx: &App) -> Div {
    pane_tabs(cx).pl(px(4.))
}

/// How a CSS `<input>` / `<textarea>` / `<select>` is drawn.
#[derive(Clone, Copy)]
pub struct FieldStyle {
    pub bg: Hsla,
    pub border: Hsla,
    pub radius: f32,
    /// `padding: y x`.
    pub pad: (f32, f32),
    pub size: f32,
    pub mono: bool,
    /// The border-box height.
    pub height: f32,
}

/// A text field drawn as its CSS: background, 1px border that turns accent while focused,
/// radius, padding and font. The layout around it sets its width.
pub fn field(
    state: &gpui_kit::Entity<gpui_kit::component::input::InputState>,
    style: FieldStyle,
    window: &Window,
    cx: &App,
) -> gpui_kit::component::input::Input {
    let focused = gpui_kit::Focusable::focus_handle(state.read(cx), cx).is_focused(window);
    let input = gpui_kit::component::input::Input::new(state).appearance(false);
    Styled::h(input, px(style.height))
        .w_full()
        .px(px(style.pad.1))
        .py(px(0.))
        .bg(style.bg)
        .border_1()
        .border_color(if focused { colors(cx).accent } else { style.border })
        .rounded(px(style.radius))
        .text_size(px(style.size))
        .line_height(normal_line(style.size))
        .when(style.mono, |d| d.font_family(MONO))
}

/// [`field`] for a multi-line `<textarea>` `style.height` high.
pub fn pane_text_area(
    state: &gpui_kit::Entity<gpui_kit::component::input::TextareaState>,
    style: FieldStyle,
    window: &Window,
    cx: &App,
) -> gpui_kit::component::input::Textarea {
    let focused = gpui_kit::Focusable::focus_handle(state.read(cx), cx).is_focused(window);
    gpui_kit::component::input::Textarea::new(state)
        .appearance(false)
        // XSmall: the editor pads itself 4px left and right, nothing above or below.
        .xsmall()
        .h(px(style.height))
        .w_full()
        .px(px(style.pad.1 - 4.))
        .py(px(style.pad.0))
        .bg(style.bg)
        .border_1()
        .border_color(if focused { colors(cx).accent } else { style.border })
        .rounded(px(style.radius))
        .text_size(px(style.size))
        .line_height(normal_line(style.size))
        .when(style.mono, |d| d.font_family(MONO))
}

/// A `<select>` drawn as its CSS (background, border, padding) with Chromium's chevron;
/// a [`Trigger`] for the dropdown menu of its options.
pub fn pane_select_box(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    style: FieldStyle,
    disabled: bool,
    cx: &App,
) -> Trigger {
    let label: gpui_kit::SharedString = label.into();
    let fg = colors(cx).text_primary;
    Trigger::new(
        div()
            .id(id)
            .h(px(style.height))
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(4.))
            // Chromium insets a menulist's text 4px more than its padding.
            .pl(px(style.pad.1 + 4.))
            // Chromium draws the arrow 5px from the right edge, over the padding.
            .pr(px(5.))
            .bg(style.bg)
            .border_1()
            .border_color(style.border)
            .rounded(px(style.radius))
            .text_size(px(style.size))
            .text_color(fg)
            .when(disabled, |d| d.opacity(0.5))
            .when(!disabled, |d| d.cursor_pointer())
            .child(div().flex_1().whitespace_nowrap().child(label))
            .child(icon(IconName::ChevronDown, style.size, fg)),
    )
}

/// An unstyled `<select>` as Chromium draws it (Arial 13.33px, black on white, grey
/// border), `width` wide (Chromium fits its widest option); a [`Trigger`] for its options.
pub fn native_select(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    width: f32,
    disabled: bool,
) -> Trigger {
    let label: gpui_kit::SharedString = label.into();
    Trigger::new(
        div()
            .id(id)
            .w(px(width))
            .h(px(19.))
            .flex()
            .flex_shrink_0()
            .items_center()
            .pl(px(4.))
            .pr(px(4.))
            .bg(gpui_kit::white())
            .border_1()
            .border_color(rgb(0x767676))
            .rounded(px(2.))
            .font_family("Arial")
            .text_size(px(13.333))
            .line_height(px(15.))
            .text_color(gpui_kit::black())
            .when(disabled, |d| d.opacity(0.5))
            .when(!disabled, |d| d.cursor_pointer())
            .child(div().flex_1().whitespace_nowrap().child(label))
            .child(icon(IconName::ChevronDown, 12., gpui_kit::black())),
    )
}

/// `.dialog-btn`'s variants.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum DialogButton {
    /// No variant: Chromium's default button face.
    Plain,
    /// `.cancel` / `.secondary`.
    Secondary,
    /// `.create` / `.primary`.
    Primary,
}

/// `.dialog-btn`: 13px text, 8px 16px padding, rounded 4px, borderless; 50% when disabled.
pub fn pane_dialog_button(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    kind: DialogButton,
    disabled: bool,
    cx: &App,
) -> Stateful<Div> {
    let label: gpui_kit::SharedString = label.into();
    let c = colors(cx);
    let (bg, fg, hover): (Hsla, Hsla, Hsla) = match kind {
        DialogButton::Plain => (rgb(0xefefef).into(), rgb(0x000000).into(), rgb(0xe5e5e5).into()),
        DialogButton::Secondary => (c.bg_tertiary, c.text_primary, c.bg_hover),
        DialogButton::Primary => (c.accent, gpui_kit::white(), rgb(0x006cbf).into()),
    };
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(16.))
        .py(px(8.))
        .rounded(px(4.))
        .bg(bg)
        .text_size(px(BODY))
        .line_height(normal_line(BODY))
        .text_color(fg)
        .whitespace_nowrap()
        .when(disabled, |d| d.opacity(0.5))
        .when(!disabled, |d| d.cursor_pointer().hover(move |d| d.bg(hover)))
        .child(label)
}

/// A bordered text button as Electron's ad-hoc CSS draws them (`.tty-list-refresh-btn`,
/// `.comments-push-btn`, …): font size, `padding: y x`, colours, radius. Callers add the
/// hover style (one per element).
#[allow(clippy::too_many_arguments)]
pub fn text_button(
    id: impl Into<ElementId>,
    label: impl Into<gpui_kit::SharedString>,
    size: f32,
    pad: (f32, f32),
    fg: impl Into<Hsla>,
    bg: impl Into<Hsla>,
    border: impl Into<Hsla>,
    radius: f32,
) -> Stateful<Div> {
    let label: gpui_kit::SharedString = label.into();
    div()
        .id(id)
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_center()
        .px(px(pad.1))
        .py(px(pad.0))
        .border_1()
        .border_color(border.into())
        .rounded(px(radius))
        .bg(bg.into())
        .text_size(px(size))
        .line_height(normal_line(size))
        .text_color(fg.into())
        .whitespace_nowrap()
        .cursor_pointer()
        .child(label)
}

/// `.dialog-close`: an 18px "✕" / "×", secondary until hovered.
pub fn dialog_close(id: impl Into<ElementId>, glyph: &'static str, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover, primary) = (c.bg_hover, c.text_primary);
    div()
        .id(id)
        .flex_shrink_0()
        .px(px(8.))
        .py(px(4.))
        .rounded(px(4.))
        .text_size(px(18.))
        .text_color(c.text_secondary)
        .cursor_pointer()
        .hover(move |d| d.bg(hover).text_color(primary))
        .child(glyph)
}

/// `.dialog-overlay`: a window-wide 60% black backdrop with `content` centred; a click on
/// the backdrop calls `on_dismiss`. `content` should `.occlude()` so clicks inside it stay
/// there.
pub fn modal(
    window: &Window,
    content: impl IntoElement,
    on_dismiss: impl Fn(&mut Window, &mut App) + 'static,
) -> gpui_kit::AnyElement {
    let size = window.viewport_size();
    gpui_kit::deferred(
        gpui_kit::anchored()
            .position(gpui_kit::point(px(0.), px(0.)))
            .position_mode(gpui_kit::AnchoredPositionMode::Window)
            .child(
                div()
                    .id("dialog-overlay")
                    .w(size.width)
                    .h(size.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(gpui_kit::rgba(0x00000099))
                    .occlude()
                    .on_mouse_down(gpui_kit::MouseButton::Left, move |_, window, cx| on_dismiss(window, cx))
                    .child(content),
            ),
    )
    .with_priority(1)
    .into_any_element()
}

/// The shadow of Electron's dialogs (`box-shadow: 0 8px 32px rgba(0, 0, 0, 0.4)`).
pub fn dialog_shadow() -> Vec<gpui_kit::BoxShadow> {
    vec![gpui_kit::BoxShadow {
        color: gpui_kit::rgba(0x00000066).into(),
        offset: gpui_kit::point(px(0.), px(8.)),
        blur_radius: px(32.),
        spread_radius: px(0.),
        inset: false,
    }]
}

// --- files ---

/// `#f14c4c`: the Files and Editor tabs' error red (`.file-viewer-error`, `.file-tree-error`).
pub const FILE_ERROR: u32 = 0xf14c4c;

/// Width of Electron's `::-webkit-scrollbar`.
const SCROLLBAR: f32 = 10.;

/// Dragging the thumb of the [`scrollbar`] named `id`, grabbed `grab` below its top (known
/// once the drag starts; the drag's value, not its view, reaches the moves).
#[derive(Clone)]
struct ThumbDrag {
    id: ElementId,
    grab: std::rc::Rc<std::cell::Cell<gpui_kit::Pixels>>,
}

impl gpui_kit::Render for ThumbDrag {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

/// A vertically scrolling column with Electron's classic scrollbar (see [`scrollbar`]).
pub fn scroll_area(id: impl Into<ElementId>, handle: &gpui_kit::ScrollHandle, content: impl IntoElement, cx: &App) -> Div {
    let id: ElementId = id.into();
    div()
        .size_full()
        .min_h_0()
        .flex()
        .relative()
        .child(div().id(id.clone()).flex_1().min_w_0().h_full().overflow_y_scroll().track_scroll(handle).child(content))
        .children(scrollbar(id, handle, cx))
}

/// Electron's classic scrollbar (`::-webkit-scrollbar`: 10px, a `#3c3c3c` thumb rounded 5px
/// on a clear track, `--bg-hover` under the pointer) for what `handle` scrolls vertically,
/// to follow it in a row. While there is something to scroll, the bar takes its 10px from
/// the content, as Chromium's does; its thumb can be dragged.
pub fn scrollbar<H: gpui_kit::base::ScrollbarHandle + Clone>(id: impl Into<ElementId>, handle: &H, cx: &App) -> Vec<gpui_kit::AnyElement> {
    use gpui_kit::{Bounds, DragMoveEvent, point, px as pixels};
    /// What the bar shows of `handle`: nothing when there is nothing to scroll, else the
    /// height shown, how far it scrolls and how far it is scrolled.
    fn extent(handle: &impl gpui_kit::base::ScrollbarHandle) -> Option<(f32, f32, f32)> {
        let viewport = f32::from(handle.viewport_bounds().size.height);
        let max = f32::from(handle.content_size().height) - viewport;
        (max > 0. && viewport > 0.).then(|| (viewport, max, -f32::from(handle.offset().y)))
    }
    let id: ElementId = id.into();
    let shown = extent(handle);
    // The bar is built from the content as it was last laid out; how it is now is only
    // known once the content is laid out again (before the bar in its row). When the two
    // differ (rows came or went, the bar itself took its 10px), ask for another frame. A
    // refresh asked for while drawing is dropped, so it is asked for from the next frame.
    let probe = {
        let handle = handle.clone();
        gpui_kit::canvas(
            move |_: Bounds<gpui_kit::Pixels>, window, _| {
                if extent(&handle) != shown {
                    window.on_next_frame(|window, _| window.refresh());
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    };
    let bar = shown.map(|(viewport, max, scrolled)| {
        let content = viewport + max;
        let thumb = (viewport * viewport / content).max(20.);
        let travel = viewport - thumb;
        let top = travel * (scrolled / max).clamp(0., 1.);
        let c = colors(cx);
        let (thumb_color, hover) = (c.border, c.bg_hover);
        let (drag_handle, drag_id, thumb_id) = (handle.clone(), id.clone(), id);
        div()
            .w(pixels(SCROLLBAR))
            .h_full()
            .flex_shrink_0()
            .relative()
            .on_drag_move(move |event: &DragMoveEvent<ThumbDrag>, window, cx| {
                let drag = event.drag(cx);
                if drag.id != drag_id || travel <= 0. {
                    return;
                }
                let at = f32::from(event.event.position.y - event.bounds.top() - drag.grab.get()).clamp(0., travel);
                drag_handle.set_offset(point(drag_handle.offset().x, pixels(-(max * at / travel))));
                window.refresh();
            })
            .child(
                div()
                    .id((thumb_id.clone(), "thumb"))
                    .absolute()
                    .top(pixels(top))
                    .w_full()
                    .h(pixels(thumb))
                    .rounded(pixels(5.))
                    .bg(thumb_color)
                    .hover(move |d| d.bg(hover))
                    .on_drag(ThumbDrag { id: thumb_id, grab: Default::default() }, |drag, grab, _, cx| {
                        drag.grab.set(grab.y);
                        cx.new(|_| drag.clone())
                    }),
            )
            .into_any_element()
    });
    let mut parts = vec![probe.into_any_element()];
    parts.extend(bar);
    parts
}

/// Lucide's star, filled (`<Star fill="currentColor" />`); `filled: false` is the outline.
pub fn star_icon(size: f32, filled: bool) -> Icon {
    const FILLED: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="black" stroke="black" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/></svg>"#;
    let icon = if filled { Icon::default().data(FILLED) } else { Icon::new(IconName::Star) };
    icon.size(px(size))
}

/// The key context of [`key_button`], [`ringless_key_button`] and [`key_checkbox`].
const KEY_CONTROL: &str = "KeyControl";
/// The key context of [`key_select`].
const KEY_SELECT: &str = "KeySelect";

/// Enter and Space reach a focused [`key_button`] or [`key_checkbox`]: GPUI runs key
/// bindings before an element's own keys, so a GPUI Kit dialog's Enter (`Confirm`) would
/// otherwise take the press, as a `<button>` in an Electron dialog takes it.
///
/// A focused [`key_select`] opens its menu on ArrowUp and ArrowDown (the menu's popover
/// opens on `Confirm`, which it binds to Space too) and ignores Enter, as Chromium's
/// `<select>` does on macOS.
pub fn bind_keys(cx: &mut App) {
    use gpui_kit::base::actions::Confirm;
    cx.bind_keys([
        KeyBinding::new("enter", NoAction, Some(KEY_CONTROL)),
        KeyBinding::new("space", NoAction, Some(KEY_CONTROL)),
        KeyBinding::new("enter", NoAction, Some(KEY_SELECT)),
        KeyBinding::new("up", Confirm { secondary: false }, Some(KEY_SELECT)),
        KeyBinding::new("down", Confirm { secondary: false }, Some(KEY_SELECT)),
    ]);
}

/// The focus handle kept for the element `id` while it is drawn, as a `<button>`'s own.
pub fn keyed_focus(id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> FocusHandle {
    window.use_keyed_state(id.into(), cx, |_, cx| cx.focus_handle()).read(cx).clone()
}

/// Whether the focus is on a [`key_button`], [`key_checkbox`] or [`key_select`]: a control
/// whose Enter no binding takes, so it reaches the raw key listeners.
fn key_control_focused(window: &Window) -> bool {
    window.context_stack().last().is_some_and(|context| context.contains(KEY_CONTROL) || context.contains(KEY_SELECT))
}

/// What a dialog did with an Enter ([`dialog_enter`]).
pub enum Entered {
    /// The dialog acted on it (it closes, or its controls are disabled): the key goes no
    /// further.
    Taken,
    /// The focused control goes on with it: a textarea breaks the line, a button presses
    /// itself.
    PassedOn,
}

/// Electron's `onKeyDown` on a dialog's root, for Enter: `enter` runs on Enter, with any
/// modifiers, in a text field or textarea within `root` or on a focused [`key_button`],
/// [`key_checkbox`] or [`key_select_box`], before the control takes the key. A checkbox
/// takes a passed-on Enter as a click that callers ignore ([`pressed_enter`]), as a native
/// checkbox ignores it. Elsewhere in a GPUI Kit dialog (no control focused) Enter is the
/// dialog's own `Confirm` (`on_ok`).
pub fn dialog_enter<V: 'static>(
    root: Div,
    cx: &mut Context<V>,
    enter: impl Fn(&mut V, &mut Window, &mut Context<V>) -> Entered + 'static,
) -> Div {
    let enter = std::rc::Rc::new(enter);
    let in_field = {
        let enter = enter.clone();
        // The fields' own `Enter` (any modifiers), caught on its way down to the field.
        cx.listener(move |view, _: &gpui_kit::component::input::Enter, window, cx| {
            if let Entered::Taken = enter(view, window, cx) {
                cx.stop_propagation();
            }
        })
    };
    let on_control = cx.listener(move |view, event: &gpui_kit::KeyDownEvent, window, cx| {
        if event.keystroke.key == "enter"
            && key_control_focused(window)
            && let Entered::Taken = enter(view, window, cx)
        {
            cx.stop_propagation();
        }
    });
    root.capture_action(in_field).capture_key_down(on_control)
}

/// A focus handle kept for the element `id`, and whether it shows Chromium's focus ring
/// (`:focus-visible`: focused from the keyboard).
fn key_focus(id: &ElementId, window: &mut Window, cx: &mut App) -> (FocusHandle, bool) {
    let focus = keyed_focus(id.clone(), window, cx);
    let ring = keyboard_focused(&focus, window);
    (focus, ring)
}

/// Whether `focus` is focused from the keyboard (`:focus-visible`).
fn keyboard_focused(focus: &FocusHandle, window: &Window) -> bool {
    focus.is_focused(window) && window.last_input_was_keyboard()
}

/// `control`, which `focus` focuses, with Chromium's focus ring (rounded `radius`) while it
/// is focused from the keyboard.
pub fn with_focus_ring<E: ParentElement + FluentBuilder>(
    control: E,
    focus: &FocusHandle,
    radius: f32,
    window: &Window,
) -> E {
    control.when(keyboard_focused(focus, window), |control| control.child(focus_outline(radius)))
}

/// A `<textarea>` around `textarea`: Tab moves the focus on and Shift+Tab back, as Chromium's
/// do, where GPUI Kit's textarea would indent the text.
pub fn tabbing_text_area(textarea: impl IntoElement) -> Div {
    use gpui_kit::component::input::{IndentInline, OutdentInline};
    div()
        .capture_action(|_: &IndentInline, window, cx| move_focus("root::Tab", window, cx))
        .capture_action(|_: &OutdentInline, window, cx| move_focus("root::TabPrev", window, cx))
        .child(textarea)
}

/// Run GPUI Kit's root focus move `action` (Tab's or Shift+Tab's, which keep the focus in an
/// open dialog) in place of the action being dispatched.
fn move_focus(action: &str, window: &mut Window, cx: &mut App) {
    cx.stop_propagation();
    let action = cx.build_action(action, None).unwrap_or_else(|error| panic!("GPUI Kit's {action}: {error}"));
    window.dispatch_action(action, cx);
}

/// A borderless `<button>`, rounded `radius`: Tab reaches it and Enter or Space presses it.
/// Focused from the keyboard (`:focus-visible`), it shows Chromium's focus ring. Callers add
/// its look, and an accessibility label when its content does not name it.
pub fn key_button(id: impl Into<ElementId>, radius: f32, window: &mut Window, cx: &mut App) -> gpui_kit::base::Button {
    let id = id.into();
    let focus = keyed_focus(id.clone(), window, cx);
    with_focus_ring(ringless_key_button(id, &focus, radius), &focus, radius, window)
}

/// A [`key_button`] whose CSS replaces the focus ring (`:focus { outline: none; ... }`): it
/// draws none, and the caller styles it while `focus` is focused. A `<button>` takes the focus
/// on a click as well as from Tab, so that style shows after either.
pub fn ringless_key_button(id: impl Into<ElementId>, focus: &FocusHandle, radius: f32) -> gpui_kit::base::Button {
    gpui_kit::base::Button::new(id)
        .track_focus(focus)
        .key_context(KEY_CONTROL)
        // The line box the button inherits (the base button sets the font size's).
        .line_height(relative(1.2))
        .rounded(px(radius))
}

/// A native `<input type="checkbox">` of `size` px ([`checkbox_box`]) with its `<label>`:
/// Tab reaches it unless `disabled`, a click anywhere on it or Space toggles it. Focused from
/// the keyboard, the box shows Chromium's focus ring. Callers add the label after the box,
/// the row's look and `on_change`. Enter clicks it too: a native checkbox ignores Enter, so
/// callers check [`pressed_enter`].
pub fn key_checkbox(
    id: impl Into<ElementId>,
    checked: bool,
    disabled: bool,
    size: f32,
    window: &mut Window,
    cx: &mut App,
) -> gpui_kit::base::Checkbox {
    let id = id.into();
    let (focus, ring) = key_focus(&id, window, cx);
    let radius = (size / 6.).round();
    gpui_kit::base::Checkbox::new(id)
        .track_focus(&focus)
        .key_context(KEY_CONTROL)
        .checked(checked)
        .disabled(disabled)
        .child(checkbox_box(checked, disabled, size).when(ring, |b| b.child(focus_outline(radius))))
}

/// Whether a click came from Enter on the focused control.
pub fn pressed_enter(event: &ClickEvent) -> bool {
    matches!(event, ClickEvent::Keyboard(KeyboardClickEvent { button: KeyboardButton::Enter, .. }))
}

/// Chromium's focus ring on a borderless control: 2px of macOS orange across its edge, a
/// 1px white halo outside that (as `review_view::parts::focus_ring` draws on a border).
fn focus_outline(radius: f32) -> Div {
    let edge = || div().absolute().top(px(0.)).left(px(0.)).right(px(0.)).bottom(px(0.));
    div()
        .absolute()
        .top(px(-2.))
        .left(px(-2.))
        .right(px(-2.))
        .bottom(px(-2.))
        .rounded(px(radius + 2.))
        .border_1()
        .border_color(gpui_kit::white())
        .child(edge().rounded(px(radius + 1.)).border_2().border_color(rgb(0xe59700)))
}

/// `.file-viewer-open-btn` / `.file-viewer-toggle-btn`: 11px text, 4px 10px padding, 1px
/// border, tertiary fill; hover fills and shows the accent border. A disabled one is faded
/// (`opacity`: 0.6 while a download runs).
pub fn viewer_button(id: impl Into<ElementId>, disabled: Option<f32>, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover_bg, accent) = (c.bg_hover, c.accent);
    let button = div()
        .id(id)
        .flex_shrink_0()
        .flex()
        .items_center()
        .px(px(10.))
        .py(px(4.))
        .rounded(px(4.))
        .border_1()
        .border_color(c.border)
        .bg(c.bg_tertiary)
        .text_size(px(SMALL))
        .text_color(c.text_primary)
        .whitespace_nowrap();
    match disabled {
        Some(opacity) => button.opacity(opacity),
        None => button.cursor_pointer().hover(move |d| d.bg(hover_bg).border_color(accent)),
    }
}

/// `.file-editor-save-btn` / `.file-editor-toggle-btn`: as [`viewer_button`] at 12px with
/// 12px side padding; disabled at half opacity, with the not-allowed cursor.
pub fn editor_button(id: impl Into<ElementId>, disabled: bool, cx: &App) -> Stateful<Div> {
    viewer_button(id, disabled.then_some(0.5), cx)
        .px(px(12.))
        .text_size(px(12.))
        .when(disabled, |d| d.cursor_not_allowed())
}

/// `.file-viewer-language` / `.file-editor-language`: the language, a small tertiary pill.
pub fn language_pill(text: impl Into<gpui_kit::SharedString>, cx: &App) -> Div {
    let c = colors(cx);
    let text: gpui_kit::SharedString = text.into();
    div()
        .flex_shrink_0()
        .px(px(8.))
        .py(px(2.))
        .rounded(px(4.))
        .bg(c.bg_tertiary)
        .text_size(px(SMALL))
        .text_color(c.text_secondary)
        .whitespace_nowrap()
        .child(text)
}

/// `.diff-find-btn`: a bare 2px-padded icon button, secondary until hovered; disabled at
/// 0.4 opacity.
pub fn find_button(id: impl Into<ElementId>, name: IconName, disabled: bool, cx: &App) -> Stateful<Div> {
    let c = colors(cx);
    let (hover_bg, primary) = (c.bg_hover, c.text_primary);
    let button = div()
        .id(id)
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .p(px(2.))
        .rounded(px(3.))
        .text_color(c.text_secondary)
        .child(Icon::new(name).size(px(14.)));
    if disabled {
        button.opacity(0.4)
    } else {
        button.cursor_pointer().hover(move |d| d.bg(hover_bg).text_color(primary))
    }
}

/// `.file-viewer-placeholder` / `.file-editor-placeholder`: centred secondary 14px text
/// under 40px of space.
pub fn viewer_placeholder(text: impl Into<gpui_kit::SharedString>, cx: &App) -> Div {
    let text: gpui_kit::SharedString = text.into();
    div()
        .w_full()
        .px(px(20.))
        .py(px(40.))
        .flex()
        .justify_center()
        .text_size(px(14.))
        .text_color(colors(cx).text_secondary)
        .child(text)
}

/// Chromium's broken image, for an `<img>` it cannot decode: the image-off icon and the `alt`
/// text, in the 16px text the image inherits, wrapped to the width the image may take.
pub fn broken_image(alt: String, cx: &App) -> impl Fn() -> gpui_kit::AnyElement + 'static {
    let (icon_color, text_color) = (colors(cx).text_secondary, colors(cx).text_primary);
    move || {
        gpui_kit::component::h_flex()
            .gap(px(4.))
            .child(icon(IconName::ImageOff, 16., icon_color).flex_shrink_0())
            .child(div().min_w_0().text_size(px(16.)).text_color(text_color).child(alt.clone()))
            .into_any_element()
    }
}

/// `.file-viewer-error`: centred red 14px text in 20px of padding.
pub fn viewer_error(child: impl IntoElement) -> Div {
    div()
        .w_full()
        .p(px(20.))
        .flex()
        .flex_col()
        .items_center()
        .text_size(px(14.))
        .text_color(rgb(FILE_ERROR))
        .child(child)
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::component::menu::DropdownMenu as _;
    use gpui_kit::{Context, KeyDownEvent, KeyUpEvent, Keystroke, Render, TestAppContext, VisualTestContext};

    use super::*;

    gpui_kit::actions!(ui_test, [Submit]);

    /// A key button counting its presses, a key checkbox and a disabled one, in a context
    /// that binds Enter (as a GPUI Kit dialog binds it to `Confirm`).
    struct Controls {
        presses: Rc<Cell<usize>>,
        submits: Rc<Cell<usize>>,
        checked: bool,
        enters: usize,
    }

    impl Render for Controls {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (presses, submits) = (self.presses.clone(), self.submits.clone());
            let toggle = cx.listener(|controls, event: &ClickEvent, _, cx| {
                if pressed_enter(event) {
                    controls.enters += 1;
                } else {
                    controls.checked = !controls.checked;
                }
                cx.notify();
            });
            div()
                .key_context("Outer")
                .on_action(move |_: &Submit, _, _| submits.set(submits.get() + 1))
                .child(key_button("pressed", 3., window, cx).child("x").on_click(move |_, _, _| presses.set(presses.get() + 1)))
                .child(
                    key_checkbox("check", self.checked, false, 13., window, cx)
                        .child("Check")
                        .on_change(move |_, event, window, cx| toggle(event, window, cx)),
                )
                .child(key_checkbox("disabled", false, true, 13., window, cx).child("Disabled"))
        }
    }

    fn press(key: &str, cx: &mut VisualTestContext) {
        let keystroke = cx.update(|_, cx| Keystroke::parse_with_os(key, cx.platform_info().os).unwrap());
        cx.simulate_event(KeyDownEvent { keystroke: keystroke.clone(), is_held: false, prefer_character_input: false });
        cx.simulate_event(KeyUpEvent { keystroke });
    }

    fn focus_next(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.focus_next(cx);
            assert!(window.focused(cx).is_some(), "not a tab stop");
            window.draw(cx).clear(cx);
        });
    }

    /// Tab visits the button and the enabled checkbox; Enter and Space press the button even
    /// where an ancestor binds Enter; Space toggles the checkbox, whose Enter callers see.
    #[gpui_kit::test]
    fn tab_reaches_key_controls_and_enter_or_space_presses_them(cx: &mut TestAppContext) {
        let (presses, submits) = (Rc::new(Cell::new(0)), Rc::new(Cell::new(0)));
        let (controls, cx) = cx.add_window_view({
            let (presses, submits) = (presses.clone(), submits.clone());
            move |_, _| Controls { presses, submits, checked: false, enters: 0 }
        });
        cx.update(|window, cx| {
            bind_keys(cx);
            cx.bind_keys([KeyBinding::new("enter", Submit, Some("Outer"))]);
            window.draw(cx).clear(cx);
        });

        focus_next(cx);
        press("enter", cx);
        press("space", cx);
        assert_eq!((presses.get(), submits.get()), (2, 0));

        focus_next(cx);
        press("space", cx);
        press("enter", cx);
        controls.read_with(cx, |controls, _| assert_eq!((controls.checked, controls.enters), (true, 1)));
        assert_eq!(submits.get(), 0);

        // The disabled checkbox is skipped: back to the button.
        focus_next(cx);
        press("space", cx);
        assert_eq!(presses.get(), 3);
    }

    /// A dialog body ([`dialog_enter`]) with a text field, a textarea, a key button, a key
    /// checkbox and a key select, counting what each did.
    struct EnterHost {
        field: Entity<gpui_kit::component::input::InputState>,
        area: Entity<gpui_kit::component::input::TextareaState>,
        /// Whether the dialog takes Enter.
        take: bool,
        enters: usize,
        presses: usize,
        toggles: usize,
        /// How many times the select's menu was built (opened).
        menus: Rc<Cell<usize>>,
    }

    impl Render for EnterHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let menus = self.menus.clone();
            let select = key_select_box("select", "Pick", &keyed_focus("select", window, cx), cx);
            let toggle = cx.listener(|host, event: &ClickEvent, _, _| {
                if !pressed_enter(event) {
                    host.toggles += 1;
                }
            });
            let enter = |host: &mut Self, _: &mut Window, _: &mut Context<Self>| {
                host.enters += 1;
                if host.take { Entered::Taken } else { Entered::PassedOn }
            };
            dialog_enter(div(), cx, enter)
                .child(text_input(&self.field, cx))
                .child(text_area(&self.area, cx))
                .child(key_button("press", 0., window, cx).child("Press").on_click(cx.listener(|host, _, _, _| {
                    host.presses += 1;
                })))
                .child(
                    key_checkbox("check", false, false, 13., window, cx)
                        .child("Check")
                        .on_change(move |_, event, window, cx| toggle(event, window, cx)),
                )
                .child(Trigger::new(select).dropdown_menu(move |menu, _, _| {
                    menus.set(menus.get() + 1);
                    menu.item(menu_item("One"))
                }))
        }
    }

    /// Enter in a field or on a focused control reaches the dialog first, with any modifiers;
    /// when it takes the key the control does nothing more, otherwise the textarea breaks the
    /// line and the button presses itself. The checkbox ignores Enter and Space toggles it;
    /// Space, Up or Down open the select's menu, Enter does not.
    #[gpui_kit::test]
    fn enter_reaches_the_dialog_before_the_focused_control(cx: &mut TestAppContext) {
        let menus = Rc::new(Cell::new(0));
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            bind_keys(cx);
        });
        let (host, cx) = cx.add_window_view({
            let menus = menus.clone();
            move |window, cx| EnterHost {
                field: cx.new(|cx| gpui_kit::component::input::InputState::new(window, cx)),
                area: cx.new(|cx| gpui_kit::component::input::TextareaState::new(window, cx)),
                take: false,
                enters: 0,
                presses: 0,
                toggles: 0,
                menus,
            }
        });
        cx.update(|window, cx| {
            window.activate_window();
            window.draw(cx).clear(cx);
        });
        let take = |take: bool, cx: &mut VisualTestContext| host.update(cx, |host, _| host.take = take);
        let counts = |cx: &mut VisualTestContext| {
            host.read_with(cx, |host, _| (host.enters, host.presses, host.toggles))
        };
        let area = |cx: &mut VisualTestContext| host.read_with(cx, |host, cx| host.area.read(cx).value().to_string());

        // The text field.
        focus_next(cx);
        press("enter", cx);
        press("shift-enter", cx);
        assert_eq!(counts(cx), (2, 0, 0));

        // The textarea: a line break unless the dialog takes the key.
        focus_next(cx);
        press("enter", cx);
        press("shift-enter", cx);
        assert_eq!((counts(cx), area(cx)), ((4, 0, 0), "\n\n".to_owned()));
        take(true, cx);
        press("enter", cx);
        press("shift-enter", cx);
        assert_eq!((counts(cx), area(cx)), ((6, 0, 0), "\n\n".to_owned()));

        // The button presses itself unless the dialog takes the key.
        focus_next(cx);
        press("enter", cx);
        take(false, cx);
        press("enter", cx);
        press("shift-enter", cx);
        press("space", cx);
        assert_eq!(counts(cx), (9, 2, 0));

        // The checkbox.
        focus_next(cx);
        press("enter", cx);
        press("space", cx);
        assert_eq!(counts(cx), (10, 2, 1));

        // The select.
        focus_next(cx);
        press("enter", cx);
        draw(cx);
        assert_eq!((counts(cx), menus.get()), ((11, 2, 1), 0));
        press("space", cx);
        draw(cx);
        assert_eq!(menus.get(), 1, "Space opens the menu");
        assert_eq!(counts(cx), (11, 2, 1));
    }

    #[gpui_kit::test]
    fn up_or_down_opens_a_focused_select(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
            bind_keys(cx);
        });
        for key in ["up", "down"] {
            let menus = Rc::new(Cell::new(0));
            let (_, cx) = cx.add_window_view({
                let menus = menus.clone();
                move |_, _| SelectHost { menus }
            });
            cx.update(|window, cx| window.draw(cx).clear(cx));
            focus_next(cx);
            press(key, cx);
            draw(cx);
            assert_eq!(menus.get(), 1, "{key}");
        }
    }

    /// A lone [`key_select_box`] counting its menu's openings.
    struct SelectHost {
        menus: Rc<Cell<usize>>,
    }

    impl Render for SelectHost {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let menus = self.menus.clone();
            let select = key_select_box("select", "Pick", &keyed_focus("select", window, cx), cx);
            div().child(Trigger::new(select).dropdown_menu(move |menu, _, _| {
                menus.set(menus.get() + 1);
                menu.item(menu_item("One"))
            }))
        }
    }

    fn draw(cx: &mut VisualTestContext) {
        cx.run_until_parked();
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.run_until_parked();
    }

    struct Model;

    struct Empty;

    impl Render for Empty {
        fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
            div()
        }
    }

    fn menu_window(cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(gpui_kit::init);
        cx.add_window_view(|_, _| Empty).1
    }

    /// GPUI releases the entities whose last handle was dropped when it flushes an update's
    /// effects.
    fn release_dropped(cx: &mut VisualTestContext) {
        cx.update(|_, _| {});
    }

    /// A menu following `model`, whose fill counts its calls in `fills`.
    fn following_menu(model: &Entity<Model>, fills: &Rc<Cell<usize>>, cx: &mut VisualTestContext) -> Entity<PopupMenu> {
        let (model, fills) = (model.clone(), fills.clone());
        let menu = cx.update(|window, cx| {
            PopupMenu::build(window, cx, move |menu, window, cx| {
                menu_following(menu, &model, window, cx, move |menu, _, _| {
                    fills.set(fills.get() + 1);
                    menu
                })
            })
        });
        cx.run_until_parked();
        menu
    }

    /// The open menu is filled again on each notify of its model, and the observation ends
    /// with the menu: what the fill holds is dropped at once, not at the model's next notify.
    #[gpui_kit::test]
    fn a_following_menu_refills_on_notify_until_it_is_released(cx: &mut TestAppContext) {
        let cx = menu_window(cx);
        let model = cx.update(|_, cx| cx.new(|_| Model));
        let fills = Rc::new(Cell::new(0));
        let menu = following_menu(&model, &fills, cx);
        assert_eq!(fills.get(), 1, "filled when built");

        model.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(fills.get(), 2, "filled again");

        drop(menu);
        release_dropped(cx);
        assert_eq!(Rc::strong_count(&fills), 1, "the observation went with the menu");
        model.update(cx, |_, cx| cx.notify());
        cx.run_until_parked();
        assert_eq!(fills.get(), 2);
    }

    /// The observation does not keep the model alive: an open menu lets it go.
    #[gpui_kit::test]
    fn a_following_menu_does_not_keep_its_model_alive(cx: &mut TestAppContext) {
        let cx = menu_window(cx);
        let model = cx.update(|_, cx| cx.new(|_| Model));
        let fills = Rc::new(Cell::new(0));
        let menu = following_menu(&model, &fills, cx);
        let weak = model.downgrade();
        drop(model);
        release_dropped(cx);
        assert!(weak.upgrade().is_none(), "released while the menu is open");
        assert_eq!(Rc::strong_count(&fills), 1, "and the observation with it");
        assert_eq!(fills.get(), 1);
        drop(menu);
    }
}
