//! A tabset's header (`FlexLayoutPane.tsx` on flex-layout's tab strip): the tabs, then "+"
//! (`flexlayout-new-tab-btn`). Tabs that do not fit scroll sideways under the wheel, the
//! selected tab is scrolled into view, and a "Hidden tabs" button lists the tabs out of view
//! (flex-layout's `useTabOverflow`); "+" is a sticky button that stays at the right end.

use std::cell::Cell;
use std::rc::Rc;

use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ElementExt as _, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::{AnyElement, Anchor, App, Bounds, EntityId, Pixels, ScrollHandle, SharedString, Window, div, px};
use treeterm_model::workspace_store::ActivityState;

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

/// One strip's tabs and handlers, drawn with the [`TabStripState`] its view keeps.
pub struct TabStrip {
    pub tabs: Vec<StripTab>,
    pub active: Option<String>,
    /// The new-tab menu; without any, there is no "+".
    pub apps: Vec<NewTabApp>,
    pub on_select: OnId,
    pub on_close: OnId,
    pub on_new_tab: OnId,
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

        let mut tabs = Vec::with_capacity(self.tabs.len());
        for tab in &self.tabs {
            let is_active = self.active.as_deref() == Some(tab.id.as_str());
            let group = SharedString::from(format!("tab-group-{}", tab.id));
            let text = if is_active { primary } else { secondary };
            let (on_select, on_close) = (self.on_select.clone(), self.on_close.clone());
            let (select_id, close_id) = (tab.id.clone(), tab.id.clone());
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
                    .on_click(move |_, window, cx| on_select(&select_id, window, cx)),
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
            div().h(px(31.)).flex_shrink_0().px(px(3.6)).flex().items_center().child(
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

        // `flexlayout__tabset_tabbar_outer`: 33px plus a 1px rule.
        h_flex()
            .h(px(34.))
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
                    .children(overflow)
                    .on_prepaint(check),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Context, Modifiers, MouseMoveEvent, Render, ScrollDelta, ScrollEvent, ScrollSource, TestAppContext, TouchPhase, VisualTestContext, bounds, point, size};

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
            let strip = TabStrip {
                tabs,
                active: self.active.clone(),
                apps: vec![NewTabApp { id: "terminal".to_owned(), name: "Terminal".to_owned(), icon: ">_".to_owned() }],
                on_select: none.clone(),
                on_close: none.clone(),
                on_new_tab: none,
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
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let active = Some(active.to_owned());
        let (host, cx) = cx.add_window_view(move |_, _| Host { state: TabStripState::default(), count, active });
        settle(cx);
        (host, cx)
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
}
