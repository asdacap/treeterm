//! A tabset's header (`FlexLayoutPane.tsx` on flex-layout's tab strip): the tabs, then "+"
//! (`flexlayout-new-tab-btn`). Tabs that do not fit scroll sideways under the wheel, the
//! selected tab is scrolled into view, and a "Hidden tabs" button lists the tabs out of view
//! (flex-layout's `useTabOverflow`); "+" is a sticky button that stays at the right end.
//! Once the layout has more than one tabset, the toolbar at the right end also has the
//! "Maximize tab set" / "Restore tab set" toggle, which a double-click on the strip toggles
//! too; a press on the strip (not on its buttons) makes the tabset the active one.
//! A tab dragged anywhere over the strip lands before the first tab whose middle is right of
//! the pointer, else at the end (flex-layout's tab bar drop), but not on the layout's edge
//! docks lying over the strip.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ElementExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Anchor, App, Bounds, Context, DragMoveEvent, EntityId, MouseButton, Pixels, Point, ScrollHandle,
    SharedString, Window, div, px,
};
use treeterm_core::layout::{Maximize, Side};
use treeterm_model::workspace_store::ActivityState;

use crate::tab_layout_view::DocksAtEdge;
use crate::theme::colors;

/// A tab as the strip shows it.
pub struct StripTab {
    pub id: String,
    pub title: String,
    /// Its application's icon.
    pub icon: String,
    /// Tabs of applications the native app lacks (or that cannot close) have no close button.
    pub closable: bool,
    pub activity: ActivityState,
}

/// An application the new-tab menu offers.
pub struct NewTabApp {
    pub id: String,
    pub name: String,
    pub icon: String,
}

/// What clicking a tab, its close button or a new-tab menu item does, given that tab's or
/// application's id.
pub type OnId = Rc<dyn Fn(&str, &mut Window, &mut App)>;

/// What dropping a dragged tab (the first id) on a side of another tab (the second) does.
pub type OnMove = Rc<dyn Fn(&str, &str, Side, &mut Window, &mut App)>;

/// What a press on the strip, or its maximize toggle, does.
pub type OnPress = Rc<dyn Fn(&mut Window, &mut App)>;

/// The strip's height, its 1px rule included: what a tabset needs at the least.
pub const STRIP_HEIGHT: f32 = 34.;

/// One strip's tabs and handlers, drawn with the [`TabStripState`] its view keeps.
pub struct TabStrip {
    pub tabs: Vec<StripTab>,
    /// The tab its tabset shows.
    pub active: Option<String>,
    /// The new-tab menu; without any, there is no "+".
    pub apps: Vec<NewTabApp>,
    pub on_select: OnId,
    pub on_close: OnId,
    pub on_move: OnMove,
    pub on_new_tab: OnId,
    /// A press on the strip, but not on "+" or the toolbar (flex-layout's `setActiveTabset`).
    pub on_activate: OnPress,
    /// The tabset's maximize toggle, and what it (or a double-click on the strip) does.
    pub maximize: Maximize,
    pub on_maximize: OnPress,
    /// Whether the tab dragged over the layout is on one of its edge docks, which then takes
    /// it from the strip under it.
    pub docks_at_edge: DocksAtEdge,
}

/// A tab being dragged, and its drag image: the tab's icon and title (flex-layout drags the
/// tab's stamp, `flexlayout__tab_button_stamp`), where the tab's own were.
#[derive(Clone)]
pub struct DraggedTab {
    id: String,
    icon: String,
    title: String,
}

impl DraggedTab {
    pub fn new(id: &str, icon: &str, title: &str) -> Self {
        Self { id: id.to_owned(), icon: icon.to_owned(), title: title.to_owned() }
    }

    /// The tab's id.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Render for DraggedTab {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // Chromium draws a drag image translucent.
        h_flex()
            .h(px(31.))
            .px(px(12.))
            .gap(px(3.6))
            .items_center()
            .text_size(px(12.))
            .text_color(colors(cx).text_primary)
            .opacity(0.75)
            .child(div().flex_shrink_0().mr(px(6.)).child(self.icon.clone()))
            .child(div().whitespace_nowrap().child(self.title.clone()))
    }
}

/// Where a dragged tab would land: beside this tab, on this side.
type DropHint = Option<(String, Side)>;

/// Where a tab dropped at `pointer` in the strip lands among `tabs` (on screen, in strip
/// order), as flex-layout's `TabSetNode.canDrop` finds it anywhere in the tab bar: before the
/// first tab whose middle is right of the pointer, else after the last tab. Off the tabs' row
/// (the strip's inset above them, its rule below) it is after the last tab too, as there.
/// `None` without tabs.
pub fn landing<T>(tabs: &[(T, Bounds<Pixels>)], pointer: Point<Pixels>) -> Option<(&T, Side)> {
    let (last, _) = tabs.last()?;
    let before =
        tabs.iter().find(|(_, tab)| pointer.y > tab.top() && pointer.y < tab.bottom() && pointer.x < tab.center().x);
    Some(before.map_or((last, Side::After), |(tab, _)| (tab, Side::Before)))
}

