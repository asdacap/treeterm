//! Building blocks shared by the views, sized and coloured as `src/renderer/styles/index.css`
//! draws them, so the native chrome matches Electron pixel for pixel.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::prelude::*;
use gpui_kit::{
    Along as _, App, Axis, Bounds, ClickEvent, Div, ElementId, FocusHandle, Hsla, KeyBinding, KeyboardButton, KeyboardClickEvent,
    NoAction, Pixels, Point, Stateful, Window, div, px, rgb,
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
) -> StatusToggle {
    toggle_owned(id, on, label, on_click)
}

/// [`toggle`] with a computed label.
pub fn toggle_owned(
    id: impl Into<ElementId>,
    on: bool,
    label: impl Into<gpui_kit::SharedString>,
    on_click: impl Fn(bool, &mut Window, &mut App) + 'static,
) -> StatusToggle {
    StatusToggle { id: id.into(), on, label: label.into(), on_click: Box::new(on_click) }
}

/// What a [`StatusToggle`] does when clicked, given the state it switches to.
type OnToggle = dyn Fn(bool, &mut Window, &mut App);

/// `.terminal-toggle` ([`toggle`]). Switching it slides the knob and blends the track's
/// colour over 0.2s with CSS's default `ease` (`transition: background 0.2s` on the slider,
/// `transition: transform 0.2s` on its knob); it is drawn at rest when it first shows.
#[derive(IntoElement)]
pub struct StatusToggle {
    id: ElementId,
    on: bool,
    label: gpui_kit::SharedString,
    on_click: Box<OnToggle>,
}

/// The `.terminal-toggle-slider` track off (`#555`) and on (`#0dbc79`).
const TOGGLE_OFF: u32 = 0x555555;
const TOGGLE_ON: u32 = 0x0dbc79;

/// The toggle drawn `on_ness` of the way from off (0) to on (1): the track's colour, blended in
/// sRGB as CSS transitions blend colours, and the knob's left edge (2px off, moved 14px on).
fn toggle_at(on_ness: f32) -> (gpui_kit::Rgba, f32) {
    let (off, on) = (rgb(TOGGLE_OFF), rgb(TOGGLE_ON));
    let mix = |from: f32, to: f32| from * (1. - on_ness) + to * on_ness;
    let track = gpui_kit::Rgba { r: mix(off.r, on.r), g: mix(off.g, on.g), b: mix(off.b, on.b), a: mix(off.a, on.a) };
    (track, 2. + 14. * on_ness)
}

impl RenderOnce for StatusToggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        use gpui_kit::base::motion::{Easing, Transition, transition};
        let target = if self.on { 1. } else { 0. };
        let policy = Transition::new(std::time::Duration::from_millis(200)).easing(Easing::Ease);
        let on_ness = transition((self.id.clone(), "slide"), target, policy, window, cx);
        let (track, knob_left) = toggle_at(on_ness);
        let knob = self.id.clone();
        let (on, on_click) = (self.on, self.on_click);
        div()
            .id(self.id)
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(6.))
            .cursor_pointer()
            .text_size(px(SMALL))
            .text_color(rgb(0x999999))
            .child(
                div().relative().w(px(28.)).h(px(14.)).rounded(px(7.)).bg(track).child(
                    div()
                        .debug_selector(move || format!("{knob}-knob"))
                        .absolute()
                        .top(px(2.))
                        .left(px(knob_left))
                        .size(px(10.))
                        .rounded_full()
                        .bg(rgb(0xcccccc)),
                ),
            )
            .child(div().whitespace_nowrap().child(self.label))
            .on_click(move |_: &ClickEvent, window, cx| on_click(!on, window, cx))
    }
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
/// colour, the text around it); working spins. Where it takes the pointer, its tooltip is
/// [`activity_hint`].
pub fn activity_glyph(
    id: impl Into<ElementId>,
    state: treeterm_model::workspace_store::ActivityState,
    size: f32,
    idle: impl Into<Hsla>,
) -> Stateful<Div> {
    use treeterm_model::workspace_store::ActivityState;
    let color = activity_color(state, idle);
    let indicator = div().id(id).flex_shrink_0();
    let glyph = match state {
        ActivityState::Idle => "○",
        ActivityState::Working => {
            use gpui_kit::component::Sizable as _;
            return indicator.child(
                gpui_kit::component::spinner::Spinner::new()
                    .icon(Icon::new(IconName::LoaderCircle))
                    .with_size(px(size))
                    .color(color),
            );
        }
        ActivityState::UserInputRequired => "▶",
        ActivityState::PermissionRequest | ActivityState::SafePermissionRequested | ActivityState::Error => "●",
        ActivityState::Completed => "✓",
        ActivityState::ApplicationError => "✗",
    };
    indicator.text_size(px(size)).text_color(color).child(glyph)
}

/// `ActivityIndicator`'s `title`: the state's name ("Idle", "Working...", ...).
pub fn activity_hint(
    state: treeterm_model::workspace_store::ActivityState,
) -> impl Fn(&mut Window, &mut App) -> gpui_kit::AnyView + 'static {
    move |window, cx| gpui_kit::component::tooltip::Tooltip::new(state.title()).build(window, cx)
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

/// A range of selection document orders of a view's own, to start a [`ReadingOrder`] at. A
/// drag from one run of selectable text to another selects every run of the window whose
/// order lies between theirs, so two views side by side, or another view's text, must not
/// share orders.
pub fn document_orders() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) << 32
}

/// Text that can be selected and copied (Cmd/Ctrl+C), as in the browser. GPUI Kit copies
/// the runs a drag spans by their document order alone, so each run takes the next one: a
/// drag over several copies them in reading order, a line each.
pub struct ReadingOrder(u64);

impl ReadingOrder {
    /// Runs ordered after `order` (a view's [`document_orders`], or where the runs read
    /// before these stop).
    pub fn after(order: u64) -> Self {
        Self(order)
    }

    pub fn text(&mut self, id: impl Into<ElementId>, text: impl Into<SharedString>) -> gpui_kit::base::SelectableText {
        self.0 += 1;
        gpui_kit::base::SelectableText::new(id, text).document_order(self.0)
    }

