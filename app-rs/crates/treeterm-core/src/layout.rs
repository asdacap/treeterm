//! A workspace's tab layout: Electron's flex-layout model (`IJsonModel`), saved as JSON in the
//! workspace's `metadata.layoutModel` (`FlexLayoutPane.tsx`, `utils/layoutModel.ts`).
//!
//! Both apps read and write it. The native app draws the layout as flex-layout does (rows of
//! tabsets sized by their weights, [`Layout::tree`]), and saves the layout whenever a tab is
//! added, closed, selected or dragged to another place, a tabset is activated or maximized,
//! or a splitter is dragged, as Electron saves every change to its model. The operations
//! follow flex-layout's own (`addNode`, `deleteTab`, `selectTab`, `moveNode` into a tabset,
//! `setActiveTabset`, `maximizeToggle`, `adjustWeights`, `tidy`), and what the native app does
//! not use (the `global` block, borders, ids, other attributes) is written back as it was read.

use serde_json::{Value, json};

/// A tab as the layout records it (`tabToFlexNode`).
#[derive(Clone, Copy, Debug)]
pub struct LayoutTab<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub application_id: &'a str,
    /// Its application can close it (`app?.canClose ?? false`).
    pub can_close: bool,
}

impl LayoutTab<'_> {
    fn node(&self) -> Value {
        json!({
            "type": "tab",
            "id": self.id,
            "name": self.title,
            "component": self.application_id,
            "enableClose": self.can_close,
        })
    }
}

/// Which side of the tab it is dropped on a dragged tab lands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Before,
    After,
}

/// Where a node of the main layout is: its child indices from the root row (`[]`).
pub type NodePath = Vec<usize>;

/// Which way a row lays out its children: side by side or stacked. The root row is
/// horizontal (unless `rootOrientationVertical`), and each row turns the other way from its
/// parent (`Node.getOrientation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Orientation {
    Horizontal,
    Vertical,
}

impl Orientation {
    fn flip(self) -> Self {
        match self {
            Self::Horizontal => Self::Vertical,
            Self::Vertical => Self::Horizontal,
        }
    }
}

/// A node of the main layout as flex-layout draws it.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutNode {
    Row(LayoutRow),
    TabSet(LayoutTabSet),
}

/// A row (`RowNode`): its children laid out along its orientation, each sized by its weight,
/// with a splitter between each two.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutRow {
    pub path: NodePath,
    /// Its share of its parent row, against its siblings' (`weight`, 100 when unset).
    pub weight: f64,
    pub orientation: Orientation,
    pub children: Vec<LayoutNode>,
}

/// A tabset (`TabSetNode`): a strip of tabs over the selected one.
#[derive(Clone, Debug, PartialEq)]
pub struct LayoutTabSet {
    pub path: NodePath,
    pub weight: f64,
    /// Its tabs' ids, in strip order.
    pub tabs: Vec<String>,
    /// The tab it shows: `None` when it selects nothing (`-1`, or an index past its last tab).
    pub selected: Option<String>,
    /// The layout's active tabset (where a new tab goes).
    pub active: bool,
    pub maximize: Maximize,
}

/// A tabset's maximize toggle (`TabSetNode.canMaximize`, `isMaximized`): offered once the
/// layout has more than one tabset, or while it is maximized.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Maximize {
    /// No toggle: the only tabset, or maximizing is turned off.
    Unavailable,
    /// "Maximize tab set".
    Restored,
    /// It fills the layout, the others hidden: "Restore tab set".
    Maximized,
}

/// The splitters between a row's children (`splitterSize`, and `splitterExtra` more around
/// them that also takes the pointer), in pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Splitters {
    pub size: f64,
    pub extra: f64,
}

/// A splitter being dragged in a row (flex-layout's `Splitter` on a `RowNode`): where it may
/// go, and the weights it leaves, from the children's sizes when the drag began. Positions are
/// of the splitter's leading edge, along the row, in the coordinates `start` is in.
#[derive(Clone, Debug, PartialEq)]
pub struct SplitDrag {
    sizes: Vec<f64>,
    mins: Vec<f64>,
    index: usize,
    splitter: f64,
    start: f64,
}

impl SplitDrag {
    /// The drag of the splitter before child `index` of a row whose children measure `sizes`
    /// along it and may shrink to `mins`, `splitter` apart, the first starting at `start`.
    /// `None` unless the splitter is between two children and the row has a size.
    pub fn new(sizes: Vec<f64>, mins: Vec<f64>, index: usize, splitter: f64, start: f64) -> Option<Self> {
        let valid = sizes.len() == mins.len() && index >= 1 && index < sizes.len() && sizes.iter().sum::<f64>() > 0.;
        valid.then_some(Self { sizes, mins, index, splitter, start })
    }

    /// Where the splitter is when the drag begins (`getSplitterInitials`' `startPosition`).
    pub fn origin(&self) -> f64 {
        self.start + self.sizes[..self.index].iter().sum::<f64>() + self.splitter * (self.index - 1) as f64
    }

    /// `position` kept where the splitter may go (`getSplitterBounds`, `getBoundPosition`):
    /// far enough along to leave each child before it its minimum, and short enough of the
    /// row's end to leave the children after it theirs, with the splitters between.
    pub fn bound(&self, position: f64) -> f64 {
        let end = self.start + self.sizes.iter().sum::<f64>() + self.splitter * (self.sizes.len() - 1) as f64;
        let low = self.start + self.mins[..self.index].iter().sum::<f64>() + self.splitter * (self.index - 1) as f64;
        let high = end - self.mins[self.index..].iter().map(|min| min + self.splitter).sum::<f64>();
        // In this order: in a row too small for its minimums, the far bound wins.
        let position = if position < low { low } else { position };
        if position > high { high } else { position }
    }

    /// The row's weights with the splitter dropped at `position` (`calculateSplit`): the child
    /// on the side it moved away from grows by the distance, and the children on the other
    /// side give it up, the nearest first, each down to its minimum. Weights are percentages
    /// of the row's size when the drag began.
    pub fn weights(&self, position: f64) -> Vec<f64> {
        let (mut sizes, mins, index) = (self.sizes.clone(), &self.mins, self.index);
        let origin = self.origin();
        let (grows, shrinking): (usize, Vec<usize>) = if position < origin {
            (index, (0..index).rev().collect())
        } else {
            (index - 1, (index..sizes.len()).collect())
        };
        let mut shift = (position - origin).abs();
        sizes[grows] += shift;
        for child in shrinking {
            if sizes[child] - shift > mins[child] {
                sizes[child] -= shift;
                break;
            }
            shift -= sizes[child] - mins[child];
            sizes[child] = mins[child];
        }
        let total: f64 = self.sizes.iter().sum();
        sizes.iter().map(|size| size.max(0.1) * 100. / total).collect()
    }
}

impl LayoutNode {
    pub fn path(&self) -> &[usize] {
        match self {
            Self::Row(row) => &row.path,
            Self::TabSet(tabset) => &tabset.path,
        }
    }

    pub fn weight(&self) -> f64 {
        match self {
            Self::Row(row) => row.weight,
            Self::TabSet(tabset) => tabset.weight,
        }
    }

