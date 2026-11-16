//! A workspace's tab layout as flex-layout draws it (`FlexLayoutPane.tsx` on flex-layout's
//! `Layout`): rows of tabsets, side by side or stacked as the rows alternate, each sized by its
//! weight, with a splitter between each two (`Row`, `Splitter`). Dragging a splitter shows
//! where it would go (`flexlayout__splitter_drag`) and, on release, saves the row's new
//! weights. A maximized tabset fills the layout alone, its splitters hidden. The tabsets
//! themselves (strip and content) are the caller's.
//!
//! A tab dragged over a tabset's content docks into it (the middle half) or beside it on the
//! side whose triangle the pointer is in, splitting it; on the edge docks shown in the middle
//! of the layout's four edges it docks along that edge of the whole layout. An outline shows
//! where, and the drop moves it there (flex-layout's `DockLocation`, `enableEdgeDock`). While
//! a tabset is maximized a tab only docks into it.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use gpui_kit::prelude::*;
use gpui_kit::component::ElementExt as _;
use gpui_kit::{
    AnyElement, App, Bounds, Context, DragMoveEvent, EntityId, Hsla, MouseButton, Pixels, Point, SharedString, Window,
    deferred, div, point, px, relative, size,
};
use treeterm_core::layout::{Dock, DropTarget, Edge, LayoutNode, LayoutRow, NodePath, Orientation, SplitDrag, Splitters};

use crate::tab_strip::{DraggedTab, STRIP_HEIGHT};
use crate::theme::colors;

/// What dropping a splitter does: the row (by path) and its children's new weights.
pub type OnResize = Rc<dyn Fn(&[usize], &[f64], &mut Window, &mut App)>;

/// What dropping a dragged tab where it docks does: the tab (by id), and where.
pub type OnDock = Rc<dyn Fn(&str, &DropTarget, &mut Window, &mut App)>;

/// Whether the tab dragged over the layout is on one of its edge docks.
pub type DocksAtEdge = Rc<dyn Fn() -> bool>;

/// One workspace's layout state, kept across renders.
#[derive(Default)]
pub struct TabLayoutState {
    /// Each node's bounds as last painted, by path (flex-layout's `node.getRect()`).
    bounds: Rc<RefCell<HashMap<NodePath, Bounds<Pixels>>>>,
    /// The splitter being dragged.
    drag: Rc<RefCell<Option<Dragging>>>,
    /// Where a tab dragged over the layout would dock.
    tab_over: Rc<RefCell<TabOver>>,
}

impl TabLayoutState {
    /// For the strips: whether the tab dragged over the layout is on one of its edge docks,
    /// which then takes it from the strip under it. The layout, their parent, learns where
    /// the tab is first on each move.
    pub fn docks_at_edge(&self) -> DocksAtEdge {
        let over = self.tab_over.clone();
        Rc::new(move || matches!(&*over.borrow(), TabOver::Docking { target: DropTarget::LayoutEdge(_), .. }))
    }
}

/// A tab dragged over the layout (flex-layout's `dropInfo`).
#[derive(Clone, Debug, Default, PartialEq)]
enum TabOver {
    /// No tab is dragged over the layout.
    #[default]
    Away,
    /// Over it, but where no tab docks: a strip, which shows where the tab lands itself, or a
    /// splitter before any dock.
    Undocked,
    /// It would dock at `target`, where `outline` (from the layout's top left) shows.
    Docking { target: DropTarget, outline: Bounds<Pixels> },
}

/// What a tab dragged to a point is over.
#[derive(Clone, Debug, PartialEq)]
enum Hover {
    /// Out of the layout.
    Outside,
    /// A tabset's strip.
    Strip,
    /// Where no tab docks, a splitter: flex-layout keeps the last dock.
    Between,
    /// Where it docks, and its outline (in the window).
    Dock(DropTarget, Bounds<Pixels>),
}

/// The edge docks' size (flex-layout's `edgeRectLength`, `edgeRectWidth`): 100px along the
/// middle of an edge, 10px into the layout.
const EDGE_LENGTH: f32 = 100.;
const EDGE_WIDTH: f32 = 10.;

