//! A workspace's tab layout: Electron's flex-layout model (`IJsonModel`), saved as JSON in the
//! workspace's `metadata.layoutModel` (`FlexLayoutPane.tsx`, `utils/layoutModel.ts`).
//!
//! Both apps read and write it. The native app shows the layout's tabs in one strip, tabsets
//! depth-first, and saves the layout whenever a tab is added, closed or selected, as Electron
//! saves every change to its model. The operations follow flex-layout's own (`addNode`,
//! `deleteTab`, `selectTab`, `tidy`), and what the native app does not use (the `global` block, borders, ids, weights, other
//! attributes) is written back as it was read, so Electron restores its splits.

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

    /// `Actions.addNode(tab, tabset, CENTER, -1, select)` on the active tabset, else the
    /// first one (a tidy layout always has one).
    fn add(&mut self, tab: &LayoutTab<'_>) {
        let tabsets = self.tabsets();
        if let Some(path) = self.active_tabset(&tabsets).or(tabsets.first()) {
            self.drop_tab(path, tab.node());
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

    /// The tabset's `drop` in the centre at the end: the tab goes there, selected, and the
    /// tabset becomes the active one.
    fn drop_tab(&mut self, path: &[usize], tab: Value) {
        let Some(tabset) = self.node_mut(path) else {
            return;
        };
        let Some(children) = children_mut(tabset) else {
            return;
        };
        children.push(tab);
        let index = children.len() - 1;
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
    fn tabs_in_borders_count_and_leave() {
        let bordered = SPLIT.replace(
            r#""borders":[]"#,
            r#""borders":[{"type":"border","location":"left","selected":1,"children":[{"type":"tab","id":"b1"},{"type":"tab","id":"b2"}]}]"#,
        );
        let mut layout = Layout::parse(&bordered).unwrap();
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4", "b1", "b2"]);
        layout.reconcile(&tabs(&["t1", "t2", "t3", "t4", "b2"]));
        assert_eq!(layout.tab_order(), ["t1", "t2", "t3", "t4", "b2"]);
        assert_eq!(model(&layout)["borders"][0]["selected"], 0);
    }
}
