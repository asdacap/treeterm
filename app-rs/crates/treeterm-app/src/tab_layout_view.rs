//! A workspace's tab layout as flex-layout draws it (`FlexLayoutPane.tsx` on flex-layout's
//! `Layout`): rows of tabsets, side by side or stacked as the rows alternate, each sized by its
//! weight, with a splitter between each two (`Row`, `Splitter`). Dragging a splitter shows
//! where it would go (`flexlayout__splitter_drag`) and, on release, saves the row's new
//! weights. A maximized tabset fills the layout alone, its splitters hidden. The tabsets
//! themselves (strip and content) are the caller's.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::component::ElementExt as _;
use gpui_kit::{
    AnyElement, App, Bounds, Context, DragMoveEvent, EntityId, MouseButton, Pixels, Point, SharedString, Window,
    deferred, div, px,
};
use treeterm_core::layout::{LayoutNode, LayoutRow, NodePath, Orientation, SplitDrag, Splitters};

use crate::tab_strip::STRIP_HEIGHT;
use crate::theme::colors;

/// What dropping a splitter does: the row (by path) and its children's new weights.
pub type OnResize = Rc<dyn Fn(&[usize], &[f64], &mut Window, &mut App)>;

/// One workspace's layout state, kept across renders.
#[derive(Default)]
pub struct TabLayoutState {
    /// Each node's bounds as last painted, by path (flex-layout's `node.getRect()`).
    bounds: Rc<RefCell<HashMap<NodePath, Bounds<Pixels>>>>,
    /// The splitter being dragged.
    drag: Rc<RefCell<Option<Dragging>>>,
}

/// A splitter pressed, and where it would drop.
struct Dragging {
    row: NodePath,
    orientation: Orientation,
    split: SplitDrag,
    /// How far into the splitter it was pressed, along the row (`dragStartX` / `Y`).
    grab: f64,
    /// Where its leading edge would go, kept where it may go.
    position: f64,
    /// The splitter where it was pressed, for the outline's place across the row.
    splitter: Bounds<Pixels>,
}

/// The value GPUI drags for a splitter: nothing is drawn at the pointer, the outline shows
/// where the splitter would go.
#[derive(Clone)]
pub struct SplitterDrag;

impl Render for SplitterDrag {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        gpui_kit::Empty
    }
}

/// Where a point is along a row.
fn along(orientation: Orientation, point: Point<Pixels>) -> f64 {
    f64::from(f32::from(match orientation {
        Orientation::Horizontal => point.x,
        Orientation::Vertical => point.y,
    }))
}

/// How far a node reaches along a row.
fn extent(orientation: Orientation, bounds: Bounds<Pixels>) -> f64 {
    f64::from(f32::from(match orientation {
        Orientation::Horizontal => bounds.size.width,
        Orientation::Vertical => bounds.size.height,
    }))
}

/// A layout to draw.
pub struct TabLayout<'a> {
    pub tree: &'a LayoutRow,
    pub splitters: Splitters,
    /// Its tabsets as drawn, by path.
    pub panes: HashMap<NodePath, AnyElement>,
    pub on_resize: OnResize,
    /// Names the layout among the window's, for element ids.
    pub id: &'a str,
    /// The view drawing it, drawn again while a splitter is dragged.
    pub view: EntityId,
}

impl TabLayout<'_> {
    pub fn render(self, state: &TabLayoutState, cx: &App) -> AnyElement {
        let TabLayout { tree, splitters, mut panes, on_resize, id, view } = self;
        // `flexlayout__tabset-maximized`: that tabset alone, filling the layout.
        if let Some(maximized) = tree.maximized() {
            let pane = panes.remove(&maximized.path).unwrap_or_else(|| div().into_any_element());
            return div().size_full().flex().child(pane).into_any_element();
        }
        let c = colors(cx);
        let rows = Rows { splitters, state, id, view, accent: c.accent, splitter: c.bg_tertiary };
        let root = rows.row(tree, &mut panes).size_full();
        with_splitter_drag(root, splitters, state, on_resize, view, c.accent)
    }
}