/// The layout edge a tab dragged to `pointer` docks along, in a layout drawn at `root`
/// (flex-layout's `RowNode.canDrop` on the root): on its edge docks, within 10px of an edge
/// and 50px of its middle.
fn edge_at(root: Bounds<Pixels>, pointer: Point<Pixels>) -> Option<Edge> {
    let (x, y) = (pointer.x - root.left(), pointer.y - root.top());
    let (width, height) = (root.size.width, root.size.height);
    let (margin, half) = (px(EDGE_WIDTH), px(EDGE_LENGTH / 2.));
    let middle = |at: Pixels, length: Pixels| at > length / 2. - half && at < length / 2. + half;
    if x < margin && middle(y, height) {
        Some(Edge::Left)
    } else if x > width - margin && middle(y, height) {
        Some(Edge::Right)
    } else if y < margin && middle(x, width) {
        Some(Edge::Top)
    } else if y > height - margin && middle(x, width) {
        Some(Edge::Bottom)
    } else {
        None
    }
}

/// The half of `bounds` on `edge`'s side (`DockLocation.getDockRect`).
fn half(bounds: Bounds<Pixels>, edge: Edge) -> Bounds<Pixels> {
    let Bounds { origin, size: whole } = bounds;
    let (width, height) = (whole.width / 2., whole.height / 2.);
    match edge {
        Edge::Top => Bounds { origin, size: size(whole.width, height) },
        Edge::Bottom => Bounds { origin: point(origin.x, origin.y + height), size: size(whole.width, height) },
        Edge::Left => Bounds { origin, size: size(width, whole.height) },
        Edge::Right => Bounds { origin: point(origin.x + width, origin.y), size: size(width, whole.height) },
    }
}

/// What a tab dragged to `pointer` is over (flex-layout's `findDropTargetNode`), in a layout
/// drawn at `root` with its `tabsets` (path and bounds, depth-first; only the maximized one,
/// filling the layout, when `maximized`): one of the layout's edge docks first (none while a
/// tabset is maximized), else the first tabset under it: its strip, or its content, where the
/// tab docks as `Dock::at` finds it (only in its middle while maximized), outlined over the
/// whole tabset or the half on that side. An edge dock is outlined over the quarter of the
/// layout on its side.
fn hover(pointer: Point<Pixels>, root: Bounds<Pixels>, tabsets: &[(NodePath, Bounds<Pixels>)], maximized: bool) -> Hover {
    if !root.contains(&pointer) {
        return Hover::Outside;
    }
    if !maximized && let Some(edge) = edge_at(root, pointer) {
        return Hover::Dock(DropTarget::LayoutEdge(edge), half(half(root, edge), edge));
    }
    let Some((path, tabset)) = tabsets.iter().find(|(_, tabset)| tabset.contains(&pointer)) else {
        return Hover::Between;
    };
    let content = Bounds::from_corners(point(tabset.left(), tabset.top() + px(STRIP_HEIGHT)), tabset.bottom_right());
    if !content.contains(&pointer) {
        return Hover::Strip;
    }
    let fraction = |at: Pixels, from: Pixels, length: Pixels| f64::from((at - from) / length);
    let dock = if maximized {
        Dock::Center
    } else {
        Dock::at(
            fraction(pointer.x, content.left(), content.size.width),
            fraction(pointer.y, content.top(), content.size.height),
        )
    };
    let outline = match dock {
        Dock::Center => *tabset,
        Dock::Edge(edge) => half(*tabset, edge),
    };
    Hover::Dock(DropTarget::TabSet { path: path.clone(), dock }, outline)
}

/// What a tab dropped where the layout takes it (a tabset's content, an edge dock, a
/// splitter) does: it docks where the outline shows, if anywhere.
#[derive(Clone)]
struct TabDrops {
    over: Rc<RefCell<TabOver>>,
    on_dock: OnDock,
    view: EntityId,
}

impl TabDrops {
    fn handler(&self) -> impl Fn(&DraggedTab, &mut Window, &mut App) + 'static {
        let drops = self.clone();
        move |dragged, window, cx| {
            let over = std::mem::take(&mut *drops.over.borrow_mut());
            cx.notify(drops.view);
            if let TabOver::Docking { target, .. } = over {
                (drops.on_dock)(dragged.id(), &target, window, cx);
            }
        }
    }
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
    pub on_dock: OnDock,
    /// Names the layout among the window's, for element ids.
    pub id: &'a str,
    /// The view drawing it, drawn again while a splitter or tab is dragged.
    pub view: EntityId,
}