/// The tabs the strip shows, by id, with their bounds on screen: a tab scrolled wholly out
/// of view is no drop target, so the tab lands where the hint bar can show it.
fn shown_tabs<'a>(scroll: &ScrollHandle, ids: &'a [String]) -> Vec<(&'a String, Bounds<Pixels>)> {
    let (viewport, offset) = (scroll.bounds(), scroll.offset());
    ids.iter()
        .zip(tab_bounds(scroll, ids.len()))
        .map(|(id, tab)| (id, Bounds { origin: tab.origin + offset, size: tab.size }))
        .filter(|(_, tab)| tab.right() > viewport.left() && tab.left() < viewport.right())
        .collect()
}

/// While a tab is dragged over the strip (`event.bounds`), the hint follows the pointer:
/// where the tab would land among the strip's tabs, `ids`. It goes when the pointer leaves,
/// or is on one of the layout's edge docks lying over the strip.
fn drag_over(
    ids: Vec<String>,
    state: &TabStripState,
    docks_at_edge: DocksAtEdge,
    view: EntityId,
) -> impl Fn(&DragMoveEvent<DraggedTab>, &mut Window, &mut App) + 'static {
    let (hint, scroll) = (state.drop_hint.clone(), state.scroll.clone());
    move |event, _, cx| {
        let pointer = event.event.position;
        let next = if event.bounds.contains(&pointer) && !docks_at_edge() {
            landing(&shown_tabs(&scroll, &ids), pointer).map(|(tab, side)| (String::clone(tab), side))
        } else {
            None
        };
        if *hint.borrow() != next {
            *hint.borrow_mut() = next;
            cx.notify(view);
        }
    }
}

/// A tab dropped on the strip lands where its hint shows.
fn drop_on(
    state: &TabStripState,
    on_move: OnMove,
    view: EntityId,
) -> impl Fn(&DraggedTab, &mut Window, &mut App) + 'static {
    let hint = state.drop_hint.clone();
    move |dragged, window, cx| {
        let landing = hint.borrow_mut().take();
        cx.notify(view);
        // Without a hint, no tab is shown to land beside.
        if let Some((target, side)) = landing {
            on_move(&dragged.id, &target, side, window, cx);
        }
    }
}

/// A strip's scroll position and what its last frame measured, kept across renders.
#[derive(Default)]
pub struct TabStripState {
    scroll: ScrollHandle,
    /// What the strip last scrolled into view. A wheel scroll changes none of it, so a render
    /// never undoes one; a newly active tab, a resize or the Hidden tabs button coming or going
    /// reveals the active tab again (flex-layout clears `userControlledLeft` on a new selection
    /// or size).
    revealed: Option<Reveal>,
    /// A pick from the Hidden tabs menu reveals the picked tab even when it was already active.
    reveal_requested: Rc<Cell<bool>>,
    /// Measured while painting, for the next render.
    painted: Rc<Cell<Painted>>,
    /// Where the tab being dragged would land, while it is over a tab.
    drop_hint: Rc<RefCell<DropHint>>,
}