    /// One line of text to be cut short by its parent's `truncate()` (see [`EllipsizedText`]).
    pub fn ellipsized(&mut self, id: impl Into<ElementId>, text: impl Into<SharedString>) -> EllipsizedText {
        self.0 += 1;
        let text = text.into();
        EllipsizedText { id: id.into(), shown: gpui_kit::StyledText::new(text.clone()), text, order: self.0 }
    }
}

/// The ellipsis `Styled::truncate` ends a line it cuts short with.
const ELLIPSIS: &str = "…";

/// What a selection of `range` in `shown`, `text` as laid out, copies: `shown` is `text`, or
/// a start of it and the [`ELLIPSIS`] standing for the rest, which a selection taking in the
/// ellipsis copies too, as Chromium's does.
fn ellipsized_copy(text: &str, shown: &str, range: std::ops::Range<usize>) -> String {
    let kept = if shown == text { text.len() } else { shown.len() - ELLIPSIS.len() };
    let end = if range.end > kept { text.len() } else { range.end };
    text[range.start.min(kept)..end].to_owned()
}

/// One line of text, in a [`ReadingOrder`], cut short with an ellipsis where it does not fit
/// (`text-overflow: ellipsis`: a parent's `truncate()`), that can still be selected and
/// copied, as in Chromium: the part shown is selected as laid out, and a selection reaching
/// the ellipsis copies the rest of the line too. GPUI Kit's `SelectableText` takes no
/// selection once cut short.
pub struct EllipsizedText {
    id: ElementId,
    text: SharedString,
    /// The text as laid out, cut short.
    shown: gpui_kit::StyledText,
    order: u64,
}

/// What an [`EllipsizedText`] keeps between frames: its part in the window's selection, and
/// what that part copies, as last painted.
#[derive(Clone)]
pub struct EllipsizedState {
    handle: gpui_kit::base::TextSelectionHandle,
    copied: std::rc::Rc<std::cell::RefCell<String>>,
}

impl IntoElement for EllipsizedText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui_kit::Element for EllipsizedText {
    type RequestLayoutState = EllipsizedState;
    type PrepaintState = gpui_kit::Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        global_id: Option<&gpui_kit::GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (gpui_kit::LayoutId, EllipsizedState) {
        let text = self.text.clone();
        let state = window.with_element_state(global_id.expect("an EllipsizedText has an id"), |kept, _| {
            let state = kept.unwrap_or_else(|| {
                let copied = std::rc::Rc::new(std::cell::RefCell::new(String::new()));
                let handle = gpui_kit::base::TextSelectionHandle::new(text, cx);
                let copy = copied.clone();
                handle.copy_with(move |_| copy.borrow().clone(), cx);
                EllipsizedState { handle, copied }
            });
            (state.clone(), state)
        });
        let (layout_id, ()) = self.shown.request_layout(global_id, inspector_id, window, cx);
        (layout_id, state)
    }

    fn prepaint(
        &mut self,
        global_id: Option<&gpui_kit::GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut EllipsizedState,
        window: &mut Window,
        cx: &mut App,
    ) -> gpui_kit::Hitbox {
        use gpui_kit::base::TextSelectionRegistration;
        self.shown.prepaint(global_id, inspector_id, bounds, &mut (), window, cx);
        let hitbox = window.insert_hitbox(bounds, gpui_kit::HitboxBehavior::Normal);
        let registration = TextSelectionRegistration::new(hitbox.clone(), bounds)
            .with_document_order(self.order)
            .with_text_bounds(vec![bounds])
            .with_rendered_element(&state.handle, window, cx);
        state.handle.register(registration, window, cx);
        hitbox
    }