impl TabLayout<'_> {
    pub fn render(self, state: &TabLayoutState, cx: &App) -> AnyElement {
        let TabLayout { tree, splitters, mut panes, on_resize, on_dock, id, view } = self;
        // A tab dropped, or let go of where nothing takes it, is no longer dragged.
        if !cx.has_active_drag() {
            *state.tab_over.borrow_mut() = TabOver::Away;
        }
        let c = colors(cx);
        let drops = TabDrops { over: state.tab_over.clone(), on_dock, view };
        let dragging = *state.tab_over.borrow() != TabOver::Away;
        let maximized = tree.maximized().map(|tabset| tabset.path.clone());
        let root = match &maximized {
            // `flexlayout__tabset-maximized`: that tabset alone, filling the layout.
            Some(path) => {
                let pane = panes.remove(path).unwrap_or_else(|| div().into_any_element());
                div().size_full().flex().child(tabset_box(pane, dragging, &drops).size_full())
            }
            None => {
                let rows = Rows {
                    splitters,
                    state,
                    id,
                    view,
                    accent: c.accent,
                    splitter: c.bg_tertiary,
                    drops: &drops,
                    dragging,
                };
                let root = rows.row(tree, &mut panes).size_full();
                with_splitter_drag(root, splitters, state, on_resize, view, c.accent)
            }
        };
        let tabsets = tree.tabsets().into_iter().map(|tabset| tabset.path.clone()).collect();
        with_tab_drag(root, tabsets, maximized, state, &drops, c.accent).into_any_element()
    }
}

/// A tabset's place in the layout holding its pane. While a tab is dragged over the layout, a
/// layer over its content takes the drop, so that what the content shows does not (as
/// flex-layout covers the tabs' content while dragging).
fn tabset_box(pane: AnyElement, dragging: bool, drops: &TabDrops) -> gpui_kit::Div {
    div().relative().flex().flex_col().child(pane).when(dragging, |d| {
        d.child(div().absolute().left_0().right_0().bottom_0().top(px(STRIP_HEIGHT)).on_drop(drops.handler()))
    })
}

/// While a tab is dragged over the layout, flex-layout's feedback follows the pointer: the
/// edge docks (not while a tabset is maximized) and the outline of where the tab would dock.
fn with_tab_drag(
    root: gpui_kit::Div,
    tabsets: Vec<NodePath>,
    maximized: Option<NodePath>,
    state: &TabLayoutState,
    drops: &TabDrops,
    accent: Hsla,
) -> gpui_kit::Div {
    let over = state.tab_over.borrow().clone();
    let edges: Vec<AnyElement> = if over != TabOver::Away && maximized.is_none() {
        [Edge::Top, Edge::Left, Edge::Bottom, Edge::Right].into_iter().map(|edge| edge_dock(edge, drops, accent)).collect()
    } else {
        Vec::new()
    };
    // `flexlayout__outline_rect` over a tabset, `flexlayout__outline_rect_edge` at the edge:
    // a 2px accent border round a tint of it, rounded 5px, over everything.
    let outline = match over {
        TabOver::Docking { target, outline } => {
            let tint = match target {
                DropTarget::TabSet { .. } => 0.1,
                DropTarget::LayoutEdge(_) => 0.15,
            };
            let rect = div()
                .absolute()
                .left(outline.origin.x)
                .top(outline.origin.y)
                .w(outline.size.width)
                .h(outline.size.height)
                .border_2()
                .border_color(accent)
                .bg(accent.opacity(tint))
                .rounded(px(5.));
            Some(deferred(rect).with_priority(2))
        }
        TabOver::Away | TabOver::Undocked => None,
    };
    let (bounds, tab_over, view) = (state.bounds.clone(), state.tab_over.clone(), drops.view);
    root.relative()
        .on_drag_move(move |event: &DragMoveEvent<DraggedTab>, _, cx| {
            let placed: Vec<(NodePath, Bounds<Pixels>)> = match &maximized {
                Some(path) => vec![(path.clone(), event.bounds)],
                None => {
                    let painted = bounds.borrow();
                    tabsets.iter().filter_map(|path| Some((path.clone(), *painted.get(path)?))).collect()
                }
            };
            let mut over = tab_over.borrow_mut();
            let next = match hover(event.event.position, event.bounds, &placed, maximized.is_some()) {
                Hover::Outside => TabOver::Away,
                Hover::Strip => TabOver::Undocked,
                Hover::Between => match &*over {
                    TabOver::Away => TabOver::Undocked,
                    kept @ (TabOver::Undocked | TabOver::Docking { .. }) => kept.clone(),
                },
                Hover::Dock(target, outline) => {
                    let origin = outline.origin - event.bounds.origin;
                    TabOver::Docking { target, outline: Bounds { origin, size: outline.size } }
                }
            };
            if *over != next {
                *over = next;
                cx.notify(view);
            }
        })
        .children(edges)
        .children(outline)
}