    /// The least it may measure along `along` (`calcMinMaxSize`): a tabset only needs room
    /// for its strip (`strip` high), a row the sum of its children's along its orientation
    /// (with the splitters between) and the largest of them across it.
    pub fn min_size(&self, along: Orientation, splitter: f64, strip: f64) -> f64 {
        match self {
            Self::TabSet(_) => match along {
                Orientation::Horizontal => 0.,
                Orientation::Vertical => strip,
            },
            Self::Row(row) => {
                let mins = row.children.iter().map(|child| child.min_size(along, splitter, strip));
                if along == row.orientation {
                    mins.sum::<f64>() + splitter * row.children.len().saturating_sub(1) as f64
                } else {
                    mins.fold(0., f64::max)
                }
            }
        }
    }
}

impl LayoutRow {
    /// The tabset that fills the layout, if one is maximized.
    pub fn maximized(&self) -> Option<&LayoutTabSet> {
        self.tabsets().into_iter().find(|tabset| tabset.maximize == Maximize::Maximized)
    }

    /// Its tabsets, depth-first.
    pub fn tabsets(&self) -> Vec<&LayoutTabSet> {
        let mut found = Vec::new();
        for child in &self.children {
            match child {
                LayoutNode::TabSet(tabset) => found.push(tabset),
                LayoutNode::Row(row) => found.extend(row.tabsets()),
            }
        }
        found
    }
}

/// A layout model, kept as JSON so that attributes the native app does not know survive.
#[derive(Clone, Debug, PartialEq)]
pub struct Layout(Value);

impl Layout {
    /// A saved model, tidied as flex-layout tidies a model it loads. `None` when it is not
    /// JSON with a `layout` row: Electron then builds the default (`buildModel`).
    pub fn parse(json: &str) -> Option<Self> {
        let model: Value = match serde_json::from_str(json) {
            Ok(model) => model,
            // `buildModel` catches the parse error and falls back to the default model.
            Err(_) => return None,
        };
        if !model.get("layout").is_some_and(Value::is_object) {
            return None;
        }
        let mut layout = Self(model);
        layout.tidy();
        Some(layout)
    }