    fn paint(
        &mut self,
        global_id: Option<&gpui_kit::GlobalElementId>,
        inspector_id: Option<&gpui_kit::InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut EllipsizedState,
        _: &mut gpui_kit::Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.shown.layout().clone();
        let shown = SharedString::from(layout.text());
        let run = gpui_kit::base::TextSelectionRun::new(shown.clone(), layout.clone(), bounds).with_document_order(self.order);
        let selected = state.handle.update_runs(&[run], cx).ranges().first().cloned().flatten();
        let copied = selected.clone().map_or_else(String::new, |range| ellipsized_copy(&self.text, &shown, range));
        *state.copied.borrow_mut() = copied;
        if let Some(range) = selected
            && let (Some(start), Some(end)) = (layout.position_for_index(range.start), layout.position_for_index(range.end))
        {
            let color = gpui_kit::base::Theme::global(cx).tokens.colors.selection;
            let end = gpui_kit::point(end.x, end.y + layout.line_height());
            window.paint_quad(gpui_kit::fill(Bounds::from_corners(start, end), color));
        }
        self.shown.paint(global_id, inspector_id, bounds, &mut (), &mut (), window, cx);
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

/// The lines of a [`text_area`]: Chromium sets a `<textarea>`'s 13px text in `normal` lines
/// 15px high (an `<input>`'s line is 15.5px).
const TEXT_AREA_LINE: f32 = 15.;

/// `<textarea class="settings-input">`: as [`text_input`], several lines tall.
pub fn text_area(state: &Entity<TextareaState>, cx: &App) -> Textarea {
    let c = colors(cx);
    // Size::Medium pads the editor 8px 10px; 2px more on each side makes Electron's 12px.
    Textarea::new(state)
        .with_size(Size::Medium)
        .px(px(2.))
        .text_size(px(BODY))
        .line_height(px(TEXT_AREA_LINE))
        .bg(c.bg_tertiary)
        .border_color(c.border)
        .rounded(px(4.))
}

/// The height of a [`text_area`] showing `rows` lines: 8px of padding above and below them,
/// and the border.
pub fn text_area_height(rows: usize) -> f32 {
    rows as f32 * TEXT_AREA_LINE + 18.
}

/// `<textarea rows={rows} style={{ resize: 'vertical' }}>`: `textarea` (a [`text_area`])
/// `rows` lines tall, [`resizable`] down to one line (Chromium's goes down to none). The
/// height it is dragged to lasts while it is shown, as a mounted element's does.
pub fn rows_text_area(
    id: impl Into<ElementId>,
    textarea: Textarea,
    rows: usize,
    window: &mut Window,
    cx: &mut App,
) -> Div {
    let id: ElementId = id.into();
    let height = window.use_keyed_state((id.clone(), "height"), cx, |_, _| TextareaHeight::new(text_area_height(rows)));
    resizable(id, textarea, &height, text_area_height(1), cx)
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

/// `.settings-checkbox-label`: a 16px checkbox, 8px gap, 13px label, as a native checkbox in
/// its `<label>` ([`key_checkbox`]); the whole row toggles. Tab reaches it unless `disabled`,
/// and Space toggles it, but Enter does not. The row keeps the pointer cursor while disabled,
/// though a click on it does nothing.
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
    boxed
        .track_focus(&focus.clone().tab_stop(true))
        .key_context(KEY_SELECT)
        .role(gpui_kit::Role::ComboBox)
        .child(focus_probe(focus))
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

/// The control holding the focus and where it was last drawn, as its [`focus_probe`]
/// reports it, for a scroller that brings the focused control into view.
#[derive(Default)]
pub struct FocusedControl(Option<(gpui_kit::WeakFocusHandle, gpui_kit::Bounds<gpui_kit::Pixels>)>);

impl gpui_kit::Global for FocusedControl {}

impl FocusedControl {
    /// Forget the control reported. A scroller does this before its content is drawn, so that
    /// a control [`Self::bounds_of`] finds after it is one of its own.
    pub fn clear(cx: &mut App) {
        cx.default_global::<Self>().0 = None;
    }

    /// Where the control `focus` focuses was drawn since [`Self::clear`], if it reports.
    pub fn bounds_of(focus: &FocusHandle, cx: &App) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        match cx.try_global::<Self>() {
            Some(FocusedControl(Some((reported, bounds)))) if reported == focus => Some(*bounds),
            Some(_) | None => None,
        }
    }
}

/// Laid over a control (absolutely, at its size): while `focus`, the control's focus handle,
/// has the focus, it reports where the control is drawn ([`FocusedControl`]).
fn focus_probe(focus: &FocusHandle) -> impl IntoElement + use<> {
    let focus = focus.clone();
    gpui_kit::canvas(
        move |bounds, window, cx| {
            if focus.is_focused(window) {
                cx.default_global::<FocusedControl>().0 = Some((focus.downgrade(), bounds));
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// The scroll top (the distance scrolled from the top, within `0..=max`) that brings `control`
/// into `viewport`, as Chromium's `focus()` scrolls (`ScrollAlignment::CenterIfNeeded`): none
/// while it is wholly in view or covers the viewport; partly in view, its nearest edge goes to
/// the viewport's; out of view, it is centred. Both were drawn scrolled to `top`.
pub fn reveal_scroll_top(
    control: gpui_kit::Bounds<gpui_kit::Pixels>,
    viewport: gpui_kit::Bounds<gpui_kit::Pixels>,
    top: f32,
    max: f32,
) -> Option<f32> {
    let (control_top, control_bottom) = (f32::from(control.top()), f32::from(control.bottom()));
    let (viewport_top, viewport_bottom) = (f32::from(viewport.top()), f32::from(viewport.bottom()));
    let (height, viewport_height) = (control_bottom - control_top, viewport_bottom - viewport_top);
    let visible = (control_bottom.min(viewport_bottom) - control_top.max(viewport_top)).max(0.);
    let by = if visible >= height || visible >= viewport_height {
        return None;
    } else if visible > 0. {
        // Chromium's closest edge: the bottom one for a control below and smaller than the
        // viewport, or above and larger.
        let bottom = (control_bottom > viewport_bottom && height < viewport_height)
            || (control_bottom < viewport_bottom && height > viewport_height);
        if bottom { control_bottom - viewport_bottom } else { control_top - viewport_top }
    } else {
        (control_top + control_bottom - viewport_top - viewport_bottom) / 2.
    };
    let scrolled = (top + by).clamp(0., max);
    (scrolled != top).then_some(scrolled)
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

/// Chromium's resizer: the square inside a textarea's bottom-right border corner that a
/// drag resizes it from.
const RESIZER: f32 = 15.;

/// A [`resizable`] textarea's height, and how its resizer was last pressed.
pub struct TextareaHeight {
    height: f32,
    /// Where the pointer pressed the resizer (its y).
    pressed: f32,
    /// How far below the pointer the drag from that press holds the bottom edge.
    below: f32,
}

impl TextareaHeight {
    pub fn new(height: f32) -> Self {
        Self { height, pressed: 0., below: 0. }
    }
}

/// Dragging the resizer of the [`resizable`] textarea whose height this is.
#[derive(Clone)]
struct TextareaResize(Entity<TextareaHeight>);

impl gpui_kit::Render for TextareaResize {
    fn render(&mut self, _: &mut Window, _: &mut gpui_kit::Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

/// A `<textarea>` with `resize: vertical`: `textarea` as tall as `height` holds (its text
/// scrolls inside), with Chromium's resizer in its bottom-right corner. A drag from the
/// resizer keeps the bottom edge where it was under the pointer at the press, wherever the
/// textarea moves meanwhile (a centred dialog grows both ways), down to `min`, under an
/// up-down cursor. The resizer takes the mouse from what is under it, so a press on it
/// neither selects text nor moves the focus. `id` names the textarea among those shown
/// with it.
pub fn resizable(
    id: impl Into<ElementId>,
    textarea: Textarea,
    height: &Entity<TextareaHeight>,
    min: f32,
    cx: &App,
) -> Div {
    use gpui_kit::{CursorStyle, DragMoveEvent, MouseButton};
    let id: ElementId = id.into();
    let selector = format!("{id}");
    let (pressed, resized) = (height.clone(), height.clone());
    div()
        .relative()
        .w_full()
        .debug_selector(move || selector)
        // The textarea's own bounds, as last laid out.
        .on_drag_move(move |event: &DragMoveEvent<TextareaResize>, window, cx| {
            if event.drag(cx).0 != resized {
                return;
            }
            let (y, top) = (f32::from(event.event.position.y), f32::from(event.bounds.top()));
            resized.update(cx, |area, _| area.height = (y - top + area.below).max(min));
            window.refresh();
        })
        .child(textarea.h(px(height.read(cx).height)))
        .child(
            div()
                .id((id, "resizer"))
                .absolute()
                .right(px(1.))
                .bottom(px(1.))
                .size(px(RESIZER))
                .cursor(CursorStyle::ResizeUpDown)
                // A wheel over it still scrolls the text, as in Chromium.
                .block_mouse_except_scroll()
                .on_mouse_down(MouseButton::Left, move |event, _, cx| {
                    let y = f32::from(event.position.y);
                    pressed.update(cx, |area, _| area.pressed = y);
                })
                // The drag starts a few pixels past the press, before anything has moved: the
                // resizer is `offset` above the pointer, on the 1px bottom border.
                .on_drag(TextareaResize(height.clone()), |drag, offset, window, cx| {
                    let bottom = f32::from(window.mouse_position().y - offset.y) + RESIZER + 1.;
                    drag.0.update(cx, |area, _| area.below = bottom - area.pressed);
                    cx.new(|_| drag.clone())
                })
                .child(resizer_glyph()),
        )
}

/// Chromium's resizer glyph (`DrawPlatformResizerImage`): two diagonal strokes in the
/// corner's bottom-right 8px, each black at 60% with a white twin at 60% a pixel lower, so
/// that it shows on any background.
fn resizer_glyph() -> impl IntoElement {
    use gpui_kit::{PathBuilder, point};
    gpui_kit::canvas(
        |_, _, _| {},
        |bounds, (), window, _| {
            let corner = bounds.bottom_right();
            let at = |left: f32, up: f32| point(corner.x - px(left), corner.y - px(up));
            for (lower, color) in [(0., gpui_kit::black().opacity(0.6)), (1., gpui_kit::white().opacity(0.6))] {
                let mut strokes = PathBuilder::stroke(px(1.));
                strokes.move_to(at(1.5, 7.5 - lower));
                strokes.line_to(at(8. - lower, 1.));
                strokes.move_to(at(1.5, 4. - lower));
                strokes.line_to(at(4. - lower, 1.));
                match strokes.build() {
                    Ok(path) => window.paint_path(path, color),
                    Err(error) => tracing::error!(%error, "cannot draw a textarea's resizer"),
                }
            }
        },
    )
    .size_full()
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

/// Width of Electron's `::-webkit-scrollbar` (the height of a horizontal one).
const SCROLLBAR: f32 = 10.;

/// Dragging the thumb of the [`scrollbar`] named `id`, grabbed `grab` from its start (known
/// once the drag starts; the drag's value, not its view, reaches the moves).
#[derive(Clone)]
struct ThumbDrag {
    id: ElementId,
    grab: std::rc::Rc<std::cell::Cell<Pixels>>,
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
        .children(scrollbar(id, Axis::Vertical, handle, cx))
}

/// What a [`scrollbar`] shows of what its handle scrolls along its axis.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Extent {
    /// The length shown.
    viewport: f32,
    /// How far it scrolls.
    max: f32,
    /// How far it is scrolled.
    scrolled: f32,
}

impl Extent {
    /// Nothing when there is nothing to scroll along `axis`.
    fn of(handle: &impl gpui_kit::base::ScrollbarHandle, axis: Axis) -> Option<Self> {
        let viewport = f32::from(handle.viewport_bounds().size.along(axis));
        let max = f32::from(handle.content_size().along(axis)) - viewport;
        (max > 0. && viewport > 0.).then(|| Self { viewport, max, scrolled: -f32::from(handle.offset().along(axis)) })
    }

    /// The thumb's length: the share of the track that is shown, at least 20px.
    fn thumb(&self) -> f32 {
        (self.viewport * self.viewport / (self.viewport + self.max)).max(20.)
    }

    /// How far along the track the thumb moves.
    fn travel(&self) -> f32 {
        self.viewport - self.thumb()
    }

    /// Where the thumb starts along the track.
    fn thumb_start(&self) -> f32 {
        self.travel() * (self.scrolled / self.max).clamp(0., 1.)
    }

    /// Which side of the thumb `at` (along the track, from its start) is; nothing on it.
    fn side(&self, at: f32) -> Option<TrackSide> {
        let start = self.thumb_start();
        if at < start {
            Some(TrackSide::Before)
        } else if at >= start + self.thumb() {
            Some(TrackSide::After)
        } else {
            None
        }
    }

    /// Scrolled a page toward `side`: Chromium's `PageStep`, 87.5% of the length shown,
    /// within the scroll range.
    fn paged(&self, side: TrackSide) -> f32 {
        let step = (self.viewport * 0.875).floor().max(1.);
        match side {
            TrackSide::Before => (self.scrolled - step).max(0.),
            TrackSide::After => (self.scrolled + step).min(self.max),
        }
    }
}

/// Where on a [`scrollbar`]'s track a press lands: before or after the thumb.
#[derive(Clone, Copy, Debug, PartialEq)]
enum TrackSide {
    Before,
    After,
}

/// The side of the thumb `position` is on, on the `track` of a [`scrollbar`] along `axis`,
/// with what the bar shows; nothing off the track or on the thumb.
fn track_side(
    handle: &impl gpui_kit::base::ScrollbarHandle,
    axis: Axis,
    track: Bounds<Pixels>,
    position: Point<Pixels>,
) -> Option<(Extent, TrackSide)> {
    if !track.contains(&position) {
        return None;
    }
    let extent = Extent::of(handle, axis)?;
    Some((extent, extent.side(f32::from(position.along(axis) - track.origin.along(axis)))?))
}

/// Scrolls `handle`, showing `extent` along `axis`, a page toward `side`.
fn page(handle: &impl gpui_kit::base::ScrollbarHandle, axis: Axis, extent: Extent, side: TrackSide, window: &mut Window) {
    handle.set_offset(handle.offset().apply_along(axis, |_| px(-extent.paged(side))));
    window.refresh();
}

/// Chromium's wait before a press held on a scrollbar's track pages again: the native
/// theme's (`NSScrollerButtonDelay` on macOS).
const TRACK_FIRST_REPEAT: std::time::Duration =
    std::time::Duration::from_millis(if cfg!(target_os = "macos") { 500 } else { 250 });
/// Chromium's wait between the pages that follow.
const TRACK_REPEAT: std::time::Duration = std::time::Duration::from_millis(50);

/// A press held on a [`scrollbar`]'s track: the pages that follow it. Dropping it (the button
/// comes up, or the bar goes) stops them.
#[derive(Default)]
struct TrackPress(Option<gpui_kit::Task<()>>);

/// Presses on the `track` of the [`scrollbar`] `id` along `axis`, off its thumb. As
/// Chromium's (`Scrollbar::AutoscrollPressedPart`), one pages toward the pointer at once,
/// then again after [`TRACK_FIRST_REPEAT`] and every [`TRACK_REPEAT`] while the button is
/// held and the pointer is on that side of the thumb, so the thumb stops where it reaches
/// the pointer.
fn track_presses<H: gpui_kit::base::ScrollbarHandle + Clone>(id: ElementId, axis: Axis, handle: H) -> impl IntoElement {
    use gpui_kit::{DispatchPhase, HitboxBehavior, MouseButton, MouseDownEvent, MouseUpEvent};
    gpui_kit::canvas(
        move |track, window, cx| {
            let press = window.use_keyed_state((id, "track"), cx, |_, _| TrackPress::default());
            (press, window.insert_hitbox(track, HitboxBehavior::Normal))
        },
        move |track, (press, hitbox), window, _| {
            let released = press.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || event.button != MouseButton::Left || !hitbox.is_hovered(window) {
                    return;
                }
                let Some((extent, side)) = track_side(&handle, axis, track, event.position) else {
                    return;
                };
                page(&handle, axis, extent, side, window);
                let handle = handle.clone();
                let repeat = window.spawn(cx, async move |cx| {
                    let mut wait = TRACK_FIRST_REPEAT;
                    loop {
                        cx.background_executor().timer(wait).await;
                        wait = TRACK_REPEAT;
                        let paged = cx.update(|window, _| {
                            if let Some((extent, under)) = track_side(&handle, axis, track, window.mouse_position())
                                && under == side
                            {
                                page(&handle, axis, extent, side, window);
                            }
                        });
                        // The window closed.
                        if paged.is_err() {
                            return;
                        }
                    }
                });
                press.update(cx, |press, _| press.0 = Some(repeat));
            });
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    released.update(cx, |press, _| press.0 = None);
                }
            });
        },
    )
    .absolute()
    .size_full()
}

/// Whether what `handle` scrolls overflows along `axis`, as last laid out: whether the
/// [`scrollbar`] along that axis is drawn.
pub fn overflows(handle: &impl gpui_kit::base::ScrollbarHandle, axis: Axis) -> bool {
    Extent::of(handle, axis).is_some()
}

/// Electron's classic scrollbar (`::-webkit-scrollbar`: 10px, a `#3c3c3c` thumb rounded 5px
/// on a clear track, `--bg-hover` under the pointer) for what `handle` scrolls along `axis`,
/// to follow it in a row (vertical) or a column (horizontal). While there is something to
/// scroll, the bar takes its 10px from the content, as Chromium's does; its thumb can be
/// dragged, and a press on its track pages (see [`track_presses`]).
pub fn scrollbar<H: gpui_kit::base::ScrollbarHandle + Clone>(
    id: impl Into<ElementId>,
    axis: Axis,
    handle: &H,
    cx: &App,
) -> Vec<gpui_kit::AnyElement> {
    use gpui_kit::DragMoveEvent;
    let name = match axis {
        Axis::Vertical => "vertical",
        Axis::Horizontal => "horizontal",
    };
    let id = ElementId::from((id.into(), name));
    let shown = Extent::of(handle, axis);
    // The bar is built from the content as it was last laid out; how it is now is only
    // known once the content is laid out again (before the bar in its row or column). When
    // the two differ (rows came or went, a bar took its 10px), ask for another frame. A
    // refresh asked for while drawing is dropped, so it is asked for from the next frame.
    let probe = {
        let handle = handle.clone();
        gpui_kit::canvas(
            move |_: Bounds<Pixels>, window, _| {
                if Extent::of(&handle, axis) != shown {
                    window.on_next_frame(|window, _| window.refresh());
                }
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_0()
    };
    let bar = shown.map(|extent| {
        let (travel, max) = (extent.travel(), extent.max);
        let c = colors(cx);
        let (thumb_color, hover) = (c.border, c.bg_hover);
        let (drag_handle, drag_id, thumb_id) = (handle.clone(), id.clone(), id.clone());
        let thumb = div()
            .id((thumb_id.clone(), "thumb"))
            .absolute()
            .rounded(px(5.))
            .bg(thumb_color)
            .hover(move |d| d.bg(hover))
            .on_drag(ThumbDrag { id: thumb_id, grab: Default::default() }, move |drag, grab, _, cx| {
                drag.grab.set(grab.along(axis));
                cx.new(|_| drag.clone())
            });
        let (start, length) = (px(extent.thumb_start()), px(extent.thumb()));
        let (track, thumb) = match axis {
            Axis::Vertical => (div().w(px(SCROLLBAR)).h_full(), thumb.top(start).w_full().h(length)),
            Axis::Horizontal => (div().h(px(SCROLLBAR)).w_full(), thumb.left(start).h_full().w(length)),
        };
        track
            .flex_shrink_0()
            .relative()
            .on_drag_move(move |event: &DragMoveEvent<ThumbDrag>, window, cx| {
                let drag = event.drag(cx);
                if drag.id != drag_id || travel <= 0. {
                    return;
                }
                let along = event.event.position.along(axis) - event.bounds.origin.along(axis) - drag.grab.get();
                let at = f32::from(along).clamp(0., travel);
                drag_handle.set_offset(drag_handle.offset().apply_along(axis, |_| px(-(max * at / travel))));
                window.refresh();
            })
            .child(thumb)
            .child(track_presses(id, axis, handle.clone()))
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
        .child(focus_probe(focus))
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
        .child(
            checkbox_box(checked, disabled, size)
                .when(ring, |b| b.child(focus_outline(radius)))
                .child(focus_probe(&focus)),
        )
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

/// The window drawn, then the frames it asks for after, as the platform draws them: a
/// [`scrollbar`] shows in the frame after the one that first lays its content out.
#[cfg(test)]
pub fn draw_settled(cx: &mut gpui_kit::VisualTestContext) {
    cx.update(|window, cx| window.draw(cx).clear(cx));
    for _ in 0..5 {
        let asked = cx.update(|window, cx| window.simulate_next_frame(cx) > 0);
        if !asked {
            return;
        }
        cx.update(|window, cx| window.draw(cx).clear(cx));
    }
    panic!("the window keeps asking for frames");
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use gpui_kit::component::menu::DropdownMenu as _;
    use gpui_kit::{Context, KeyDownEvent, KeyUpEvent, Keystroke, Render, TestAppContext, VisualTestContext};

    use super::*;

    /// A control at `top`, `height` tall, in a 100px viewport at y 50 scrolled 40px of 300.
    fn revealed(top: f32, height: f32) -> Option<f32> {
        use gpui_kit::{Bounds, point, size};
        let control = Bounds::new(point(px(0.), px(top)), size(px(10.), px(height)));
        let viewport = Bounds::new(point(px(0.), px(50.)), size(px(10.), px(100.)));
        reveal_scroll_top(control, viewport, 40., 300.)
    }

    /// Chromium's `CenterIfNeeded`: in view, nothing; partly in view, the nearest edge; out of
    /// view, centred; never past the ends.
    #[test]
    fn a_focused_control_scrolls_into_view_as_in_chromium() {
        assert_eq!(revealed(50., 100.), None, "wholly in view");
        assert_eq!(revealed(60., 20.), None);
        assert_eq!(revealed(20., 200.), None, "covering the viewport");
        assert_eq!(revealed(140., 20.), Some(50.), "partly below: its bottom at the bottom");
        assert_eq!(revealed(40., 20.), Some(30.), "partly above: its top at the top");
        assert_eq!(revealed(100., 150.), Some(90.), "partly below and taller: its top at the top");
        assert_eq!(revealed(-40., 150.), Some(0.), "partly above and taller: its bottom at the bottom");
        assert_eq!(revealed(300., 20.), Some(250.), "below: centred");
        assert_eq!(revealed(0., 20.), Some(0.), "above: centred, as far as it goes");
        assert_eq!(revealed(900., 20.), Some(300.), "as far as it goes");
    }

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

    /// A lone status-bar [`toggle`] at the window's top left, switched by clicking it.
    struct ToggleHost {
        on: bool,
    }

    impl Render for ToggleHost {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let host = cx.entity();
            div().child(toggle("switch", self.on, "Idle detector", move |on, _, cx| {
                host.update(cx, |host, cx| {
                    host.on = on;
                    cx.notify();
                });
            }))
        }
    }

    fn toggle_window(on: bool, cx: &mut TestAppContext) -> &mut VisualTestContext {
        cx.update(gpui_kit::init);
        let cx = cx.add_window_view(move |_, _| ToggleHost { on }).1;
        draw(cx);
        cx
    }

    /// The knob's left edge, from the track's (which is the window's).
    fn knob_left(cx: &mut VisualTestContext) -> f32 {
        f32::from(cx.debug_bounds("switch-knob").expect("the knob is drawn").origin.x)
    }

    /// `after` later, the frame the toggle asked for: whether it asked for one, and where the
    /// knob is then.
    fn next_frame(after: std::time::Duration, cx: &mut VisualTestContext) -> (bool, f32) {
        cx.executor().advance_clock(after);
        let asked = cx.update(|window, cx| window.simulate_next_frame(cx)) > 0;
        draw(cx);
        (asked, knob_left(cx))
    }

    /// Clicking the toggle slides its knob from 2px to 16px over 0.2s, eased (most of the way
    /// by half time), and it stops asking for frames once there; switching back slides it
    /// back.
    #[gpui_kit::test]
    fn a_toggle_slides_its_knob_over_a_fifth_of_a_second(cx: &mut TestAppContext) {
        let ms = std::time::Duration::from_millis;
        let cx = toggle_window(false, cx);
        assert_eq!(knob_left(cx), 2.);

        let knob = cx.debug_bounds("switch-knob").expect("the knob is drawn");
        cx.simulate_click(knob.center(), gpui_kit::Modifiers::none());
        draw(cx);
        assert_eq!(knob_left(cx), 2., "it starts where it was");
        let (asked, mid) = next_frame(ms(100), cx);
        assert!(asked);
        assert!((12. ..15.).contains(&mid), "eased past the middle at half time: {mid}");
        assert_eq!(next_frame(ms(100), cx), (true, 16.));
        assert_eq!(next_frame(ms(16), cx), (false, 16.), "at rest");

        cx.simulate_click(knob.center(), gpui_kit::Modifiers::none());
        draw(cx);
        assert_eq!(next_frame(ms(200), cx), (true, 2.));
        assert_eq!(next_frame(ms(16), cx), (false, 2.), "at rest");
    }

    /// A toggle that shows already on (a tab switched to) is drawn at rest, not sliding in.
    #[gpui_kit::test]
    fn a_toggle_first_drawn_on_is_at_rest(cx: &mut TestAppContext) {
        let cx = toggle_window(true, cx);
        assert_eq!(knob_left(cx), 16.);
        assert_eq!(next_frame(std::time::Duration::from_millis(16), cx), (false, 16.));
    }

    /// The track is `#555` off and `#0dbc79` on, and blends between them in sRGB.
    #[test]
    fn a_toggle_track_blends_its_colours_in_srgb() {
        assert_eq!(toggle_at(0.), (rgb(0x555555), 2.));
        assert_eq!(toggle_at(1.), (rgb(0x0dbc79), 16.));
        let (track, knob) = toggle_at(0.5);
        let channels = [track.r, track.g, track.b, track.a].map(|c| (c * 255.).round());
        assert_eq!((channels, knob), ([49., 137., 103., 255.], 9.));
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

    /// A [`rows_text_area`] four lines tall, shown while `rows_shown`, above a [`resizable`]
    /// one at least 120 high whose height the view keeps.
    struct Areas {
        rows: Entity<TextareaState>,
        kept: Entity<TextareaState>,
        kept_height: Entity<TextareaHeight>,
        rows_shown: bool,
    }

    impl Render for Areas {
        fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let rows = self.rows_shown.then(|| rows_text_area("rows", text_area(&self.rows, cx), 4, window, cx));
            let kept = resizable("kept", text_area(&self.kept, cx), &self.kept_height, 120., cx);
            div().w(px(300.)).p(px(20.)).flex().flex_col().gap(px(10.)).children(rows).child(kept)
        }
    }

    fn areas(cx: &mut TestAppContext) -> (Entity<Areas>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
        });
        let (view, cx) = cx.add_window_view(|window, cx| Areas {
            rows: cx.new(|cx| TextareaState::new(window, cx)),
            kept: cx.new(|cx| TextareaState::new(window, cx)),
            kept_height: cx.new(|_| TextareaHeight::new(120.)),
            rows_shown: true,
        });
        (view, cx)
    }

    /// The bounds of the textarea `selector` names, drawn afresh.
    fn area(selector: &'static str, cx: &mut VisualTestContext) -> gpui_kit::Bounds<gpui_kit::Pixels> {
        cx.update(|window, cx| window.draw(cx).clear(cx));
        cx.debug_bounds(selector).unwrap_or_else(|| panic!("{selector} is drawn"))
    }

    /// Press the resizer of the textarea `selector` names 5px inside its corner and drag it
    /// `dy` down (GPUI starts the drag 10px on): the textarea's height after.
    fn drag_resizer(selector: &'static str, dy: f32, cx: &mut VisualTestContext) -> f32 {
        use gpui_kit::{Modifiers, MouseButton, point};
        let from = area(selector, cx).bottom_right() - point(px(5.), px(5.));
        let to = |dy: f32| from + point(px(0.), px(dy));
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to(10. * dy.signum()), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(to(dy), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(to(dy), MouseButton::Left, Modifiers::none());
        f32::from(area(selector, cx).size.height)
    }

    /// Electron's `resize: vertical`: the bottom edge follows a drag from the resizer, no
    /// higher than the minimum; the other textarea keeps its height.
    #[gpui_kit::test]
    fn a_resizable_textarea_follows_a_drag_of_its_resizer_down_to_its_minimum(cx: &mut TestAppContext) {
        let (_, cx) = areas(cx);
        assert_eq!(f32::from(area("rows", cx).size.height), 78., "four 15px lines, the padding and the border");
        assert_eq!(f32::from(area("kept", cx).size.height), 120.);

        assert_eq!(drag_resizer("rows", 100., cx), 178.);
        assert_eq!(f32::from(area("kept", cx).size.height), 120.);
        assert_eq!(drag_resizer("rows", -500., cx), text_area_height(1), "one line");
        assert_eq!(drag_resizer("kept", 50., cx), 170.);
        assert_eq!(drag_resizer("kept", -500., cx), 120.);
        assert_eq!(f32::from(area("rows", cx).size.height), text_area_height(1));
    }

    /// A press on the resizer, as in Chromium, neither focuses its textarea nor takes the
    /// focus from another.
    #[gpui_kit::test]
    fn a_press_on_the_resizer_leaves_the_focus_alone(cx: &mut TestAppContext) {
        let (view, cx) = areas(cx);
        let focused = |state: &Entity<TextareaState>, cx: &mut VisualTestContext| {
            cx.update(|window, cx| gpui_kit::Focusable::focus_handle(state.read(cx), cx).is_focused(window))
        };
        let (rows, kept) = cx.update(|_, cx| (view.read(cx).rows.clone(), view.read(cx).kept.clone()));
        drag_resizer("rows", 30., cx);
        assert!(!focused(&rows, cx));

        let inside = area("kept", cx).center();
        cx.simulate_click(inside, gpui_kit::Modifiers::none());
        assert!(focused(&kept, cx));
        drag_resizer("rows", 30., cx);
        assert!(focused(&kept, cx), "still focused");
        assert!(!focused(&rows, cx));
    }

    /// A [`rows_text_area`] is dragged to a height that lasts while it is shown, and is back
    /// to its rows once shown again, as Electron's is when it mounts again.
    #[gpui_kit::test]
    fn a_rows_text_area_is_back_to_its_rows_once_shown_again(cx: &mut TestAppContext) {
        let (view, cx) = areas(cx);
        assert_eq!(drag_resizer("rows", 40., cx), 118.);
        assert_eq!(f32::from(area("rows", cx).size.height), 118., "kept while shown");

        let show = |shown: bool, cx: &mut VisualTestContext| {
            view.update(cx, |view, cx| {
                view.rows_shown = shown;
                cx.notify();
            });
        };
        show(false, cx);
        assert!(area("kept", cx).size.height > px(0.));
        assert!(cx.debug_bounds("rows").is_none());
        show(true, cx);
        assert_eq!(f32::from(area("rows", cx).size.height), 78.);
    }

    /// A selection in a line cut short copies what it takes in of the part shown, and the
    /// rest of the line too once it takes in the ellipsis; a line shown whole copies as
    /// selected.
    #[test]
    fn a_selection_taking_in_the_ellipsis_copies_the_rest_of_the_line() {
        let text = "src/components/list.rs";
        let shown = "src/compo…";
        assert_eq!(ellipsized_copy(text, shown, 0..4), "src/");
        assert_eq!(ellipsized_copy(text, shown, 4..9), "compo");
        assert_eq!(ellipsized_copy(text, shown, 4..shown.len()), "components/list.rs");
        assert_eq!(ellipsized_copy(text, shown, 9..shown.len()), "nents/list.rs", "the ellipsis alone");
        assert_eq!(ellipsized_copy(text, text, 4..14), "components");
    }

    /// A 100×100 box scrolling 1000px of content along `axis`, with its [`scrollbar`]: the
    /// thumb is 20px long (the shortest), travels 80px over the 900px scrolled, and a page is
    /// 87px.
    struct Scrolled {
        axis: Axis,
        handle: gpui_kit::ScrollHandle,
    }

    impl Render for Scrolled {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let (axis, handle) = (self.axis, &self.handle);
            match axis {
                Axis::Vertical => div().size(px(100.)).child(scroll_area("scrolled", handle, div().w_full().h(px(1000.)), cx)),
                Axis::Horizontal => div().size(px(100.)).flex().flex_col().child(
                    div()
                        .id("scrolled")
                        .flex_1()
                        .min_h_0()
                        .overflow_x_scroll()
                        .track_scroll(handle)
                        .child(div().h_full().w(px(1000.)).flex_none()),
                ),
            }
            .children(match axis {
                Axis::Vertical => Vec::new(),
                Axis::Horizontal => scrollbar("scrolled", axis, handle, cx),
            })
        }
    }

    fn scrolled(axis: Axis, cx: &mut TestAppContext) -> (Entity<Scrolled>, &mut VisualTestContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            crate::theme::install(cx);
        });
        let (view, cx) = cx.add_window_view(|_, _| Scrolled { axis, handle: gpui_kit::ScrollHandle::new() });
        draw_settled(cx);
        (view, cx)
    }

    /// How far the box is scrolled along its axis.
    fn scrolled_by(view: &Entity<Scrolled>, cx: &mut VisualTestContext) -> f32 {
        view.read_with(cx, |view, _| -f32::from(view.handle.offset().along(view.axis)))
    }

    /// The point `at` along the bar of a [`Scrolled`] along `axis`, in the middle of its 10px.
    fn on_bar(axis: Axis, at: f32) -> Point<Pixels> {
        match axis {
            Axis::Vertical => gpui_kit::point(px(95.), px(at)),
            Axis::Horizontal => gpui_kit::point(px(at), px(95.)),
        }
    }

    /// Chromium's press on the track: a page (87.5% of the length shown) toward the pointer,
    /// either way; a press on the thumb pages nothing.
    fn a_press_on_the_track_pages_toward_the_pointer(axis: Axis, cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton};
        let (view, cx) = scrolled(axis, cx);
        let click = |at: f32, cx: &mut VisualTestContext| {
            cx.simulate_mouse_down(on_bar(axis, at), MouseButton::Left, Modifiers::none());
            cx.simulate_mouse_up(on_bar(axis, at), MouseButton::Left, Modifiers::none());
            draw_settled(cx);
        };
        click(10., cx);
        assert_eq!(scrolled_by(&view, cx), 0., "on the thumb");
        click(90., cx);
        assert_eq!(scrolled_by(&view, cx), 87., "a page on");
        click(90., cx);
        assert_eq!(scrolled_by(&view, cx), 174.);
        // The thumb starts 80 × 174/900 = 15.5px along the track.
        click(5., cx);
        assert_eq!(scrolled_by(&view, cx), 87., "a page back");
        click(5., cx);
        assert_eq!(scrolled_by(&view, cx), 0., "no further than the start");
    }

    #[gpui_kit::test]
    fn a_press_on_a_vertical_track_pages_toward_the_pointer(cx: &mut TestAppContext) {
        a_press_on_the_track_pages_toward_the_pointer(Axis::Vertical, cx);
    }

    #[gpui_kit::test]
    fn a_press_on_a_horizontal_track_pages_toward_the_pointer(cx: &mut TestAppContext) {
        a_press_on_the_track_pages_toward_the_pointer(Axis::Horizontal, cx);
    }

    /// A press held on the track pages on after a wait, then every 50 ms, while the pointer
    /// is on that side of the thumb, and stops where the thumb reaches it or the button
    /// comes up.
    #[gpui_kit::test]
    fn a_press_held_on_the_track_pages_until_the_thumb_reaches_the_pointer(cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton};
        let (view, cx) = scrolled(Axis::Vertical, cx);
        let wait = |time: std::time::Duration, cx: &mut VisualTestContext| {
            cx.executor().advance_clock(time);
            cx.run_until_parked();
            draw_settled(cx);
        };
        let ticks = |count: usize, cx: &mut VisualTestContext| {
            for _ in 0..count {
                wait(TRACK_REPEAT, cx);
            }
        };
        cx.simulate_mouse_down(on_bar(Axis::Vertical, 60.), MouseButton::Left, Modifiers::none());
        draw_settled(cx);
        assert_eq!(scrolled_by(&view, cx), 87.);
        wait(TRACK_FIRST_REPEAT - std::time::Duration::from_millis(1), cx);
        assert_eq!(scrolled_by(&view, cx), 87., "not before the first wait");
        wait(std::time::Duration::from_millis(1), cx);
        assert_eq!(scrolled_by(&view, cx), 174.);
        ticks(1, cx);
        assert_eq!(scrolled_by(&view, cx), 261.);

        // Off the bar, the pages wait; back on it, they go on.
        cx.simulate_mouse_move(gpui_kit::point(px(50.), px(60.)), MouseButton::Left, Modifiers::none());
        ticks(4, cx);
        assert_eq!(scrolled_by(&view, cx), 261.);
        cx.simulate_mouse_move(on_bar(Axis::Vertical, 60.), MouseButton::Left, Modifiers::none());
        ticks(1, cx);
        assert_eq!(scrolled_by(&view, cx), 348.);

        // At 522px, the thumb (80 × 522/900 = 46.4px along, 20px long) is under the pointer.
        ticks(2, cx);
        assert_eq!(scrolled_by(&view, cx), 522.);
        ticks(4, cx);
        assert_eq!(scrolled_by(&view, cx), 522., "the thumb reached the pointer");

        // Further down, the pages go on, until the button comes up.
        cx.simulate_mouse_move(on_bar(Axis::Vertical, 95.), MouseButton::Left, Modifiers::none());
        ticks(1, cx);
        assert_eq!(scrolled_by(&view, cx), 609.);
        cx.simulate_mouse_up(on_bar(Axis::Vertical, 95.), MouseButton::Left, Modifiers::none());
        ticks(4, cx);
        assert_eq!(scrolled_by(&view, cx), 609.);
    }

    /// The thumb follows a drag along its bar.
    fn a_thumb_follows_a_drag_along_its_bar(axis: Axis, cx: &mut TestAppContext) {
        use gpui_kit::{Modifiers, MouseButton};
        let (view, cx) = scrolled(axis, cx);
        cx.simulate_mouse_move(on_bar(axis, 5.), None, Modifiers::none());
        cx.simulate_mouse_down(on_bar(axis, 5.), MouseButton::Left, Modifiers::none());
        for at in [10., 30., 50.] {
            cx.simulate_mouse_move(on_bar(axis, at), MouseButton::Left, Modifiers::none());
        }
        cx.simulate_mouse_up(on_bar(axis, 50.), MouseButton::Left, Modifiers::none());
        draw_settled(cx);
        // Grabbed 10px into the thumb (where the drag starts) and moved 40px along a travel
        // of 80px: half way.
        assert_eq!(scrolled_by(&view, cx), 450.);
    }

    #[gpui_kit::test]
    fn a_vertical_thumb_follows_a_drag_along_its_bar(cx: &mut TestAppContext) {
        a_thumb_follows_a_drag_along_its_bar(Axis::Vertical, cx);
    }

    #[gpui_kit::test]
    fn a_horizontal_thumb_follows_a_drag_along_its_bar(cx: &mut TestAppContext) {
        a_thumb_follows_a_drag_along_its_bar(Axis::Horizontal, cx);
    }
}