/// An edge dock (`flexlayout__edge_rect`): 100×10px in the middle of the layout's edge, a
/// tint of the accent with an accent border, rounded on its inner side, its arrow pointing
/// out. A tab dropped there docks along that edge.
fn edge_dock(edge: Edge, drops: &TabDrops, accent: Hsla) -> AnyElement {
    let (length, width) = (px(EDGE_LENGTH), px(EDGE_WIDTH));
    let marker = div()
        .absolute()
        .flex()
        .items_center()
        .justify_center()
        .bg(accent.opacity(0.2))
        .border_1()
        .border_color(accent)
        // `--color-edge-icon`.
        .text_color(gpui_kit::rgb(0xeeeeee))
        .text_size(px(7.))
        .line_height(px(7.))
        .on_drop(drops.handler());
    let (marker, arrow) = match edge {
        Edge::Top => (marker.top_0().left(relative(0.5)).ml(-length / 2.).w(length).h(width).rounded_b(width), "▲"),
        Edge::Bottom => (marker.bottom_0().left(relative(0.5)).ml(-length / 2.).w(length).h(width).rounded_t(width), "▼"),
        Edge::Left => (marker.left_0().top(relative(0.5)).mt(-length / 2.).w(width).h(length).rounded_r(width), "◀"),
        Edge::Right => (marker.right_0().top(relative(0.5)).mt(-length / 2.).w(width).h(length).rounded_l(width), "▶"),
    };
    deferred(marker.child(arrow)).with_priority(1).into_any_element()
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
) -> gpui_kit::Div {
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
}