/// While a splitter is dragged, the outline follows the pointer along its row, and the
/// release drops it there.
fn with_splitter_drag(
    root: gpui_kit::Div,
    splitters: Splitters,
    state: &TabLayoutState,
    on_resize: OnResize,
    view: EntityId,
    accent: gpui_kit::Hsla,
) -> AnyElement {
    // `flexlayout__splitter_drag`: the splitter's place across the row, the pointer's along it.
    let outline = state.drag.borrow().as_ref().and_then(|dragging| {
        let origin = state.bounds.borrow().get(&Vec::new()).copied()?;
        let offset = |point: f64, from: Pixels| px((point - f64::from(f32::from(from))) as f32);
        let size = px(splitters.size as f32);
        let bar = div().absolute().rounded(px(5.)).bg(accent);
        Some(match dragging.orientation {
            Orientation::Horizontal => bar
                .left(offset(dragging.position, origin.left()))
                .top(dragging.splitter.top() - origin.top())
                .w(size)
                .h(dragging.splitter.size.height),
            Orientation::Vertical => bar
                .top(offset(dragging.position, origin.top()))
                .left(dragging.splitter.left() - origin.left())
                .h(size)
                .w(dragging.splitter.size.width),
        })
    });
    let moved = state.drag.clone();
    let dropped = state.drag.clone();
    let finish = Rc::new(move |window: &mut Window, cx: &mut App| {
        let Some(dragging) = dropped.borrow_mut().take() else {
            return;
        };
        cx.notify(view);
        on_resize(&dragging.row, &dragging.split.weights(dragging.position), window, cx);
    });
    let finish_out = finish.clone();
    root.relative()
        .children(outline)
        .on_drag_move(move |event: &DragMoveEvent<SplitterDrag>, _, cx| {
            let mut drag = moved.borrow_mut();
            let Some(dragging) = drag.as_mut() else {
                return;
            };
            let position = dragging.split.bound(along(dragging.orientation, event.event.position) - dragging.grab);
            if position != dragging.position {
                dragging.position = position;
                cx.notify(view);
            }
        })
        // Wherever the button comes up, inside the layout or out of it.
        .capture_any_mouse_up(move |event, window, cx| {
            if event.button == MouseButton::Left {
                finish(window, cx);
            }
        })
        .on_mouse_up_out(MouseButton::Left, move |_, window, cx| finish_out(window, cx))
        .into_any_element()
}

/// What drawing the rows needs.
struct Rows<'a> {
    splitters: Splitters,
    state: &'a TabLayoutState,
    id: &'a str,
    view: EntityId,
    accent: gpui_kit::Hsla,
    splitter: gpui_kit::Hsla,
}