    /// `createDefaultLayoutModel`: one row holding one active tabset of every tab, in order,
    /// the active tab selected (the first one when there is none or it is not a tab).
    pub fn default_for(tabs: &[LayoutTab<'_>], active: Option<&str>) -> Self {
        let selected = active.and_then(|active| tabs.iter().position(|tab| tab.id == active)).unwrap_or(0);
        let mut layout = Self(json!({
            "global": {
                "tabEnableRename": false,
                "tabEnablePopout": false,
                "tabSetEnableMaximize": true,
                "tabSetEnableDeleteWhenEmpty": true,
                "splitterSize": 4,
                "splitterExtra": 4,
                "tabEnableRenderOnDemand": false,
            },
            "borders": [],
            "layout": {
                "type": "row",
                "children": [{
                    "type": "tabset",
                    "active": true,
                    "selected": selected,
                    "children": tabs.iter().map(LayoutTab::node).collect::<Vec<_>>(),
                }],
            },
        }));
        // As `Model.fromJson` loads it: a tabset without tabs is replaced by an empty one.
        layout.tidy();
        layout
    }

    /// The model as saved in `metadata.layoutModel`.
    pub fn to_json(&self) -> String {
        self.0.to_string()
    }

    /// Tab ids in the order the layout shows them: tabsets depth-first, then the borders.
    pub fn tab_order(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .tabsets()
            .iter()
            .filter_map(|path| self.node(path))
            .flat_map(|tabset| kids(tabset).iter().filter_map(tab_id).map(str::to_owned))
            .collect();
        for border in self.0.get("borders").and_then(Value::as_array).into_iter().flatten() {
            ids.extend(kids(border).iter().filter_map(tab_id).map(str::to_owned));
        }
        ids
    }

    /// The tab the active tabset (else the first one) shows: `None` when it selects nothing
    /// (`-1`, or an index past its last tab).
    pub fn selected_tab(&self) -> Option<String> {
        let tabsets = self.tabsets();
        let tabset = self.node(self.active_tabset(&tabsets).or(tabsets.first())?)?;
        let Ok(index) = usize::try_from(selected(tabset, 0)) else {
            return None; // -1: none
        };
        kids(tabset).get(index).and_then(tab_id).map(str::to_owned)
    }

    /// Bring the layout in line with the workspace's `tabs` (`FlexLayoutPane`'s sync): a tab
    /// it lacks is added at the end of the active tabset (else the first one) and selected
    /// there; a tab that is gone is deleted.
    pub fn reconcile(&mut self, tabs: &[LayoutTab<'_>]) {
        let known = self.tab_order();
        for tab in tabs.iter().filter(|tab| !known.iter().any(|id| id == tab.id)) {
            self.add(tab);
        }
        for id in known.iter().filter(|id| !tabs.iter().any(|tab| tab.id == id.as_str())) {
            self.delete(id);
        }
    }

    /// `Actions.selectTab`: the tab is selected in its tabset, which becomes the active one.
    pub fn select(&mut self, tab: &str) {
        let Some((path, index)) = self.find(tab) else {
            return;
        };
        if let Some(tabset) = self.node_mut(&path) {
            set(tabset, "selected", json!(index));
        }
        self.activate(&path);
    }

    /// Drop `dragged` on `side` of `target` (`Actions.moveNode` into the target's tabset, at
    /// the target's index): it joins that tabset there, selected, and the tabset becomes the
    /// active one; a tabset it leaves empty is deleted. `false`, changing nothing, unless
    /// both are tabs of tabsets.
    pub fn move_tab(&mut self, dragged: &str, target: &str, side: Side) -> bool {
        let (Some((from, from_index)), Some((to, to_index))) = (self.find(dragged), self.find(target)) else {
            return false;
        };
        let mut index = match side {
            Side::Before => to_index,
            Side::After => to_index + 1,
        };
        // The tab leaves first, so a later place in its own tabset moves down one.
        if from == to && from_index < index {
            index -= 1;
        }
        let Some(node) = self.node_mut(&from).and_then(|tabset| remove_child(tabset, from_index, 0)) else {
            return false;
        };
        self.drop_tab(&to, Some(index), node);
        self.tidy();
        true
    }

    /// The workspace's active tab, as `FlexLayoutPane` follows `activeTabId`: selected in its
    /// tabset, which becomes the active one, unless it already is the tab its tabset shows.
    pub fn show(&mut self, tab: &str) {
        let Some((path, index)) = self.find(tab) else {
            return;
        };
        if self.node(&path).is_some_and(|tabset| usize::try_from(selected(tabset, 0)) == Ok(index)) {
            return;
        }
        self.select(tab);
    }

    /// The main layout as flex-layout draws it: rows of tabsets, each with its tabs, the one
    /// it shows, and whether it is active or maximized (the last tabset marked so, as
    /// `Model.fromJson` reads them).
    pub fn tree(&self) -> LayoutRow {
        let tabsets = self.tabsets();
        let root = self.0.get("layout").unwrap_or(&Value::Null);
        let marks = Marks {
            active: self.active_tabset(&tabsets).cloned(),
            maximized: tabsets.iter().rev().find(|path| self.is_maximized(path)).cloned(),
            // `canMaximize`: not the root's only child.
            sole: (kids(root).len() == 1).then(|| vec![0]),
            enabled: self.global("tabSetEnableMaximize").and_then(Value::as_bool).unwrap_or(true),
        };
        let vertical = self.global("rootOrientationVertical").and_then(Value::as_bool).unwrap_or(false);
        let orientation = if vertical { Orientation::Vertical } else { Orientation::Horizontal };
        row_view(root, Vec::new(), orientation, &marks)
    }

    /// The splitters' size and extra hit area (flex-layout's defaults: 8 and 0; Electron's
    /// model sets 4 and 4).
    pub fn splitters(&self) -> Splitters {
        let px = |key: &str, default: f64| self.global(key).and_then(Value::as_f64).unwrap_or(default);
        Splitters { size: px("splitterSize", 8.), extra: px("splitterExtra", 0.) }
    }

    /// `Actions.setActiveTabset` (a press in a tabset's strip or content): the tabset at
    /// `path` becomes the active one. `false`, changing nothing, when it already is or is no
    /// tabset.
    pub fn activate_tabset(&mut self, path: &[usize]) -> bool {
        let tabsets = self.tabsets();
        if !tabsets.iter().any(|tabset| tabset == path) || self.active_tabset(&tabsets).is_some_and(|active| active == path) {
            return false;
        }
        self.activate(path);
        true
    }

    /// `Actions.maximizeToggle` (the tabset's maximize button, or a double-click on its strip):
    /// a maximized tabset is restored; another one fills the layout instead and becomes the
    /// active one. `false`, changing nothing, unless the tabset offers the toggle.
    pub fn toggle_maximize(&mut self, path: &[usize]) -> bool {
        let tree = self.tree();
        let Some(maximize) = tree.tabsets().into_iter().find(|tabset| tabset.path == path).map(|tabset| tabset.maximize) else {
            return false;
        };
        match maximize {
            Maximize::Unavailable => return false,
            Maximize::Maximized => {
                if let Some(Value::Object(tabset)) = self.node_mut(path) {
                    tabset.shift_remove("maximized");
                }
            }
            Maximize::Restored => {
                for other in self.tabsets() {
                    if let Some(Value::Object(tabset)) = self.node_mut(&other) {
                        tabset.shift_remove("maximized");
                    }
                }
                self.activate(path);
                if let Some(tabset) = self.node_mut(path) {
                    set(tabset, "maximized", Value::Bool(true));
                }
            }
        }
        true
    }

    /// `Actions.adjustWeights` (a splitter dropped): the children of the row at `path` take
    /// `weights`, in order. `false`, changing nothing, unless it is a row with as many
    /// children and the weights are numbers.
    pub fn adjust_weights(&mut self, path: &[usize], weights: &[f64]) -> bool {
        if !weights.iter().all(|weight| weight.is_finite()) {
            return false;
        }
        let Some(row) = self.node_mut(path).filter(|node| !is_tabset(node)) else {
            return false;
        };
        let Some(children) = row.get_mut("children").and_then(Value::as_array_mut) else {
            return false;
        };
        if children.len() != weights.len() {
            return false;
        }
        for (child, weight) in children.iter_mut().zip(weights) {
            set(child, "weight", number(*weight));
        }
        true
    }

    fn global(&self, key: &str) -> Option<&Value> {
        self.0.get("global")?.get(key)
    }

    fn is_maximized(&self, path: &[usize]) -> bool {
        self.node(path).is_some_and(|tabset| tabset.get("maximized") == Some(&Value::Bool(true)))
    }

    /// `Actions.addNode(tab, tabset, CENTER, -1, select)` on the active tabset, else the
    /// first one (a tidy layout always has one).
    fn add(&mut self, tab: &LayoutTab<'_>) {
        let tabsets = self.tabsets();
        if let Some(path) = self.active_tabset(&tabsets).or(tabsets.first()) {
            self.drop_tab(path, None, tab.node());
        }
    }

    /// `Actions.deleteTab`: the tab leaves its tabset or border (whose selection follows),
    /// and the layout is tidied.
    fn delete(&mut self, id: &str) {
        if let Some((path, index)) = self.find(id) {
            if let Some(tabset) = self.node_mut(&path)
                && remove_child(tabset, index, 0).is_some()
            {
                self.tidy();
            }
            return;
        }
        for border in self.0.get_mut("borders").and_then(Value::as_array_mut).into_iter().flatten() {
            if let Some(index) = kids(border).iter().position(|tab| tab_id(tab) == Some(id)) {
                // A border selects nothing by default.
                if remove_child(border, index, -1).is_some() {
                    return;
                }
            }
        }
    }

    /// The tabset's `drop` in the centre at `index` (the end when `None`): the tab goes
    /// there, selected, and the tabset becomes the active one.
    fn drop_tab(&mut self, path: &[usize], index: Option<usize>, tab: Value) {
        let Some(tabset) = self.node_mut(path) else {
            return;
        };
        let Some(children) = children_mut(tabset) else {
            return;
        };
        let index = index.unwrap_or(children.len()).min(children.len());
        children.insert(index, tab);
        set(tabset, "selected", json!(index));
        self.activate(path);
    }

    /// Make the tabset at `path` the only active one.
    fn activate(&mut self, path: &[usize]) {
        for other in self.tabsets() {
            if let Some(Value::Object(tabset)) = self.node_mut(&other) {
                if other == path {
                    tabset.insert("active".to_owned(), Value::Bool(true));
                } else {
                    tabset.shift_remove("active");
                }
            }
        }
    }

    /// flex-layout's `tidy`: empty tabsets are deleted (unless they keep themselves,
    /// `enableDeleteWhenEmpty`), rows left without children are deleted and rows left with
    /// one are replaced by it, and a root left empty gets a new, active tabset.
    fn tidy(&mut self) {
        let deletes_empty = self
            .0
            .get("global")
            .and_then(|global| global.get("tabSetEnableDeleteWhenEmpty"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let Some(root) = self.0.get_mut("layout") else {
            return;
        };
        tidy_row(root, deletes_empty);
        if kids(root).is_empty() {
            set(root, "children", json!([{ "type": "tabset", "selected": -1, "children": [], "active": true }]));
        }
    }

    /// The tabsets of the main layout, depth-first, as child indices from its root row.
    fn tabsets(&self) -> Vec<Vec<usize>> {
        fn walk(row: &Value, path: &mut Vec<usize>, found: &mut Vec<Vec<usize>>) {
            for (index, child) in kids(row).iter().enumerate() {
                path.push(index);
                if is_tabset(child) {
                    found.push(path.clone());
                } else {
                    walk(child, path, found);
                }
                path.pop();
            }
        }
        let mut found = Vec::new();
        if let Some(root) = self.0.get("layout") {
            walk(root, &mut Vec::new(), &mut found);
        }
        found
    }

    /// The tabset marked active (flex-layout keeps the last one so marked).
    fn active_tabset<'a>(&self, tabsets: &'a [Vec<usize>]) -> Option<&'a Vec<usize>> {
        tabsets
            .iter()
            .rev()
            .find(|path| self.node(path).is_some_and(|tabset| tabset.get("active") == Some(&Value::Bool(true))))
    }

    /// The tabset holding `tab` and its index there.
    fn find(&self, tab: &str) -> Option<(Vec<usize>, usize)> {
        self.tabsets().into_iter().find_map(|path| {
            let index = kids(self.node(&path)?).iter().position(|node| tab_id(node) == Some(tab))?;
            Some((path, index))
        })
    }

    fn node(&self, path: &[usize]) -> Option<&Value> {
        path.iter().try_fold(self.0.get("layout")?, |node, index| kids(node).get(*index))
    }

    fn node_mut(&mut self, path: &[usize]) -> Option<&mut Value> {
        let mut node = self.0.get_mut("layout")?;
        for index in path {
            node = node.get_mut("children")?.get_mut(*index)?;
        }
        Some(node)
    }
}

/// What [`Layout::tree`] marks on its tabsets.
struct Marks {
    active: Option<NodePath>,
    maximized: Option<NodePath>,
    /// The root's only child, which offers no maximize toggle.
    sole: Option<NodePath>,
    /// `tabSetEnableMaximize`.
    enabled: bool,
}

fn row_view(row: &Value, path: NodePath, orientation: Orientation, marks: &Marks) -> LayoutRow {
    let children = kids(row)
        .iter()
        .enumerate()
        .map(|(index, child)| {
            let mut path = path.clone();
            path.push(index);
            if is_tabset(child) {
                LayoutNode::TabSet(tabset_view(child, path, marks))
            } else {
                LayoutNode::Row(row_view(child, path, orientation.flip(), marks))
            }
        })
        .collect();
    LayoutRow { weight: weight(row), path, orientation, children }
}

fn tabset_view(tabset: &Value, path: NodePath, marks: &Marks) -> LayoutTabSet {
    let shown = usize::try_from(selected(tabset, 0)).ok().and_then(|index| kids(tabset).get(index));
    let enabled = tabset.get("enableMaximize").and_then(Value::as_bool).unwrap_or(marks.enabled);
    let maximize = if marks.maximized.as_ref() == Some(&path) {
        Maximize::Maximized
    } else if !enabled || marks.sole.as_ref() == Some(&path) {
        Maximize::Unavailable
    } else {
        Maximize::Restored
    };
    LayoutTabSet {
        weight: weight(tabset),
        tabs: kids(tabset).iter().filter_map(tab_id).map(str::to_owned).collect(),
        selected: shown.and_then(tab_id).map(str::to_owned),
        active: marks.active.as_ref() == Some(&path),
        maximize,
        path,
    }
}

/// flex-layout's `RowNode.tidy` below the root (see [`Layout::tidy`]).
fn tidy_row(row: &mut Value, deletes_empty: bool) {
    let Some(children) = row.get_mut("children").and_then(Value::as_array_mut) else {
        return;
    };
    let mut index = 0;
    while index < children.len() {
        let child = &mut children[index];
        if is_tabset(child) {
            let deletes = child.get("enableDeleteWhenEmpty").and_then(Value::as_bool).unwrap_or(deletes_empty);
            if kids(child).is_empty() && deletes {
                children.remove(index);
            } else {
                index += 1;
            }
            continue;
        }
        // Anything else is a row (`RowNode.fromJson`).
        tidy_row(child, deletes_empty);
        match kids(child).len() {
            0 => {
                children.remove(index);
            }
            // What replaces it is looked at again.
            1 => {
                let row = children.remove(index);
                for (offset, node) in lift(row).into_iter().enumerate() {
                    children.insert(index + offset, node);
                }
            }
            _ => index += 1,
        }
    }
}

/// What replaces a row with one child: that child with the row's weight, or, when it is a
/// row too, its children sharing the row's weight in their own proportions.
fn lift(row: Value) -> Vec<Value> {
    let row_weight = weight(&row);
    let Some(mut only) = take_kids(row).pop() else {
        return Vec::new();
    };
    if is_tabset(&only) {
        set(&mut only, "weight", number(row_weight));
        return vec![only];
    }
    let mut nodes = take_kids(only);
    let total: f64 = nodes.iter().map(weight).sum();
    if total > 0. {
        for node in &mut nodes {
            let share = row_weight * weight(node) / total;
            set(node, "weight", number(share));
        }
    }
    nodes
}

/// A number as `JSON.stringify` writes it: a whole one without a fraction.
fn number(value: f64) -> Value {
    let whole = value.trunc();
    if whole == value && whole.abs() < 9_007_199_254_740_992. { json!(whole as i64) } else { json!(value) }
}

/// `adjustSelectedIndex` after child `removed` of a tabset or border left: an earlier
/// selection moves down one; the selected tab's place passes to the next tab (the last, when
/// it was the last), or to none when no tab is left. Returns the removed child.
fn remove_child(parent: &mut Value, removed: usize, unselected: i64) -> Option<Value> {
    let children = parent.get_mut("children").and_then(Value::as_array_mut)?;
    if removed >= children.len() {
        return None;
    }
    let node = children.remove(removed);
    let (left, removed) = (children.len() as i64, removed as i64);
    let current = selected(parent, unselected);
    let next = match current {
        -1 => -1,
        current if removed == current && left > 0 => current.min(left - 1),
        current if removed < current => current - 1,
        current if removed > current => current,
        _ => -1,
    };
    if next != current {
        set(parent, "selected", json!(next));
    }
    Some(node)
}

/// A tabset's or border's `selected`, `default` when unset.
fn selected(node: &Value, default: i64) -> i64 {
    node.get("selected").and_then(Value::as_i64).unwrap_or(default)
}

fn weight(node: &Value) -> f64 {
    node.get("weight").and_then(Value::as_f64).unwrap_or(100.)
}

fn is_tabset(node: &Value) -> bool {
    node.get("type").and_then(Value::as_str) == Some("tabset")
}

fn tab_id(node: &Value) -> Option<&str> {
    node.get("id").and_then(Value::as_str)
}

fn kids(node: &Value) -> &[Value] {
    node.get("children").and_then(Value::as_array).map_or(&[], Vec::as_slice)
}

/// A node's children, an empty list made for a node without any.
fn children_mut(node: &mut Value) -> Option<&mut Vec<Value>> {
    node.as_object_mut()?.entry("children").or_insert_with(|| json!([])).as_array_mut()
}

fn take_kids(node: Value) -> Vec<Value> {
    match node {
        Value::Object(mut node) => match node.shift_remove("children") {
            Some(Value::Array(children)) => children,
            Some(_) | None => Vec::new(),
        },
        _ => Vec::new(),
    }
}

fn set(node: &mut Value, key: &str, value: Value) {
    if let Value::Object(node) = node {
        node.insert(key.to_owned(), value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Written by Electron's flex-layout (`Model.toJson`): `t1 t2 | (t3 / t4)`, `t2`
    /// selected in the active left tabset.
    const SPLIT: &str = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}],"active":true},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}]},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}]}]}]},"popouts":{}}"##;

    fn tab(id: &str) -> LayoutTab<'_> {
        LayoutTab { id, title: id, application_id: "terminal", can_close: true }
    }

    fn split() -> Layout {
        Layout::parse(SPLIT).expect("valid")
    }

    fn model(layout: &Layout) -> Value {
        serde_json::from_str(&layout.to_json()).expect("json")
    }

    /// What Electron's flex-layout wrote after the same action on [`SPLIT`], as a value.
    fn electron(json: &str) -> Value {
        serde_json::from_str(json).expect("fixture")
    }

    fn tabs(ids: &[&'static str]) -> Vec<LayoutTab<'static>> {
        ids.iter().map(|id| tab(id)).collect()
    }

    #[test]
    fn a_saved_layout_round_trips_with_everything_it_holds() {
        let layout = split();
        assert_eq!(model(&layout), electron(SPLIT));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("t2"));
    }

    #[test]
    fn a_model_that_is_not_a_layout_is_not_read() {
        assert_eq!(Layout::parse("not json"), None);
        assert_eq!(Layout::parse(""), None);
        assert_eq!(Layout::parse("[]"), None);
        assert_eq!(Layout::parse(r#"{"global":{}}"#), None);
    }

    #[test]
    fn the_default_layout_is_electrons() {
        let layout = Layout::default_for(&[tab("a"), tab("b")], Some("b"));
        let expected = json!({
            "global": {
                "tabEnableRename": false, "tabEnablePopout": false, "tabSetEnableMaximize": true,
                "tabSetEnableDeleteWhenEmpty": true, "splitterSize": 4, "splitterExtra": 4,
                "tabEnableRenderOnDemand": false,
            },
            "borders": [],
            "layout": {"type": "row", "children": [{
                "type": "tabset", "active": true, "selected": 1,
                "children": [
                    {"type": "tab", "id": "a", "name": "a", "component": "terminal", "enableClose": true},
                    {"type": "tab", "id": "b", "name": "b", "component": "terminal", "enableClose": true},
                ],
            }]},
        });
        assert_eq!(model(&layout), expected);
        // `Math.max(0, findIndex)`: no active tab, or one that is not a tab, selects the first.
        assert_eq!(Layout::default_for(&tabs(&["a", "b"]), Some("gone")).selected_tab().as_deref(), Some("a"));
        assert_eq!(Layout::default_for(&tabs(&["a", "b"]), None).selected_tab().as_deref(), Some("a"));
        let unclosable = LayoutTab { id: "x", title: "X", application_id: "other", can_close: false };
        assert_eq!(model(&Layout::default_for(&[unclosable], None))["layout"]["children"][0]["children"][0]["enableClose"], false);
    }

    #[test]
    fn a_default_layout_without_tabs_has_one_empty_tabset() {
        let layout = Layout::default_for(&[], None);
        assert_eq!(
            model(&layout)["layout"]["children"],
            json!([{ "type": "tabset", "selected": -1, "children": [], "active": true }])
        );
        assert_eq!(layout.selected_tab(), None);
        assert_eq!(layout.tab_order(), Vec::<String>::new());
    }

    #[test]
    fn the_selected_tab_is_the_active_tabsets_else_the_first_tabsets() {
        assert_eq!(split().selected_tab().as_deref(), Some("t2"));
        let mut layout = split();
        layout.select("t4");
        assert_eq!(layout.selected_tab().as_deref(), Some("t4"));

        let none_active = SPLIT.replace(r#","active":true"#, "");
        assert_eq!(Layout::parse(&none_active).unwrap().selected_tab().as_deref(), Some("t2"));
        let unset = none_active.replace(r#""selected":1,"#, "");
        assert_eq!(Layout::parse(&unset).unwrap().selected_tab().as_deref(), Some("t1"), "selected defaults to 0");
        let past_the_end = none_active.replace(r#""selected":1"#, r#""selected":5"#);
        assert_eq!(Layout::parse(&past_the_end).unwrap().selected_tab(), None);
        let nothing = none_active.replace(r#""selected":1"#, r#""selected":-1"#);
        assert_eq!(Layout::parse(&nothing).unwrap().selected_tab(), None);
    }

    #[test]
    fn selecting_a_tab_makes_its_tabset_the_only_active_one() {
        let mut layout = split();
        layout.select("t4");
        // flex-layout's `selectTab('t4')` on SPLIT, with t4's tabset's `selected` written out.
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}]},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}]},{"type":"tabset","id":"#bottom","weight":50,"selected":0,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}],"active":true}]}]},"popouts":{}}"##;
        assert_eq!(model(&layout), electron(expected));

        let unchanged = model(&layout);
        layout.select("gone");
        assert_eq!(model(&layout), unchanged);
    }

    #[test]
    fn a_new_tab_goes_to_the_end_of_the_active_tabset_selected() {
        let mut layout = split();
        layout.reconcile(&tabs(&["t1", "t2", "t3", "t4", "t5"]));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t5", "t3", "t4"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("t5"));
        assert_eq!(model(&layout)["layout"]["children"][0]["selected"], 2);

        // With no tabset active, to the first one, which becomes active.
        let mut layout = Layout::parse(&SPLIT.replace(r#","active":true"#, "")).unwrap();
        layout.reconcile(&tabs(&["t1", "t2", "t3", "t4", "t5"]));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t5", "t3", "t4"]);
        assert_eq!(model(&layout)["layout"]["children"][0]["active"], true);
    }

    #[test]
    fn a_closed_tab_leaves_and_its_empty_tabset_and_row_with_it() {
        // flex-layout's `deleteTab('t3')` on SPLIT: the column row is left with t4's tabset,
        // which takes its place and weight.
        let mut layout = split();
        layout.reconcile(&tabs(&["t1", "t2", "t4"]));
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}],"active":true},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}]}]},"popouts":{}}"##;
        assert_eq!(model(&layout), electron(expected));
    }

    #[test]
    fn closing_a_tab_keeps_the_selection_on_the_same_tab_or_the_next() {
        // Before the selected t2: the selection moves down to it.
        let mut layout = split();
        layout.reconcile(&tabs(&["t2", "t3", "t4"]));
        assert_eq!(model(&layout)["layout"]["children"][0]["selected"], 0);
        assert_eq!(layout.selected_tab().as_deref(), Some("t2"));
        // The selected t2 itself, the last: the one before it.
        let mut layout = split();
        layout.reconcile(&tabs(&["t1", "t3", "t4"]));
        assert_eq!(model(&layout)["layout"]["children"][0]["selected"], 0);
        assert_eq!(layout.selected_tab().as_deref(), Some("t1"));
        // The selected first of three: the next one takes its place.
        let mut layout = Layout::default_for(&tabs(&["a", "b", "c"]), Some("a"));
        layout.reconcile(&tabs(&["b", "c"]));
        assert_eq!(layout.selected_tab().as_deref(), Some("b"));
        // After it: unchanged.
        let mut layout = Layout::default_for(&tabs(&["a", "b", "c"]), Some("a"));
        layout.reconcile(&tabs(&["a", "b"]));
        assert_eq!(layout.selected_tab().as_deref(), Some("a"));
    }

    #[test]
    fn closing_every_tab_leaves_one_new_empty_tabset() {
        let mut layout = split();
        layout.reconcile(&[]);
        assert_eq!(
            model(&layout)["layout"],
            json!({"type": "row", "id": "#root", "children": [{"type": "tabset", "selected": -1, "children": [], "active": true}]})
        );
        // And a new tab goes there.
        layout.reconcile(&tabs(&["n"]));
        assert_eq!(layout.tab_order(), ["n"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("n"));
    }

    #[test]
    fn a_tabset_that_keeps_itself_stays_empty() {
        let keeps = SPLIT.replace(r##""id":"#top","##, r##""id":"#top","enableDeleteWhenEmpty":false,"##);
        let mut layout = Layout::parse(&keeps).unwrap();
        layout.reconcile(&tabs(&["t1", "t2", "t4"]));
        let column = &model(&layout)["layout"]["children"][1];
        assert_eq!(column["id"], "#column");
        assert_eq!(column["children"][0]["children"], json!([]));
        assert_eq!(column["children"][0]["selected"], -1);

        // Globally too.
        let keeps = SPLIT.replace(r#""global":{"#, r#""global":{"tabSetEnableDeleteWhenEmpty":false,"#);
        let mut layout = Layout::parse(&keeps).unwrap();
        layout.reconcile(&tabs(&["t1", "t2", "t4"]));
        assert_eq!(model(&layout)["layout"]["children"][1]["children"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn a_row_left_with_one_row_hands_its_weight_to_that_rows_children() {
        // root: [a | row(w 60): [row(w 50): [b, c] , row(w 50): [d]]] with d closed.
        let nested = json!({
            "global": {}, "borders": [],
            "layout": {"type": "row", "children": [
                {"type": "tabset", "children": [{"type": "tab", "id": "a"}], "active": true},
                {"type": "row", "weight": 60, "children": [
                    {"type": "row", "weight": 50, "children": [
                        {"type": "tabset", "weight": 30, "children": [{"type": "tab", "id": "b"}]},
                        {"type": "tabset", "weight": 10, "children": [{"type": "tab", "id": "c"}]},
                    ]},
                    {"type": "tabset", "weight": 50, "children": [{"type": "tab", "id": "d"}]},
                ]},
            ]},
        });
        let mut layout = Layout::parse(&nested.to_string()).unwrap();
        layout.reconcile(&tabs(&["a", "b", "c"]));
        let root = model(&layout)["layout"]["children"].clone();
        assert_eq!(root.as_array().map(Vec::len), Some(3));
        assert_eq!(root[1]["weight"], 45.);
        assert_eq!(root[2]["weight"], 15.);
        assert_eq!(layout.tab_order(), ["a", "b", "c"]);
    }

    #[test]
    fn a_dragged_tab_moves_within_its_tabset_and_is_selected() {
        // flex-layout's `moveNode('t1', 'main', CENTER, 2)`.
        let mut layout = split();
        assert!(layout.move_tab("t1", "t2", Side::After));
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true},{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true}],"active":true},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}]},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}]}]}]},"popouts":{}}"##;
        assert_eq!(model(&layout), electron(expected));

        let mut layout = Layout::default_for(&tabs(&["a", "b", "c"]), Some("a"));
        assert!(layout.move_tab("c", "a", Side::Before));
        assert_eq!(layout.tab_order(), ["c", "a", "b"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("c"));
        assert!(layout.move_tab("c", "b", Side::After));
        assert_eq!(layout.tab_order(), ["a", "b", "c"]);
        assert!(layout.move_tab("a", "c", Side::Before));
        assert_eq!(layout.tab_order(), ["b", "a", "c"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("a"));
    }

    #[test]
    fn a_tab_dragged_into_another_tabset_leaves_its_empty_tabset() {
        // flex-layout's `moveNode('t4', 'main', CENTER, 0)`: t4's tabset goes, t3's takes
        // the column's place.
        let mut layout = split();
        assert!(layout.move_tab("t4", "t1", Side::Before));
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true},{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}],"active":true},{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}]}]},"popouts":{}}"##;
        let mut written = model(&layout);
        // flex-layout leaves `selected` out at its default, 0.
        assert_eq!(written["layout"]["children"][0]["selected"], 0);
        written["layout"]["children"][0].as_object_mut().unwrap().shift_remove("selected");
        assert_eq!(written, electron(expected));
        assert_eq!(layout.tab_order(), ["t4", "t1", "t2", "t3"]);
    }

    #[test]
    fn a_tab_dragged_out_of_the_active_tabset_takes_the_activation() {
        // flex-layout's `moveNode('t2', '#top', CENTER, 0)`.
        let mut layout = split();
        assert!(layout.move_tab("t2", "t3", Side::Before));
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true}]},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true},{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}],"active":true},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}]}]}]},"popouts":{}}"##;
        let mut written = model(&layout);
        // flex-layout leaves `selected` out at its default, 0.
        for pointer in ["/layout/children/0", "/layout/children/1/children/0"] {
            let tabset = written.pointer_mut(pointer).unwrap();
            assert_eq!(tabset["selected"], 0);
            tabset.as_object_mut().unwrap().shift_remove("selected");
        }
        assert_eq!(written, electron(expected));
        assert_eq!(layout.selected_tab().as_deref(), Some("t2"));
    }

    #[test]
    fn a_tab_dropped_on_itself_stays_and_is_selected() {
        let mut layout = split();
        assert!(layout.move_tab("t1", "t1", Side::Before));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4"]);
        assert_eq!(layout.selected_tab().as_deref(), Some("t1"));
        assert!(layout.move_tab("t1", "t1", Side::After));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4"]);
    }

    #[test]
    fn a_drag_of_or_onto_an_unknown_tab_changes_nothing() {
        let mut layout = split();
        assert!(!layout.move_tab("gone", "t1", Side::Before));
        assert!(!layout.move_tab("t1", "gone", Side::After));
        assert_eq!(model(&layout), electron(SPLIT));
    }

    fn tabset(layout: &LayoutRow, path: &[usize]) -> LayoutTabSet {
        layout.tabsets().into_iter().find(|tabset| tabset.path == path).cloned().expect("a tabset there")
    }

    #[test]
    fn a_split_layout_is_drawn_as_its_rows_and_tabsets() {
        let tree = split().tree();
        assert_eq!((tree.path.clone(), tree.weight, tree.orientation), (vec![], 100., Orientation::Horizontal));
        let left = LayoutTabSet {
            path: vec![0],
            weight: 50.,
            tabs: vec!["t1".into(), "t2".into()],
            selected: Some("t2".into()),
            active: true,
            maximize: Maximize::Restored,
        };
        let top = LayoutTabSet {
            path: vec![1, 0],
            weight: 50.,
            tabs: vec!["t3".into()],
            // Unset: the first tab.
            selected: Some("t3".into()),
            active: false,
            maximize: Maximize::Restored,
        };
        let bottom = LayoutTabSet { path: vec![1, 1], tabs: vec!["t4".into()], selected: Some("t4".into()), ..top.clone() };
        let column = LayoutRow {
            path: vec![1],
            weight: 50.,
            orientation: Orientation::Vertical,
            children: vec![LayoutNode::TabSet(top), LayoutNode::TabSet(bottom)],
        };
        assert_eq!(tree.children, vec![LayoutNode::TabSet(left), LayoutNode::Row(column)]);
        assert_eq!(tree.maximized(), None);
        assert_eq!(split().splitters(), Splitters { size: 4., extra: 4. });
    }

    #[test]
    fn a_layout_without_settings_draws_with_flex_layouts_defaults() {
        let bare = json!({
            "global": { "rootOrientationVertical": true }, "borders": [],
            "layout": { "type": "row", "children": [
                { "type": "row", "children": [
                    { "type": "tabset", "selected": -1, "children": [{ "type": "tab", "id": "a" }] },
                    { "type": "tabset", "selected": 3, "children": [{ "type": "tab", "id": "b" }] },
                ]},
            ]},
        });
        let layout = Layout::parse(&bare.to_string()).unwrap();
        assert_eq!(layout.splitters(), Splitters { size: 8., extra: 0. });
        let tree = layout.tree();
        assert_eq!(tree.orientation, Orientation::Vertical);
        let LayoutNode::Row(row) = &tree.children[0] else { panic!("a row") };
        assert_eq!((row.weight, row.orientation), (100., Orientation::Horizontal));
        // Selecting nothing, or past the last tab: none shown. No tabset marked active.
        let (a, b) = (tabset(&tree, &[0, 0]), tabset(&tree, &[0, 1]));
        assert_eq!((a.selected, b.selected, a.active, b.active), (None, None, false, false));
        // The root's only child is a row: its tabsets can be maximized.
        assert_eq!((a.maximize, b.maximize), (Maximize::Restored, Maximize::Restored));
    }

    #[test]
    fn the_only_tabset_offers_no_maximize() {
        let tree = Layout::default_for(&tabs(&["a", "b"]), Some("a")).tree();
        assert_eq!(tabset(&tree, &[0]).maximize, Maximize::Unavailable);
        let mut layout = Layout::default_for(&tabs(&["a"]), None);
        assert!(!layout.toggle_maximize(&[0]));

        // Nor any tabset with maximizing turned off, globally or on itself.
        let off = SPLIT.replace(r#""global":{"#, r#""global":{"tabSetEnableMaximize":false,"#);
        assert_eq!(tabset(&Layout::parse(&off).unwrap().tree(), &[0]).maximize, Maximize::Unavailable);
        let on = off.replace(r##""id":"#top","##, r##""id":"#top","enableMaximize":true,"##);
        let tree = Layout::parse(&on).unwrap().tree();
        assert_eq!((tabset(&tree, &[1, 0]).maximize, tabset(&tree, &[1, 1]).maximize), (Maximize::Restored, Maximize::Unavailable));
    }

    #[test]
    fn maximizing_a_tabset_makes_it_fill_the_layout_and_active_until_restored() {
        let mut layout = split();
        assert!(layout.toggle_maximize(&[1, 0]));
        // flex-layout's `maximizeToggle('#top')` on SPLIT.
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}]},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}],"active":true,"maximized":true},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}]}]}]},"popouts":{}}"##;
        assert_eq!(model(&layout), electron(expected));
        let tree = layout.tree();
        assert_eq!(tree.maximized().map(|tabset| tabset.path.clone()), Some(vec![1, 0]));
        assert!(tabset(&tree, &[1, 0]).active);
        assert_eq!(tabset(&tree, &[0]).maximize, Maximize::Restored);

        // Another one takes its place.
        assert!(layout.toggle_maximize(&[0]));
        let tree = layout.tree();
        assert_eq!(tree.maximized().map(|tabset| tabset.path.clone()), Some(vec![0]));
        assert_eq!(model(&layout)["layout"]["children"][1]["children"][0].get("maximized"), None);

        // Restored: the layout as it was, the tabset still the active one.
        assert!(layout.toggle_maximize(&[0]));
        assert_eq!(layout.tree().maximized(), None);
        assert_eq!(model(&layout), electron(SPLIT));

        // Not a tabset.
        assert!(!layout.toggle_maximize(&[1]));
        assert!(!layout.toggle_maximize(&[5]));
        assert_eq!(model(&layout), electron(SPLIT));
    }

    #[test]
    fn of_several_tabsets_marked_maximized_the_last_one_is() {
        let both = SPLIT
            .replace(r##""id":"#top","##, r##""id":"#top","maximized":true,"##)
            .replace(r##""id":"#bottom","##, r##""id":"#bottom","maximized":true,"##);
        let mut layout = Layout::parse(&both).unwrap();
        assert_eq!(layout.tree().maximized().map(|tabset| tabset.path.clone()), Some(vec![1, 1]));
        assert_eq!(tabset(&layout.tree(), &[1, 0]).maximize, Maximize::Restored);
        // Maximizing one clears every other mark.
        assert!(layout.toggle_maximize(&[0]));
        let written = model(&layout);
        assert_eq!(written["layout"]["children"][1]["children"][0].get("maximized"), None);
        assert_eq!(written["layout"]["children"][1]["children"][1].get("maximized"), None);
    }

    #[test]
    fn a_maximized_tabset_closed_empty_takes_the_maximize_with_it() {
        let mut layout = split();
        assert!(layout.toggle_maximize(&[1, 1]));
        layout.reconcile(&tabs(&["t1", "t2", "t3"]));
        assert_eq!(layout.tree().maximized(), None);
    }

    #[test]
    fn activating_a_tabset_moves_the_activation_to_it() {
        let mut layout = split();
        assert!(layout.activate_tabset(&[1, 1]));
        // flex-layout's `setActiveTabset('#bottom')` on SPLIT.
        let expected = r##"{"global":{"splitterSize":4,"splitterExtra":4,"tabEnableRename":false,"tabEnableRenderOnDemand":false},"borders":[],"layout":{"type":"row","id":"#root","children":[{"type":"tabset","id":"main","weight":50,"selected":1,"children":[{"type":"tab","id":"t1","name":"T t1","component":"terminal","enableClose":true},{"type":"tab","id":"t2","name":"T t2","component":"terminal","enableClose":true}]},{"type":"row","id":"#column","weight":50,"children":[{"type":"tabset","id":"#top","weight":50,"children":[{"type":"tab","id":"t3","name":"T t3","component":"terminal","enableClose":true}]},{"type":"tabset","id":"#bottom","weight":50,"children":[{"type":"tab","id":"t4","name":"T t4","component":"terminal","enableClose":true}],"active":true}]}]},"popouts":{}}"##;
        assert_eq!(model(&layout), electron(expected));
        // The tab the layout reports as shown is now the new active tabset's.
        assert_eq!(layout.selected_tab().as_deref(), Some("t4"));

        // Already active, a row, or nothing there: unchanged.
        assert!(!layout.activate_tabset(&[1, 1]));
        assert!(!layout.activate_tabset(&[1]));
        assert!(!layout.activate_tabset(&[]));
        assert!(!layout.activate_tabset(&[7]));
        assert_eq!(model(&layout), electron(expected));
    }

    #[test]
    fn the_active_tab_is_shown_without_moving_the_activation_when_its_tabset_shows_it() {
        // t3 is what its tabset shows already: the left tabset stays the active one.
        let mut layout = split();
        layout.show("t3");
        assert_eq!(model(&layout), electron(SPLIT));
        layout.show("gone");
        assert_eq!(model(&layout), electron(SPLIT));
        // t1 is not: selected, its tabset (already active) keeps the activation.
        layout.show("t1");
        assert_eq!(model(&layout)["layout"]["children"][0]["selected"], 0);
        // From an inactive tabset, the selection moves the activation, as `selectTab` does.
        let mut layout = Layout::parse(&SPLIT.replace(r##""id":"#bottom","##, r##""id":"#bottom","selected":-1,"##)).unwrap();
        layout.show("t4");
        assert_eq!(layout.selected_tab().as_deref(), Some("t4"));
    }

    #[test]
    fn dropping_a_splitter_saves_the_rows_weights() {
        let mut layout = split();
        assert!(layout.adjust_weights(&[], &[30., 70.]));
        assert!(layout.adjust_weights(&[1], &[25.5, 74.5]));
        let written = model(&layout);
        assert_eq!(written["layout"]["children"][0]["weight"], json!(30));
        assert_eq!(written["layout"]["children"][1]["weight"], json!(70));
        assert_eq!(written["layout"]["children"][1]["children"][0]["weight"], json!(25.5));
        assert_eq!(written["layout"]["children"][1]["children"][1]["weight"], json!(74.5));
        let tree = layout.tree();
        assert_eq!(tree.children.iter().map(LayoutNode::weight).collect::<Vec<_>>(), [30., 70.]);

        // Not a row, the wrong count, or not numbers: unchanged.
        let before = model(&layout);
        assert!(!layout.adjust_weights(&[0], &[]));
        assert!(!layout.adjust_weights(&[1], &[50.]));
        assert!(!layout.adjust_weights(&[4], &[50., 50.]));
        assert!(!layout.adjust_weights(&[], &[f64::NAN, 50.]));
        assert_eq!(model(&layout), before);
    }

    #[test]
    fn a_row_needs_room_for_its_strips_and_splitters() {
        let tree = split().tree();
        let root = LayoutNode::Row(tree.clone());
        // Side by side: tabsets need no width, the splitter its own.
        assert_eq!(root.min_size(Orientation::Horizontal, 4., 34.), 4.);
        // Stacked in the column: two strips and the splitter between.
        assert_eq!(root.min_size(Orientation::Vertical, 4., 34.), 72.);
        assert_eq!(tree.children[0].min_size(Orientation::Vertical, 4., 34.), 34.);
        assert_eq!(tree.children[0].path(), [0]);
        assert_eq!(tree.children[1].path(), [1]);
    }

    #[test]
    fn a_splitter_moves_within_its_row_and_shares_the_row_by_the_childrens_new_sizes() {
        // Two panes 300px and 700px wide, a 4px splitter between.
        let drag = SplitDrag::new(vec![300., 700.], vec![0., 0.], 1, 4., 0.).unwrap();
        assert_eq!(drag.origin(), 300.);
        assert_eq!(drag.weights(300.), [30., 70.]);
        assert_eq!(drag.weights(200.), [20., 80.]);
        assert_eq!(drag.weights(500.), [50., 50.]);
        // As far as the row's start, and as far as its end leaves room for the splitter.
        assert_eq!(drag.bound(-50.), 0.);
        assert_eq!(drag.bound(2000.), 1000.);
        assert_eq!(drag.bound(420.), 420.);
        // From 100px in, in the row's coordinates.
        let drag = SplitDrag::new(vec![300., 700.], vec![0., 0.], 1, 4., 100.).unwrap();
        assert_eq!((drag.origin(), drag.bound(0.)), (400., 100.));
        assert_eq!(drag.weights(300.), [20., 80.]);
    }

    #[test]
    fn a_splitter_pushed_past_its_neighbour_takes_from_the_next_one() {
        // Three stacked panes 100px high, each needing its 34px strip.
        let drag = SplitDrag::new(vec![100., 100., 100.], vec![34., 34., 34.], 2, 4., 0.).unwrap();
        assert_eq!(drag.origin(), 204.);
        // Up by 100: the middle pane gives 66 down to its strip, the first one the rest.
        let weights = drag.weights(104.);
        let expected = [66., 34., 200.].map(|size| size * 100. / 300.);
        assert!(weights.iter().zip(expected).all(|(got, want)| (got - want).abs() < 1e-9), "{weights:?}");
        // Down by 50: the last pane gives it.
        let weights = drag.weights(254.);
        let expected = [100., 150., 50.].map(|size| size * 100. / 300.);
        assert!(weights.iter().zip(expected).all(|(got, want)| (got - want).abs() < 1e-9), "{weights:?}");
        // No further than every pane before it at its minimum (and the splitter between), nor
        // past the last one's.
        assert_eq!(drag.bound(10.), 72.);
        assert_eq!(drag.bound(299.), 270.);
        // A collapsed pane still keeps a sliver.
        let drag = SplitDrag::new(vec![100., 100.], vec![0., 0.], 1, 4., 0.).unwrap();
        assert_eq!(drag.weights(0.), [0.05, 100.]);
    }

    #[test]
    fn a_splitter_drag_needs_a_splitter_and_a_sized_row() {
        assert_eq!(SplitDrag::new(vec![100., 100.], vec![0., 0.], 0, 4., 0.), None);
        assert_eq!(SplitDrag::new(vec![100., 100.], vec![0., 0.], 2, 4., 0.), None);
        assert_eq!(SplitDrag::new(vec![100., 100.], vec![0.], 1, 4., 0.), None);
        assert_eq!(SplitDrag::new(vec![0., 0.], vec![0., 0.], 1, 4., 0.), None);
        // In a row too small for its minimums the far bound wins.
        let drag = SplitDrag::new(vec![20., 20.], vec![34., 34.], 1, 4., 0.).unwrap();
        assert_eq!(drag.bound(-10.), 44. - 38.);
    }

    #[test]
    fn tabs_in_borders_count_and_leave_but_do_not_move() {
        let bordered = SPLIT.replace(
            r#""borders":[]"#,
            r#""borders":[{"type":"border","location":"left","selected":1,"children":[{"type":"tab","id":"b1"},{"type":"tab","id":"b2"}]}]"#,
        );
        let mut layout = Layout::parse(&bordered).unwrap();
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4", "b1", "b2"]);
        assert!(!layout.move_tab("b1", "t1", Side::Before));
        layout.reconcile(&tabs(&["t1", "t2", "t3", "t4", "b2"]));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4", "b2"]);
        assert_eq!(model(&layout)["borders"][0]["selected"], 0);
    }
}
