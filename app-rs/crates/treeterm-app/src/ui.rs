//! Building blocks shared by the views, sized and coloured as `src/renderer/styles/index.css`
//! draws them, so the native chrome matches Electron pixel for pixel.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{App, ClickEvent, Div, ElementId, Hsla, Stateful, Window, div, px, rgb};

use crate::theme::colors;

/// The UI font size most chrome text uses (`font-size: 11px`).
pub const SMALL: f32 = 11.;
/// Tree rows, titles (`font-size: 13px`).
pub const BODY: f32 = 13.;

/// A Lucide icon (`<Name size={size} />`), drawn in `color`.
pub fn icon(name: IconName, size: f32, color: impl Into<Hsla>) -> Icon {
    Icon::new(name).size(px(size)).text_color(color.into())
}

/// A 24×24 square button body, rounded 4px; callers add colours and the hover style
/// (GPUI allows one hover style per element).
fn square_button(id: impl Into<ElementId>) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(24.))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(4.))
        .cursor_pointer()
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

/// A rounded pill (`.workspace-branch`, `.activity-state-badge`, …).
pub fn pill(text: impl Into<gpui_kit::SharedString>, fg: impl Into<Hsla>, bg: impl Into<Hsla>) -> Div {
    let text: gpui_kit::SharedString = text.into();
    div()
        .flex_shrink_0()
        .px(px(6.))
        .py(px(1.))
        .rounded(px(4.))
        .text_size(px(SMALL))
        .text_color(fg.into())
        .bg(bg.into())
        .child(text)
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

/// `ActivityIndicator`: the state's glyph in its `.activity-*` colour (idle keeps `idle`'s
/// colour, the text around it); working spins.
pub fn activity_glyph(
    state: treeterm_model::workspace_store::ActivityState,
    size: f32,
    idle: impl Into<Hsla>,
) -> gpui_kit::AnyElement {
    use treeterm_model::workspace_store::ActivityState;
    let (glyph, color): (&str, Hsla) = match state {
        ActivityState::Idle => ("○", idle.into()),
        ActivityState::Working => {
            use gpui_kit::component::Sizable as _;
            return gpui_kit::component::spinner::Spinner::new()
                .icon(Icon::new(IconName::LoaderCircle))
                .with_size(px(size))
                .color(rgb(0xe5e510).into())
                .into_any_element();
        }
        ActivityState::UserInputRequired => ("▶", rgb(0x00bcd4).into()),
        ActivityState::PermissionRequest => ("●", rgb(0xcd6600).into()),
        ActivityState::SafePermissionRequested => ("●", rgb(0x0dbc79).into()),
        ActivityState::Completed => ("✓", rgb(0x23d18b).into()),
        ActivityState::ApplicationError => ("✗", rgb(0xd16969).into()),
        ActivityState::Error => ("●", rgb(0xf44747).into()),
    };
    div().text_size(px(size)).text_color(color).child(glyph).into_any_element()
}

// --- dialogs ---
// Electron's modal dialogs (`.dialog-overlay` around `.settings-dialog`,
// `.create-child-dialog`, …) and their form controls, measured from its computed styles.

use gpui_kit::component::dialog::{Dialog, DialogEntrance};
use gpui_kit::component::input::{Input, InputState, Textarea, TextareaState};
use gpui_kit::component::menu::PopupMenuItem;
use gpui_kit::component::{Sizable as _, Size, WindowExt as _};
use gpui_kit::{Entity, FontWeight, SharedString, relative};

/// The dialog box: bg-secondary, 8px radius, no border or padding, centred, shown at once
/// (no entrance animation). Callers add [`dialog_header`], their content and
/// [`dialog_actions`] as children.
pub fn dialog_frame(dialog: Dialog, width: f32, cx: &App) -> Dialog {
    dialog
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
    let c = colors(cx);
    let (hover_bg, primary) = (c.bg_hover, c.text_primary);
    div()
        .flex()
        .flex_shrink_0()
        .items_center()
        .justify_between()
        .px(px(20.))
        .py(px(16.))
        .border_b_1()
        .border_color(c.border)
        .child(
            div()
                .text_size(px(16.))
                .line_height(px(19.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(title_color.into())
                .child(title.into()),
        )
        .child(
            div()
                .id("dialog-close")
                .px(px(8.))
                .py(px(4.))
                .rounded(px(4.))
                .text_size(px(18.))
                .line_height(px(21.))
                .text_color(c.text_secondary)
                .cursor_pointer()
                .hover(move |d| d.bg(hover_bg).text_color(primary))
                .child(close_glyph)
                .on_click(|_, window, cx| window.close_dialog(cx)),
        )
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
    let c = colors(cx);
    let (bg, fg, hover): (Hsla, Hsla, Hsla) = match kind {
        ButtonKind::Secondary => (c.bg_tertiary, c.text_primary, c.bg_hover),
        ButtonKind::Primary => (c.accent, gpui_kit::white(), rgb(0x006cbf).into()),
        ButtonKind::Warning => (rgb(0xb36200).into(), gpui_kit::white(), rgb(0xcc7000).into()),
    };
    let label: SharedString = label.into();
    div()
        .id(id)
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

/// A `.context-menu-item` for GPUI Kit's popup menu: 13px text on a 31.5px pitch (the menu
/// adds the 12px left padding and 2px between items itself).
pub fn menu_item(label: impl Into<SharedString>) -> PopupMenuItem {
    let label: SharedString = label.into();
    PopupMenuItem::element(move |_, _| div().h(px(29.5)).flex().items_center().text_size(px(BODY)).child(label.clone()))
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