impl Rows<'_> {
    /// A row (`flexlayout__row`): its children along its orientation, each growing by its
    /// weight from nothing (`flex-basis: 0`, `flex-grow: max(1, weight × 1000)`), with a
    /// splitter between each two.
    fn row(&self, row: &LayoutRow, panes: &mut HashMap<NodePath, AnyElement>) -> gpui_kit::Div {
        let mut element = match row.orientation {
            Orientation::Horizontal => div().flex().flex_row(),
            Orientation::Vertical => div().flex().flex_col(),
        };
        let size = self.splitters.size;
        let mins: Vec<f64> = row.children.iter().map(|child| child.min_size(row.orientation, size, STRIP_HEIGHT.into())).collect();
        for (index, child) in row.children.iter().enumerate() {
            if index > 0 {
                element = element.child(self.splitter(row, index, &mins));
            }
            let node = match child {
                LayoutNode::Row(child) => self.row(child, panes),
                LayoutNode::TabSet(tabset) => div()
                    .flex()
                    .flex_col()
                    .child(panes.remove(&tabset.path).unwrap_or_else(|| div().into_any_element())),
            };
            let (min_width, min_height) = (
                child.min_size(Orientation::Horizontal, size, STRIP_HEIGHT.into()),
                child.min_size(Orientation::Vertical, size, STRIP_HEIGHT.into()),
            );
            let mut node = node.flex_basis(px(0.)).min_w(px(min_width as f32)).min_h(px(min_height as f32)).overflow_hidden();
            node.style().flex_grow = Some((child.weight() * 1000.).max(1.) as f32);
            element = element.child(self.measured(node, child.path().to_vec()));
        }
        self.measured(element, row.path.clone())
    }

    /// `element`, its bounds kept under `path` each frame.
    fn measured(&self, element: gpui_kit::Div, path: NodePath) -> gpui_kit::Div {
        let bounds = self.state.bounds.clone();
        element.on_prepaint(move |painted, _, _| {
            bounds.borrow_mut().insert(path, painted);
        })
    }

    /// The splitter before child `index` of `row` (`flexlayout__splitter`): `size` thick in
    /// the splitter colour, accent while the pointer is on it. It takes the pointer `extra`
    /// beyond itself, half on each side (`flexlayout__splitter_extra`), over the panes beside
    /// it, under popups and dialogs.
    fn splitter(&self, row: &LayoutRow, index: usize, mins: &[f64]) -> impl IntoElement {
        let Splitters { size, extra } = self.splitters;
        let orientation = row.orientation;
        let name = format!("splitter-{}-{:?}-{index}", self.id, row.path);
        let group = SharedString::from(name.clone());
        let accent = self.accent;
        let bar = div().absolute().group_hover(group.clone(), move |s| s.bg(accent));
        let band = div().id(SharedString::from(name)).group(group).absolute().occlude();
        let (band, bar) = match orientation {
            Orientation::Horizontal => (
                band.top_0().bottom_0().left(px((-extra / 2.) as f32)).w(px((size + extra) as f32)).cursor_col_resize(),
                bar.top_0().bottom_0().left(px((extra / 2.) as f32)).w(px(size as f32)),
            ),
            Orientation::Vertical => (
                band.left_0().right_0().top(px((-extra / 2.) as f32)).h(px((size + extra) as f32)).cursor_row_resize(),
                bar.left_0().right_0().top(px((extra / 2.) as f32)).h(px(size as f32)),
            ),
        };
        let press = {
            let (bounds, drag, view) = (self.state.bounds.clone(), self.state.drag.clone(), self.view);
            let row_path = row.path.clone();
            let children: Vec<NodePath> = row.children.iter().map(|child| child.path().to_vec()).collect();
            let mins = mins.to_vec();
            move |event: &gpui_kit::MouseDownEvent, _: &mut Window, cx: &mut App| {
                let painted = bounds.borrow();
                let Some(sized) = children.iter().map(|path| painted.get(path).copied()).collect::<Option<Vec<_>>>() else {
                    return;
                };
                let sizes = sized.iter().map(|child| extent(orientation, *child)).collect();
                let Some(split) = SplitDrag::new(sizes, mins.clone(), index, size, along(orientation, sized[0].origin)) else {
                    return;
                };
                // Between the children it separates, across the whole row.
                let (before, after) = (sized[index - 1], sized[index]);
                let splitter = Bounds::from_corners(
                    match orientation {
                        Orientation::Horizontal => gpui_kit::point(before.right(), before.top()),
                        Orientation::Vertical => gpui_kit::point(before.left(), before.bottom()),
                    },
                    match orientation {
                        Orientation::Horizontal => gpui_kit::point(after.left(), after.bottom()),
                        Orientation::Vertical => gpui_kit::point(after.right(), after.top()),
                    },
                );
                let origin = split.origin();
                *drag.borrow_mut() = Some(Dragging {
                    row: row_path.clone(),
                    orientation,
                    grab: along(orientation, event.position) - origin,
                    position: origin,
                    split,
                    splitter,
                });
                cx.notify(view);
            }
        };
        let band = band
            .on_mouse_down(MouseButton::Left, press)
            .on_drag(SplitterDrag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            .child(bar);
        let placeholder = div().relative().flex_shrink_0().bg(self.splitter);
        match orientation {
            Orientation::Horizontal => placeholder.w(px(size as f32)),
            Orientation::Vertical => placeholder.h(px(size as f32)),
        }
        .child(deferred(band))
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{Modifiers, TestAppContext, VisualTestContext, point};
    use treeterm_core::layout::Layout;

    use super::*;

    /// `left | (top / bottom)` as Electron writes it: 30 and 70 wide, the column 40 and 60
    /// high, 4px splitters taking the pointer 2px beyond each side.
    fn split(maximized: Option<&str>) -> Layout {
        let tabset = |id: &str, weight: u32| {
            let mut tabset = serde_json::json!({ "type": "tabset", "id": id, "weight": weight, "children": [{ "type": "tab", "id": format!("tab-{id}") }] });
            if maximized == Some(id) {
                tabset["maximized"] = true.into();
            }
            tabset
        };
        let model = serde_json::json!({
            "global": { "splitterSize": 4, "splitterExtra": 4 }, "borders": [],
            "layout": { "type": "row", "children": [
                tabset("left", 30),
                { "type": "row", "weight": 70, "children": [tabset("top", 40), tabset("bottom", 60)] },
            ]},
        });
        Layout::parse(&model.to_string()).expect("a layout")
    }

    type Resizes = Rc<RefCell<Vec<(NodePath, Vec<f64>)>>>;

    /// The layout in a 1004×600 box at the window's corner, each tabset a blank pane.
    struct Host {
        layout: Layout,
        state: TabLayoutState,
        /// Where each pane was drawn.
        panes: Rc<RefCell<HashMap<NodePath, Bounds<Pixels>>>>,
        resizes: Resizes,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let tree = self.layout.tree();
            let panes = tree
                .tabsets()
                .into_iter()
                .map(|tabset| {
                    let (drawn, path) = (self.panes.clone(), tabset.path.clone());
                    let pane = div().size_full().on_prepaint(move |bounds, _, _| {
                        drawn.borrow_mut().insert(path, bounds);
                    });
                    (tabset.path.clone(), pane.into_any_element())
                })
                .collect();
            let resizes = self.resizes.clone();
            let on_resize: OnResize = Rc::new(move |row, weights, _, _| resizes.borrow_mut().push((row.to_vec(), weights.to_vec())));
            let layout = TabLayout { tree: &tree, splitters: self.layout.splitters(), panes, on_resize, id: "test", view: cx.entity_id() };
            div().w(px(1004.)).h(px(600.)).flex().child(layout.render(&self.state, cx))
        }
    }

    fn settle(cx: &mut VisualTestContext) {
        for _ in 0..3 {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.draw(cx).clear(cx);
                window.simulate_next_frame(cx);
            });
        }
    }

    fn host(layout: Layout, cx: &mut TestAppContext) -> (gpui_kit::Entity<Host>, &mut VisualTestContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::install);
        let (host, cx) = cx.add_window_view(move |_, _| Host {
            layout,
            state: TabLayoutState::default(),
            panes: Rc::default(),
            resizes: Rc::default(),
        });
        settle(cx);
        (host, cx)
    }

    fn pane(host: &gpui_kit::Entity<Host>, path: &[usize], cx: &mut VisualTestContext) -> Option<Bounds<Pixels>> {
        cx.update(|_, cx| host.read(cx).panes.borrow().get(path).copied())
    }

    fn resizes(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Vec<(NodePath, Vec<f64>)> {
        cx.update(|_, cx| host.read(cx).resizes.borrow().clone())
    }

    fn dragging(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Option<f64> {
        cx.update(|_, cx| host.read(cx).state.drag.borrow().as_ref().map(|dragging| dragging.position))
    }

    /// Press at `from`, drag to `to` (past the first move the drag is GPUI's), release there.
    fn drag(from: Point<Pixels>, to: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(from, None, Modifiers::default());
        cx.simulate_mouse_down(from, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        settle(cx);
    }

    fn release(at: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_up(at, MouseButton::Left, Modifiers::default());
        settle(cx);
    }

    fn close(left: &[f64], right: &[f64]) -> bool {
        left.len() == right.len() && left.iter().zip(right).all(|(a, b)| (a - b).abs() < 1e-3)
    }

    #[gpui_kit::test]
    fn the_tabsets_share_their_rows_by_weight_around_the_splitters(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // 1000px across for 30 and 70 after the 4px splitter; 596px down for 40 and 60.
        let (left, top, bottom) =
            (pane(&host, &[0], cx).unwrap(), pane(&host, &[1, 0], cx).unwrap(), pane(&host, &[1, 1], cx).unwrap());
        assert_eq!((left.origin.x, left.size.width, left.size.height), (px(0.), px(300.), px(600.)));
        assert_eq!((top.origin.x, top.size.width), (px(304.), px(700.)));
        // Laid out to the half pixel.
        assert!((f32::from(top.size.height) - 238.4).abs() <= 0.5, "{top:?}");
        assert!((f32::from(bottom.origin.y) - 242.4).abs() <= 0.5, "{bottom:?}");
        assert!((f32::from(bottom.size.height) - 357.6).abs() <= 0.5, "{bottom:?}");
    }

    #[gpui_kit::test]
    fn a_dragged_splitter_shows_where_it_goes_and_saves_the_weights_where_dropped(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // Pressed 2px into the splitter (300–304px), dragged 100px right.
        drag(point(px(302.), px(300.)), point(px(402.), px(300.)), cx);
        assert_eq!(dragging(&host, cx), Some(400.), "the outline's leading edge");
        assert_eq!(resizes(&host, cx), Vec::new(), "not before the release");
        assert_eq!(pane(&host, &[0], cx).unwrap().size.width, px(300.), "the panes keep their size meanwhile");
        release(point(px(402.), px(300.)), cx);
        let saved = resizes(&host, cx);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, Vec::<usize>::new());
        assert!(close(&saved[0].1, &[40., 60.]), "{saved:?}");
        assert_eq!(dragging(&host, cx), None);
    }

    #[gpui_kit::test]
    fn a_splitter_takes_the_pointer_beside_it_too(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // 1px into the column, inside the splitter's extra 2px.
        drag(point(px(305.), px(300.)), point(px(205.), px(300.)), cx);
        release(point(px(205.), px(300.)), cx);
        let saved = resizes(&host, cx);
        assert!(saved.len() == 1 && close(&saved[0].1, &[20., 80.]), "{saved:?}");

        // 3px into it, past the extra: the pane's, not the splitter's.
        drag(point(px(307.), px(300.)), point(px(207.), px(300.)), cx);
        release(point(px(207.), px(300.)), cx);
        assert_eq!(resizes(&host, cx).len(), 1);
    }

    #[gpui_kit::test]
    fn a_stacked_splitter_leaves_each_pane_its_strip(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // The column's splitter, 238.4–242.4px down, dragged to the top: the top pane keeps
        // its 34px strip.
        drag(point(px(700.), px(240.)), point(px(700.), px(5.)), cx);
        assert_eq!(dragging(&host, cx), Some(34.));
        release(point(px(700.), px(5.)), cx);
        let saved = resizes(&host, cx);
        assert_eq!(saved[0].0, vec![1]);
        assert!(close(&saved[0].1, &[34. * 100. / 596., 562. * 100. / 596.]), "{saved:?}");
    }

    #[gpui_kit::test]
    fn a_splitter_released_outside_the_layout_drops_at_its_bound(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        drag(point(px(302.), px(300.)), point(px(1200.), px(300.)), cx);
        // As far as the row's end leaves room for the splitter.
        assert_eq!(dragging(&host, cx), Some(1000.));
        release(point(px(1200.), px(300.)), cx);
        let saved = resizes(&host, cx);
        assert!(saved.len() == 1 && close(&saved[0].1, &[100., 0.01]), "{saved:?}");
    }

    #[gpui_kit::test]
    fn a_maximized_tabset_fills_the_layout_alone(cx: &mut TestAppContext) {
        let (host, cx) = host(split(Some("bottom")), cx);
        let bottom = pane(&host, &[1, 1], cx).unwrap();
        assert_eq!((bottom.origin, bottom.size.width, bottom.size.height), (point(px(0.), px(0.)), px(1004.), px(600.)));
        assert_eq!((pane(&host, &[0], cx), pane(&host, &[1, 0], cx)), (None, None));
        // No splitter to drag.
        drag(point(px(302.), px(300.)), point(px(402.), px(300.)), cx);
        release(point(px(402.), px(300.)), cx);
        assert_eq!(resizes(&host, cx), Vec::new());
    }
}
