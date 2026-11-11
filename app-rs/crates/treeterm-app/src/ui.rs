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