/// What drawing the rows needs.
struct Rows<'a> {
    splitters: Splitters,
    state: &'a TabLayoutState,
    id: &'a str,
    view: EntityId,
    accent: gpui_kit::Hsla,
    splitter: gpui_kit::Hsla,
    drops: &'a TabDrops,
    /// A tab is dragged over the layout.
    dragging: bool,
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
                LayoutNode::TabSet(tabset) => {
                    let pane = panes.remove(&tabset.path).unwrap_or_else(|| div().into_any_element());
                    tabset_box(pane, self.dragging, self.drops)
                }
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
    /// it, under popups and dialogs. A tab dropped on it docks where the outline last showed
    /// (flex-layout keeps the last dock over a splitter).
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
            .on_drop(self.drops.handler())
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
    type Docks = Rc<RefCell<Vec<(String, DropTarget)>>>;

    /// The layout in a 1004×600 box at the window's corner, each tabset a blank pane with a
    /// tab to drag 100px in from its left and 10px down (in its strip's place).
    struct Host {
        layout: Layout,
        state: TabLayoutState,
        /// Where each pane was drawn.
        panes: Rc<RefCell<HashMap<NodePath, Bounds<Pixels>>>>,
        resizes: Resizes,
        docks: Docks,
    }

    impl Render for Host {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let tree = self.layout.tree();
            let panes = tree
                .tabsets()
                .into_iter()
                .map(|tabset| {
                    let (drawn, path) = (self.panes.clone(), tabset.path.clone());
                    let tab = div()
                        .id(SharedString::from(format!("tab-{path:?}")))
                        .ml(px(100.))
                        .mt(px(10.))
                        .w(px(40.))
                        .h(px(20.))
                        .on_drag(DraggedTab::new("dragged", ">_", "Terminal"), |dragged, _, _, cx| cx.new(|_| dragged.clone()));
                    let pane = div().size_full().child(tab).on_prepaint(move |bounds, _, _| {
                        drawn.borrow_mut().insert(path, bounds);
                    });
                    (tabset.path.clone(), pane.into_any_element())
                })
                .collect();
            let resizes = self.resizes.clone();
            let on_resize: OnResize = Rc::new(move |row, weights, _, _| resizes.borrow_mut().push((row.to_vec(), weights.to_vec())));
            let docks = self.docks.clone();
            let on_dock: OnDock = Rc::new(move |tab, target, _, _| docks.borrow_mut().push((tab.to_owned(), target.clone())));
            let splitters = self.layout.splitters();
            let layout = TabLayout { tree: &tree, splitters, panes, on_resize, on_dock, id: "test", view: cx.entity_id() };
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
            docks: Rc::default(),
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

    fn over(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> TabOver {
        cx.update(|_, cx| host.read(cx).state.tab_over.borrow().clone())
    }

    fn docks(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Vec<(String, DropTarget)> {
        cx.update(|_, cx| host.read(cx).docks.borrow().clone())
    }

    /// Where the tab dragged over the layout would dock, and its outline rounded to the pixel.
    fn docking(host: &gpui_kit::Entity<Host>, cx: &mut VisualTestContext) -> Option<(DropTarget, [f32; 4])> {
        match over(host, cx) {
            TabOver::Docking { target, outline } => {
                let round = |at: Pixels| f32::from(at).round();
                let rect = [outline.left(), outline.top(), outline.size.width, outline.size.height].map(round);
                Some((target, rect))
            }
            TabOver::Away | TabOver::Undocked => None,
        }
    }

    /// Drag the left pane's tab to `to`.
    fn drag_tab(to: Point<Pixels>, cx: &mut VisualTestContext) {
        drag(point(px(120.), px(20.)), to, cx);
    }

    fn move_to(to: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::default());
        settle(cx);
    }

    fn into(path: &[usize], dock: Dock) -> DropTarget {
        DropTarget::TabSet { path: path.to_vec(), dock }
    }

    #[gpui_kit::test]
    fn a_tab_over_a_tabsets_content_is_outlined_where_it_docks_and_docks_there(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // The top right pane (304–1004px across, 0–238.5px down) has its content from 34px down:
        // near its right edge, the right half of the pane.
        drag_tab(point(px(950.), px(130.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[1, 0], Dock::Edge(Edge::Right)), [654., 0., 350., 239.])));
        // In its middle, all of it.
        move_to(point(px(654.), px(136.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[1, 0], Dock::Center), [304., 0., 700., 239.])));
        // Near the bottom of the left pane's content, the bottom half of the left pane.
        move_to(point(px(150.), px(590.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[0], Dock::Edge(Edge::Bottom)), [0., 300., 300., 300.])));
        assert_eq!(docks(&host, cx), Vec::new(), "not before the release");

        move_to(point(px(950.), px(130.)), cx);
        release(point(px(950.), px(130.)), cx);
        assert_eq!(docks(&host, cx), [("dragged".to_owned(), into(&[1, 0], Dock::Edge(Edge::Right)))]);
        assert_eq!(over(&host, cx), TabOver::Away);
    }

    #[gpui_kit::test]
    fn a_tab_on_an_edge_dock_is_outlined_over_that_quarter_of_the_layout_and_docks_along_it(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        let at_edge = cx.update(|_, cx| host.read(cx).state.docks_at_edge());
        let edge = DropTarget::LayoutEdge;
        // The left dock: 10px in, along the middle 100px (250–350px down) of the edge.
        drag_tab(point(px(3.), px(300.)), cx);
        assert_eq!(docking(&host, cx), Some((edge(Edge::Left), [0., 0., 251., 600.])));
        assert!(at_edge());
        // Just above it, the left pane's own left side.
        move_to(point(px(3.), px(245.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[0], Dock::Edge(Edge::Left)), [0., 0., 150., 600.])));
        assert!(!at_edge());
        // The top one lies over the strips.
        move_to(point(px(502.), px(4.)), cx);
        assert_eq!(docking(&host, cx), Some((edge(Edge::Top), [0., 0., 1004., 150.])));
        assert!(at_edge());
        move_to(point(px(1001.), px(300.)), cx);
        assert_eq!(docking(&host, cx), Some((edge(Edge::Right), [753., 0., 251., 600.])));

        move_to(point(px(502.), px(597.)), cx);
        assert_eq!(docking(&host, cx), Some((edge(Edge::Bottom), [0., 450., 1004., 150.])));
        release(point(px(502.), px(597.)), cx);
        assert_eq!(docks(&host, cx), [("dragged".to_owned(), edge(Edge::Bottom))]);
        assert!(!at_edge());
    }

    #[gpui_kit::test]
    fn a_tab_on_a_strip_or_out_of_the_layout_docks_nowhere(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // In the top right pane's strip (the strip shows where it lands).
        drag_tab(point(px(700.), px(20.)), cx);
        assert_eq!(over(&host, cx), TabOver::Undocked);
        release(point(px(700.), px(20.)), cx);
        assert_eq!(over(&host, cx), TabOver::Away);

        // Over a pane, then out of the layout.
        drag_tab(point(px(150.), px(300.)), cx);
        assert_eq!(docking(&host, cx).map(|(target, _)| target), Some(into(&[0], Dock::Center)));
        move_to(point(px(1100.), px(300.)), cx);
        assert_eq!(over(&host, cx), TabOver::Away);
        release(point(px(1100.), px(300.)), cx);
        assert_eq!(docks(&host, cx), Vec::new());
    }

    #[gpui_kit::test]
    fn a_tab_over_a_splitter_keeps_its_last_dock_and_docks_there(cx: &mut TestAppContext) {
        let (host, cx) = host(split(None), cx);
        // Onto the splitter first: nowhere yet.
        drag_tab(point(px(302.), px(300.)), cx);
        assert_eq!(over(&host, cx), TabOver::Undocked);
        // The left pane's right side, then the splitter beside it.
        move_to(point(px(290.), px(300.)), cx);
        let right = Some((into(&[0], Dock::Edge(Edge::Right)), [150., 0., 150., 600.]));
        assert_eq!(docking(&host, cx), right);
        move_to(point(px(302.), px(300.)), cx);
        assert_eq!(docking(&host, cx), right);
        release(point(px(302.), px(300.)), cx);
        assert_eq!(docks(&host, cx), [("dragged".to_owned(), into(&[0], Dock::Edge(Edge::Right)))]);
        assert_eq!(resizes(&host, cx), Vec::new(), "a tab is no splitter drag");
    }

    #[gpui_kit::test]
    fn a_maximized_tabset_takes_a_tab_only_into_its_middle(cx: &mut TestAppContext) {
        let (host, cx) = host(split(Some("bottom")), cx);
        // Where the left edge dock and the content's left side would be: all of the tabset.
        drag_tab(point(px(3.), px(300.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[1, 1], Dock::Center), [0., 0., 1004., 600.])));
        move_to(point(px(990.), px(590.)), cx);
        assert_eq!(docking(&host, cx), Some((into(&[1, 1], Dock::Center), [0., 0., 1004., 600.])));
        release(point(px(990.), px(590.)), cx);
        assert_eq!(docks(&host, cx), [("dragged".to_owned(), into(&[1, 1], Dock::Center))]);
    }

    #[test]
    fn an_edge_dock_takes_10px_along_the_middle_100px_of_its_edge() {
        let root = Bounds { origin: point(px(100.), px(50.)), size: size(px(400.), px(300.)) };
        let at = |x: f32, y: f32| edge_at(root, point(px(x), px(y)));
        // Middles: 300px across, 200px down.
        assert_eq!(at(109.9, 200.), Some(Edge::Left));
        assert_eq!(at(110., 200.), None);
        assert_eq!(at(100., 150.5), Some(Edge::Left));
        assert_eq!(at(100., 150.), None, "not from the dock's very end");
        assert_eq!(at(100., 249.5), Some(Edge::Left));
        assert_eq!(at(100., 250.), None);
        assert_eq!(at(490.5, 200.), Some(Edge::Right));
        assert_eq!(at(490., 200.), None);
        assert_eq!(at(300., 59.), Some(Edge::Top));
        assert_eq!(at(251., 50.), Some(Edge::Top));
        assert_eq!(at(250., 50.), None);
        assert_eq!(at(349., 345.), Some(Edge::Bottom));
        assert_eq!(at(350., 345.), None);
        assert_eq!(at(300., 200.), None);
    }
}