#[derive(Clone, Debug, PartialEq)]
struct Reveal {
    tab: String,
    width: Pixels,
    overflowing: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Painted {
    /// The strip's width: zero until it is first painted.
    width: Pixels,
    /// It showed the Hidden tabs button.
    overflowing: bool,
    /// Where "+" ends, from the window's left edge.
    plus_right: Pixels,
}

/// How far past the strip's edge a tab may reach and still count as shown: the scroll offset
/// is rounded, which can leave a sub-pixel residue at either end.
const EDGE: Pixels = px(0.5);

/// The tabs not wholly inside the strip (flex-layout's `hiddenTabs`), by index: from the
/// strip's `viewport`, its horizontal scroll `offset` and each tab's unscrolled bounds.
pub fn hidden_tabs(viewport: Bounds<Pixels>, offset: Pixels, tabs: &[Bounds<Pixels>]) -> Vec<usize> {
    tabs.iter()
        .enumerate()
        .filter(|(_, tab)| tab.left() + offset < viewport.left() - EDGE || tab.right() + offset > viewport.right() + EDGE)
        .map(|(ix, _)| ix)
        .collect()
}

/// The new-tab menu's corner on "+": right-aligned, opening leftward, once "+" ends 140px or
/// more from the window's left edge; left-aligned before that (`NewTabDropdownMenu`).
pub fn new_tab_menu_anchor(plus_right: Pixels) -> Anchor {
    if plus_right >= px(140.) { Anchor::TopRight } else { Anchor::TopLeft }
}

/// Each tab's bounds as last laid out, unscrolled.
fn tab_bounds(scroll: &ScrollHandle, count: usize) -> Vec<Bounds<Pixels>> {
    (0..count).map_while(|ix| scroll.bounds_for_item(ix)).collect()
}

impl TabStripState {
    fn hidden(&self, count: usize) -> Vec<usize> {
        hidden_tabs(self.scroll.bounds(), self.scroll.offset().x, &tab_bounds(&self.scroll, count))
    }
}

impl TabStrip {
    /// The strip, `id` naming it among the window's strips. `view` is the view that renders
    /// it: it renders again whenever a frame's layout leaves the strip out of date.
    pub fn render(self, id: &str, state: &mut TabStripState, view: EntityId, cx: &App) -> AnyElement {
        let c = colors(cx);
        let (secondary, primary, active_bg, hover) = (c.text_secondary, c.text_primary, c.bg_primary, c.bg_hover);
        let accent = c.accent;
        let painted = state.painted.get();
        let active_ix = self.active.as_ref().and_then(|active| self.tabs.iter().position(|tab| &tab.id == active));
        let wanted = self.active.clone().map(|tab| Reveal { tab, width: painted.width, overflowing: painted.overflowing });
        let requested = state.reveal_requested.take();
        // `scroll_to_item` measures against the last frame's strip, so not before there was one.
        if painted.width > px(0.) && (wanted != state.revealed || requested) {
            if let Some(ix) = active_ix {
                state.scroll.scroll_to_item(ix);
            }
            state.revealed = wanted;
        }
        let hidden = state.hidden(self.tabs.len());
        let anchor = new_tab_menu_anchor(painted.plus_right);
        let hint = state.drop_hint.borrow().clone();

        let mut tabs = Vec::with_capacity(self.tabs.len());
        for tab in &self.tabs {
            let is_active = self.active.as_deref() == Some(tab.id.as_str());
            let group = SharedString::from(format!("tab-group-{}", tab.id));
            let text = if is_active { primary } else { secondary };
            let (on_select, on_close) = (self.on_select.clone(), self.on_close.clone());
            let (select_id, close_id) = (tab.id.clone(), tab.id.clone());
            let side = hint.as_ref().filter(|(over, _)| *over == tab.id).map(|(_, side)| *side);
            // flex-layout's tab strip (`flexlayout-overrides.css`): tabs 31px high from a 2px
            // inset, rounded on top, 4px apart. Every closable tab, active or not, shows its
            // close button while hovered (`.flexlayout__tab_button_trailing` at `opacity: 0`
            // until `:hover`).
            tabs.push(
                h_flex()
                    .id(SharedString::from(format!("tab-{}", tab.id)))
                    .group(group.clone())
                    .flex_shrink_0()
                    .h(px(31.))
                    .px(px(12.))
                    .gap(px(3.6))
                    .items_center()
                    .rounded_t(px(4.))
                    .text_size(px(12.))
                    .text_color(text)
                    .when(is_active, |d| d.bg(active_bg))
                    .when(!is_active, |d| d.hover(move |s| s.bg(hover).text_color(primary)))
                    .cursor_pointer()
                    .child(div().flex_shrink_0().mr(px(6.)).child(tab.icon.clone()))
                    .child(div().whitespace_nowrap().child(tab.title.clone()))
                    .child(div().ml(px(4.)).flex_shrink_0().child(crate::ui::activity_glyph(tab.activity, 10., text)))
                    .when(tab.closable, |d| {
                        d.child(
                            div()
                                .id(SharedString::from(format!("tab-close-{}", tab.id)))
                                .size(px(13.))
                                .flex_shrink_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(4.))
                                .invisible()
                                .group_hover(group.clone(), |d| d.visible())
                                .hover(move |s| s.bg(hover))
                                .child(crate::ui::icon(gpui_kit::assets::IconName::X, 11., secondary))
                                .on_click(move |_, window, cx| {
                                    cx.stop_propagation();
                                    on_close(&close_id, window, cx);
                                }),
                        )
                    })
                    .on_click(move |_, window, cx| on_select(&select_id, window, cx))
                    .relative()
                    .on_drag(DraggedTab::new(&tab.id, &tab.icon, &tab.title), |dragged, _, _, cx| {
                        cx.new(|_| dragged.clone())
                    })
                    // flex-layout's drop outline in a tab strip: a 3px accent bar from 2px left
                    // of the edge the tab would land against.
                    .when_some(side, |d, side| {
                        let bar = div().absolute().top_0().bottom_0().w(px(3.)).rounded(px(1.5)).bg(accent);
                        d.child(match side {
                            Side::Before => bar.left(px(-2.)),
                            Side::After => bar.right(px(-1.)),
                        })
                    }),
            );
        }
        // The tabs scroll inside their own box (`flexlayout__tabset_tabbar_inner`), as wide as
        // they are until the strip is full, so "+" follows the last tab or sits at the end.
        let viewport = h_flex()
            .id(SharedString::from(format!("tab-strip-{id}")))
            .h_full()
            .min_w_0()
            .pt(px(2.))
            .px(px(4.))
            .gap(px(4.))
            .items_end()
            .overflow_x_scroll()
            .track_scroll(&state.scroll)
            .children(tabs);

        // "+" opens the new-tab menu: every application offered.
        let plus = (!self.apps.is_empty()).then(|| {
            let (apps, on_new_tab) = (Rc::new(self.apps), self.on_new_tab.clone());
            let measured = state.painted.clone();
            div()
                .h(px(31.))
                .flex_shrink_0()
                .pl(px(5.))
                .flex()
                .items_center()
                .child(
                    crate::ui::Trigger::new(
                        crate::ui::item_action(SharedString::from(format!("new-tab-{id}")), cx)
                            .text_size(px(16.))
                            .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("New tab").build(window, cx))
                            .child("+"),
                    )
                    .dropdown_menu_with_anchor(anchor, move |mut menu, _window, _cx| {
                        for app in apps.iter() {
                            // `.app-menu-item`: a 16px centred icon, 8px gap, the name (13px).
                            let (icon, name) = (app.icon.clone(), app.name.clone());
                            let item = PopupMenuItem::element(move |_, _| {
                                h_flex()
                                    .h(px(29.5))
                                    .gap(px(8.))
                                    .text_size(px(13.))
                                    .child(div().w(px(16.)).flex_shrink_0().flex().justify_center().child(icon.clone()))
                                    .child(div().flex_1().whitespace_nowrap().child(name.clone()))
                            });
                            let (on_new_tab, app_id) = (on_new_tab.clone(), app.id.clone());
                            menu = menu.item(item.on_click(move |_, window, cx| on_new_tab(&app_id, window, cx)));
                        }
                        menu
                    }),
                )
                .on_prepaint(move |bounds, _, _| {
                    let mut painted = measured.get();
                    painted.plus_right = bounds.right();
                    measured.set(painted);
                })
                // A press on "+" is not one on the strip (`onInterceptPointerDown`).
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });

        // The Hidden tabs button (`flexlayout__tab_button_overflow`): flex-layout's caret and
        // the count; its menu lists each hidden tab's icon and title, and picking one selects it.
        let overflow = (!hidden.is_empty()).then(|| {
            let entries: Vec<(String, String, String)> = hidden
                .iter()
                .filter_map(|ix| self.tabs.get(*ix))
                .map(|tab| (tab.id.clone(), tab.icon.clone(), tab.title.clone()))
                .collect();
            let (on_select, requested) = (self.on_select.clone(), state.reveal_requested.clone());
            div().flex().items_center().child(
                crate::ui::Trigger::new(
                    h_flex()
                        .id(SharedString::from(format!("tab-overflow-{id}")))
                        .p(px(1.))
                        .rounded(px(4.))
                        .text_size(px(12.))
                        .text_color(secondary)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover).text_color(primary))
                        .tooltip(|window, cx| gpui_kit::component::tooltip::Tooltip::new("Hidden tabs").build(window, cx))
                        .child(div().w(px(12.)).flex().justify_center().text_size(px(9.)).child("▼"))
                        .child(div().child(hidden.len().to_string())),
                )
                .dropdown_menu_with_anchor(Anchor::TopRight, move |mut menu, window, _cx| {
                    // `.flexlayout__popup_menu_container`: at most half the height, scrolling.
                    menu = menu.scrollable(true).max_h(window.viewport_size().height / 2.);
                    for (tab_id, icon, title) in &entries {
                        // `.flexlayout__popup_menu_item` holding the tab's stamp: icon, title.
                        let (icon, title) = (icon.clone(), title.clone());
                        let item = PopupMenuItem::element(move |_, _| {
                            h_flex()
                                .py(px(2.))
                                .gap(px(3.6))
                                .text_size(px(12.))
                                .child(div().flex_shrink_0().mr(px(6.)).child(icon.clone()))
                                .child(div().whitespace_nowrap().child(title.clone()))
                        });
                        let (on_select, requested, tab_id) = (on_select.clone(), requested.clone(), tab_id.clone());
                        menu = menu.item(item.on_click(move |_, window, cx| {
                            requested.set(true);
                            on_select(&tab_id, window, cx);
                        }));
                    }
                    menu
                }),
            )
        });

        // The maximize toggle (`flexlayout__tab_toolbar_button-min` / `-max`): flex-layout's
        // icon at the toolbar's 1em, 1px padding, rounded 4px.
        let toggle = match self.maximize {
            Maximize::Unavailable => None,
            Maximize::Restored => Some(("Maximize tab set", gpui_kit::assets::IconName::Maximize)),
            Maximize::Maximized => Some(("Restore tab set", gpui_kit::assets::IconName::Minimize)),
        }
        .map(|(title, icon)| {
            let on_maximize = self.on_maximize.clone();
            div()
                .id(SharedString::from(format!("tab-maximize-{id}")))
                .p(px(1.))
                .rounded(px(4.))
                .text_color(secondary)
                .cursor_pointer()
                .hover(move |s| s.bg(hover).text_color(primary))
                .tooltip(move |window, cx| gpui_kit::component::tooltip::Tooltip::new(title).build(window, cx))
                .child(gpui_kit::component::Icon::new(icon).size(px(12.)))
                .on_click(move |_, window, cx| on_maximize(window, cx))
        });

        // The toolbar (`flexlayout__tab_toolbar`, 0.3em apart): Hidden tabs, then the toggle. A
        // press there is not one on the strip (`onInterceptPointerDown`).
        let toolbar = (overflow.is_some() || toggle.is_some()).then(|| {
            div()
                .h(px(31.))
                .flex_shrink_0()
                .px(px(3.6))
                .gap(px(3.6))
                .flex()
                .items_center()
                .children(overflow)
                .children(toggle)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });

        // After layout, the strip's size and the tabs' places are this frame's: when they
        // leave what was just rendered out of date (the first frame, a resize, the button
        // coming or going, a tab added or closed), render again.
        let check = {
            let (scroll, measured) = (state.scroll.clone(), state.painted.clone());
            let (count, active, revealed) = (self.tabs.len(), self.active.clone(), state.revealed.clone());
            let rendered = hidden.clone();
            move |bounds: Bounds<Pixels>, window: &mut Window, _: &mut App| {
                let now = Painted { width: bounds.size.width, overflowing: !rendered.is_empty(), ..measured.get() };
                measured.set(now);
                let unrevealed = active.is_some_and(|tab| {
                    revealed != Some(Reveal { tab, width: now.width, overflowing: now.overflowing })
                });
                let moved = hidden_tabs(scroll.bounds(), scroll.offset().x, &tab_bounds(&scroll, count)) != rendered;
                if unrevealed || moved || new_tab_menu_anchor(now.plus_right) != anchor {
                    window.on_next_frame(move |_, cx| cx.notify(view));
                }
            }
        };

        // `flexlayout__tabset_tabbar_outer`: 33px plus a 1px rule. All of it takes a dragged
        // tab (flex-layout's `tabStripRect`). A press on it activates the tabset, and a
        // double-click toggles its maximize (`onPointerDown`, `onDoubleClick`).
        let ids = self.tabs.iter().map(|tab| tab.id.clone()).collect();
        let (on_activate, on_maximize, maximize) = (self.on_activate, self.on_maximize, self.maximize);
        h_flex()
            .on_drag_move(drag_over(ids, state, self.docks_at_edge, view))
            .on_drop(drop_on(state, self.on_move, view))
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                if event.click_count == 2 && maximize != Maximize::Unavailable {
                    on_maximize(window, cx);
                }
                on_activate(window, cx);
            })
            .h(px(STRIP_HEIGHT))
            .flex_shrink_0()
            .px(px(2.))
            .bg(c.bg_secondary)
            .border_b_1()
            .border_color(c.border)
            .child(
                h_flex()
                    .h_full()
                    .flex_1()
                    .min_w_0()
                    .items_end()
                    .child(viewport)
                    .children(plus)
                    // The toolbar sits at the strip's end (`flexlayout__tabset_tabbar_inner`
                    // takes the room between).
                    .child(div().flex_1())
                    .children(toolbar)
                    .on_prepaint(check),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Modifiers, MouseButton, MouseMoveEvent, Render, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext, TouchPhase, VisualTestContext, bounds, point, size};

    use super::*;

    fn span(left: f32, right: f32) -> Bounds<Pixels> {
        bounds(point(px(left), px(0.)), size(px(right - left), px(31.)))
    }

    #[test]
    fn tabs_inside_the_strip_are_shown() {
        let viewport = span(100., 400.);
        assert_eq!(hidden_tabs(viewport, px(0.), &[span(100., 200.), span(200., 400.)]), Vec::<usize>::new());
        assert_eq!(hidden_tabs(viewport, px(0.), &[]), Vec::<usize>::new());
    }

    #[test]
    fn a_tab_clipped_on_either_side_is_hidden() {
        let viewport = span(100., 400.);
        let tabs = [span(90., 150.), span(150., 300.), span(300., 410.)];
        assert_eq!(hidden_tabs(viewport, px(0.), &tabs), vec![0, 2]);
        // Scrolled 10px left: the first tab is further out, the last one now ends at the edge.
        assert_eq!(hidden_tabs(viewport, px(-10.), &tabs), vec![0]);
        // A sub-pixel residue at the edge still counts as shown.
        assert_eq!(hidden_tabs(viewport, px(9.7), &tabs), vec![2]);
    }

    #[test]
    fn the_new_tab_menu_opens_leftward_from_140px() {
        assert_eq!(new_tab_menu_anchor(px(139.)), Anchor::TopLeft);
        assert_eq!(new_tab_menu_anchor(px(140.)), Anchor::TopRight);
        assert_eq!(new_tab_menu_anchor(px(900.)), Anchor::TopRight);
    }

    /// A strip of `count` tabs, 400px wide from 200px in.
    struct Host {
        state: TabStripState,
        count: usize,
        active: Option<String>,
        /// What the strip asked for: `select <tab>`, `move <dragged> <target> <side>`.
        log: Rc<RefCell<Vec<String>>>,
        maximize: Maximize,
        /// Presses on the strip: `activate`, `maximize`.
        presses: Rc<RefCell<Vec<&'static str>>>,
        /// The layout's edge docks take the dragged tab.
        at_edge: Rc<Cell<bool>>,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let tabs = (0..self.count)
                .map(|ix| StripTab {
                    id: format!("t{ix}"),
                    title: format!("Terminal {ix}"),
                    icon: ">_".to_owned(),
                    closable: true,
                    activity: ActivityState::Idle,
                })
                .collect();
            let none: OnId = Rc::new(|_, _, _| {});
            let (selects, moves) = (self.log.clone(), self.log.clone());
            let strip = TabStrip {
                tabs,
                active: self.active.clone(),
                apps: vec![NewTabApp { id: "terminal".to_owned(), name: "Terminal".to_owned(), icon: ">_".to_owned() }],
                on_select: Rc::new(move |tab, _, _| selects.borrow_mut().push(format!("select {tab}"))),
                on_close: none.clone(),
                on_move: Rc::new(move |dragged, target, side, _, _| {
                    moves.borrow_mut().push(format!("move {dragged} {target} {side:?}"));
                }),
                on_new_tab: none,
                on_activate: {
                    let presses = self.presses.clone();
                    Rc::new(move |_, _| presses.borrow_mut().push("activate"))
                },
                maximize: self.maximize,
                on_maximize: {
                    let presses = self.presses.clone();
                    Rc::new(move |_, _| presses.borrow_mut().push("maximize"))
                },
                docks_at_edge: {
                    let at_edge = self.at_edge.clone();
                    Rc::new(move || at_edge.get())
                },
            };
            let view = cx.entity_id();
            div().pl(px(200.)).w(px(600.)).child(strip.render("test", &mut self.state, view, cx))
        }
    }

    /// Frames until the strip settles (the first one measures, the next ones catch up).
    fn settle(cx: &mut VisualTestContext) {
        for _ in 0..6 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }
    }

    fn strip<'a>(count: usize, active: &str, cx: &'a mut TestAppContext) -> (gpui_kit::Entity<Host>, &'a mut VisualTestContext) {
        strip_with(count, active, Maximize::Unavailable, cx)
    }

    fn strip_with<'a>(
        count: usize,
        active: &str,
        maximize: Maximize,
        cx: &'a mut TestAppContext,
    ) -> (gpui_kit::Entity<Host>, &'a mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let active = Some(active.to_owned());
        let (host, cx) = cx.add_window_view(move |_, _| Host {
            state: TabStripState::default(),
            count,
            active,
            log: Rc::default(),
            maximize,
            presses: Rc::default(),
            at_edge: Rc::default(),
        });
        settle(cx);
        (host, cx)
    }

    fn presses(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Vec<&'static str> {
        cx.update(|_, cx| host.read(cx).presses.borrow().clone())
    }

    fn press(at: Point<Pixels>, clicks: usize, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(at, None, Modifiers::default());
        cx.simulate_event(gpui_kit::MouseDownEvent {
            button: MouseButton::Left,
            position: at,
            modifiers: Modifiers::default(),
            click_count: clicks,
            first_mouse: false,
        });
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        settle(cx);
    }

    #[gpui_kit::test]
    fn a_press_on_the_strip_activates_its_tabset_but_not_one_on_plus(cx: &mut TestAppContext) {
        let (host, cx) = strip(2, "t0", cx);
        let (second, plus_right) = (tab_at(&host, 1, cx), painted(&host, cx).plus_right);
        // On a tab, and on the strip past "+".
        press(second.center(), 1, cx);
        press(point(px(590.), second.center().y), 1, cx);
        assert_eq!(presses(&host, cx), ["activate", "activate"]);
        assert_eq!(log(&host, cx), ["select t1"]);
        // On "+": not the strip's.
        press(point(plus_right - px(12.), second.center().y), 1, cx);
        assert_eq!(presses(&host, cx), ["activate", "activate"]);
    }

    #[gpui_kit::test]
    fn a_double_click_on_the_strip_toggles_maximize(cx: &mut TestAppContext) {
        // Between "+" (ending at 544px) and the toggle.
        let (host, cx) = strip_with(2, "t0", Maximize::Restored, cx);
        let row = tab_at(&host, 0, cx).center().y;
        assert!(painted(&host, cx).plus_right < px(555.));
        press(point(px(560.), row), 2, cx);
        assert_eq!(presses(&host, cx), ["maximize", "activate"]);
        // On a tab too (flex-layout's tabs cannot be renamed, so theirs reaches the strip).
        press(tab_at(&host, 1, cx).center(), 2, cx);
        assert_eq!(presses(&host, cx), ["maximize", "activate", "maximize", "activate"]);
    }

    #[gpui_kit::test]
    fn a_double_click_on_a_strip_without_the_toggle_only_activates(cx: &mut TestAppContext) {
        let (host, cx) = strip(2, "t0", cx);
        press(point(px(560.), tab_at(&host, 0, cx).center().y), 2, cx);
        assert_eq!(presses(&host, cx), ["activate"]);
    }

    #[gpui_kit::test]
    fn the_maximize_toggle_sits_at_the_strips_end(cx: &mut TestAppContext) {
        // The strip ends at 598px (2px padding); the toolbar keeps 3.6px from it, and the
        // toggle is a 12px icon in 1px padding: its middle is 7px further in.
        let (host, cx) = strip_with(2, "t0", Maximize::Maximized, cx);
        let toggle = point(px(598. - 3.6 - 7.), tab_at(&host, 0, cx).center().y);
        press(toggle, 1, cx);
        assert_eq!(presses(&host, cx), ["maximize"], "the toolbar is not the strip");
        // Left of it, the strip (no "+" there).
        press(point(toggle.x - px(12.), toggle.y), 1, cx);
        assert_eq!(presses(&host, cx), ["maximize", "activate"]);
    }

    fn shown(host: &gpui_kit::Entity<Host>, ix: usize, cx: &mut VisualTestContext) -> bool {
        cx.update(|_, cx| {
            let host = host.read(cx);
            !host.state.hidden(host.count).contains(&ix)
        })
    }

    fn painted(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Painted {
        cx.update(|_, cx| host.read(cx).state.painted.get())
    }

    #[gpui_kit::test]
    fn tabs_that_fit_have_plus_after_the_last_and_no_hidden_tabs_button(cx: &mut TestAppContext) {
        let (host, cx) = strip(2, "t1", cx);
        let painted = painted(&host, cx);
        assert!(!painted.overflowing);
        let last = cx.update(|_, cx| host.read(cx).state.scroll.bounds_for_item(1).expect("laid out"));
        // The tab, the strip's 4px padding, then "+" 5px in and 24px wide.
        assert_eq!(painted.plus_right, last.right() + px(4. + 5. + 24.));
    }

    #[gpui_kit::test]
    fn an_overflowing_strip_keeps_plus_in_view_and_reveals_the_active_tab(cx: &mut TestAppContext) {
        let (host, cx) = strip(25, "t24", cx);
        let painted = painted(&host, cx);
        assert!(painted.overflowing);
        assert!(painted.plus_right <= px(600.), "+ ends at {:?}", painted.plus_right);
        assert!(shown(&host, 24, cx), "the active last tab is in view");
        assert!(!shown(&host, 0, cx), "the first tab is scrolled out");

        // Selecting another tab (from the keyboard or the Hidden tabs menu) brings it into view.
        host.update(cx, |host, cx| {
            host.active = Some("t0".to_owned());
            cx.notify();
        });
        settle(cx);
        assert!(shown(&host, 0, cx));
        assert!(!shown(&host, 24, cx));
        // Flush with the strip's edge, as flex-layout places it: the 4px inset scrolls away.
        assert_eq!(cx.update(|_, cx| host.read(cx).state.scroll.offset().x), px(-4.));
    }

    #[gpui_kit::test]
    fn a_wheel_scroll_moves_the_strip_and_stays(cx: &mut TestAppContext) {
        let (host, cx) = strip(25, "t0", cx);
        let position = point(px(300.), px(15.));
        cx.simulate_event(MouseMoveEvent { position, pressed_button: None, modifiers: Modifiers::default() });
        cx.simulate_event(ScrollEvent {
            position,
            delta: ScrollDelta::Pixels(point(px(0.), px(-150.))),
            modifiers: Modifiers::default(),
            touch_phase: TouchPhase::Moved,
            source: ScrollSource::Pointer,
        });
        settle(cx);
        assert_eq!(cx.update(|_, cx| host.read(cx).state.scroll.offset().x), px(-150.));
        assert!(!shown(&host, 0, cx), "the wheel scrolled the active tab out, and it stays out");

        // Picking the active tab from the Hidden tabs menu brings it back.
        host.update(cx, |host, cx| {
            host.state.reveal_requested.set(true);
            cx.notify();
        });
        settle(cx);
        assert!(shown(&host, 0, cx));
    }

    #[test]
    fn a_dropped_tab_lands_before_the_first_tab_whose_middle_is_right_of_the_pointer() {
        // Three tabs 4px apart, in a row from 2px below the strip's top to its rule at 33px.
        let tab = |left: f32, right: f32| bounds(point(px(left), px(2.)), size(px(right - left), px(31.)));
        let tabs = [("a", tab(100., 200.)), ("b", tab(204., 304.)), ("c", tab(308., 408.))];
        let at = |x: f32, y: f32| landing(&tabs, point(px(x), px(y))).map(|(tab, side)| (*tab, side));
        // Left of a's middle, the strip's edge included: before a.
        assert_eq!(at(97., 15.), Some(("a", Side::Before)));
        assert_eq!(at(149., 15.), Some(("a", Side::Before)));
        // From a's middle to b's, the gap between them included: before b.
        assert_eq!(at(150., 15.), Some(("b", Side::Before)));
        assert_eq!(at(202., 15.), Some(("b", Side::Before)));
        assert_eq!(at(253., 15.), Some(("b", Side::Before)));
        assert_eq!(at(254., 15.), Some(("c", Side::Before)));
        // From the last tab's middle to the strip's end: after it.
        assert_eq!(at(358., 15.), Some(("c", Side::After)));
        assert_eq!(at(500., 15.), Some(("c", Side::After)));
        // Off the tabs' row, above them or on the strip's rule below: after the last one, as
        // flex-layout tests the pointer against each tab's row.
        assert_eq!(at(120., 1.), Some(("c", Side::After)));
        assert_eq!(at(120., 2.), Some(("c", Side::After)));
        assert_eq!(at(120., 33.5), Some(("c", Side::After)));
        assert_eq!(landing::<&str>(&[], point(px(120.), px(15.))), None);
    }

    fn tab_at(host: &gpui_kit::Entity<Host>, ix: usize, cx: &mut VisualTestContext) -> Bounds<Pixels> {
        cx.update(|_, cx| host.read(cx).state.scroll.bounds_for_item(ix).expect("laid out"))
    }

    fn hint(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> DropHint {
        cx.update(|_, cx| host.read(cx).state.drop_hint.borrow().clone())
    }

    fn log(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Vec<String> {
        cx.update(|_, cx| host.read(cx).log.borrow().clone())
    }

    fn hinted(tab: &str, side: Side) -> DropHint {
        Some((tab.to_owned(), side))
    }

    /// Press on `from` and move to `to` with the button held: past the first move the tab is
    /// being dragged, and the next one is over `to`.
    fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(from, None, Modifiers::default());
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        assert!(cx.update(|_, cx| cx.has_active_drag()), "a tab is being dragged");
        drag_to(to, cx);
    }

    fn drag_to(to: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
    }

    #[gpui_kit::test]
    fn a_dragged_tab_lands_between_the_tabs_whose_middles_the_pointer_is_between(cx: &mut TestAppContext) {
        // Two tabs fit the strip.
        let (host, cx) = strip(2, "t0", cx);
        let (first, second) = (tab_at(&host, 0, cx), tab_at(&host, 1, cx));
        let left_of_first = point(first.left() + px(4.), first.center().y);
        drag(second.center(), left_of_first, cx);
        assert_eq!(hint(&host, cx), hinted("t0", Side::Before));
        settle(cx);

        // Past the first tab's middle the dragged tab lands where it is: before itself.
        drag_to(point(first.right() - px(4.), first.center().y), cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::Before));
        drag_to(point(second.right() - px(4.), second.center().y), cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::After));
        drag_to(left_of_first, cx);
        settle(cx);

        cx.simulate_mouse_up(left_of_first, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), ["move t1 t0 Before"], "a drag is not a click");
        assert_eq!(hint(&host, cx), None);
    }

    #[gpui_kit::test]
    fn a_tab_dropped_past_the_last_tab_lands_at_the_end(cx: &mut TestAppContext) {
        // Two tabs fit the strip, with room past "+".
        let (host, cx) = strip(2, "t0", cx);
        let (first, last) = (tab_at(&host, 0, cx), tab_at(&host, 1, cx));
        let plus_right = painted(&host, cx).plus_right;
        let row = last.center().y;
        let (on_plus, empty) = (point(plus_right - px(12.), row), point(px(590.), row));
        assert!(plus_right < px(590.), "+ ends at {plus_right:?}");

        // Over "+", in the strip past it, and in the gap between the last tab and "+".
        drag(first.center(), on_plus, cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::After));
        drag_to(empty, cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::After));
        drag_to(point(last.right() + px(2.), row), cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::After));
        // Over the first tab, but in the strip's 2px inset above the tabs.
        drag_to(point(first.center().x - px(10.), first.top() - px(1.)), cx);
        assert_eq!(hint(&host, cx), hinted("t1", Side::After));
        drag_to(empty, cx);
        settle(cx);
        cx.simulate_mouse_up(empty, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), ["move t0 t1 After"]);
        assert_eq!(hint(&host, cx), None);

        // Dropped on "+".
        drag(first.center(), on_plus, cx);
        settle(cx);
        cx.simulate_mouse_up(on_plus, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), ["move t0 t1 After", "move t0 t1 After"]);
    }

    #[gpui_kit::test]
    fn a_tab_dropped_in_the_gap_between_two_tabs_lands_before_the_right_one(cx: &mut TestAppContext) {
        let (host, cx) = strip(3, "t0", cx);
        let (first, second, third) = (tab_at(&host, 0, cx), tab_at(&host, 1, cx), tab_at(&host, 2, cx));
        let row = second.center().y;
        assert_eq!(third.left() - second.right(), px(4.), "the tabs are 4px apart");
        let gap = point(second.right() + px(2.), row);

        // From the middle tab's right half, across the gap, to the right tab's left half: the
        // hint stays before the right tab.
        drag(first.center(), point(second.right() - px(4.), row), cx);
        assert_eq!(hint(&host, cx), hinted("t2", Side::Before));
        drag_to(gap, cx);
        assert_eq!(hint(&host, cx), hinted("t2", Side::Before));
        drag_to(point(third.left() + px(4.), row), cx);
        assert_eq!(hint(&host, cx), hinted("t2", Side::Before));
        drag_to(gap, cx);
        settle(cx);
        cx.simulate_mouse_up(gap, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), ["move t0 t2 Before"]);
    }

    #[gpui_kit::test]
    fn a_tab_dragged_off_the_strip_lands_nowhere(cx: &mut TestAppContext) {
        let (host, cx) = strip(2, "t0", cx);
        let (first, second) = (tab_at(&host, 0, cx), tab_at(&host, 1, cx));
        drag(second.center(), point(first.left() + px(4.), first.center().y), cx);
        assert_eq!(hint(&host, cx), hinted("t0", Side::Before));

        let below = point(first.center().x, first.bottom() + px(40.));
        drag_to(below, cx);
        assert_eq!(hint(&host, cx), None);
        cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), Vec::<String>::new());
    }

    #[gpui_kit::test]
    fn a_tab_on_an_edge_dock_of_the_layout_lands_nowhere_in_the_strip(cx: &mut TestAppContext) {
        let (host, cx) = strip(2, "t0", cx);
        let (first, second) = (tab_at(&host, 0, cx), tab_at(&host, 1, cx));
        drag(second.center(), point(first.left() + px(4.), first.center().y), cx);
        assert_eq!(hint(&host, cx), hinted("t0", Side::Before));

        // The layout docks it at its edge there: the strip shows nothing, and takes no drop.
        cx.update(|_, cx| host.read(cx).at_edge.set(true));
        let on_dock = point(first.center().x, first.top() + px(2.));
        drag_to(on_dock, cx);
        assert_eq!(hint(&host, cx), None);
        settle(cx);
        cx.simulate_mouse_up(on_dock, MouseButton::Left, Modifiers::default());
        assert_eq!(log(&host, cx), Vec::<String>::new());
    }

    #[gpui_kit::test]
    fn a_tab_scrolled_out_of_the_strip_is_no_drop_target(cx: &mut TestAppContext) {
        let (host, cx) = strip(25, "t0", cx);
        let viewport = cx.update(|_, cx| host.read(cx).state.scroll.bounds());
        let ids: Vec<String> = (0..25).map(|ix| format!("t{ix}")).collect();
        let last_shown = cx.update(|_, cx| {
            let shown = shown_tabs(&host.read(cx).state.scroll, &ids);
            shown.last().map(|(id, _)| (*id).clone()).expect("a tab is shown")
        });
        // The last tab lies past the strip's right edge, under "+" and the Hidden tabs button.
        assert!(tab_at(&host, 24, cx).left() > viewport.right());
        assert_ne!(last_shown, "t24");

        // Over "+" and the Hidden tabs button, the tab lands beside the last tab shown.
        let past = point(viewport.right() + px(20.), viewport.center().y);
        drag(tab_at(&host, 0, cx).center(), past, cx);
        assert_eq!(hint(&host, cx).map(|(tab, _)| tab), Some(last_shown.clone()));
        drag_to(point(past.x, viewport.top() + px(1.)), cx);
        assert_eq!(hint(&host, cx), Some((last_shown, Side::After)));
    }
}
