//! The Electron app's tab layout: a flex-layout JSON model saved in a workspace's
//! `metadata.layoutModel`. The native app reads it for tab order; Electron reconciles the
//! model against the workspace's tabs whenever it renders, so tabs added or closed here
//! show up there without the native app rewriting the layout.

use serde_json::Value;

/// Tab ids in the order the layout shows them: tabsets depth-first, left to right.
/// `None` when the model is missing or not valid JSON (Electron then builds a default).
pub fn tab_order(layout_model: &str) -> Option<Vec<String>> {
    let model: Value = serde_json::from_str(layout_model).ok()?;
    let mut ids = Vec::new();
    collect(&model["layout"], &mut ids);
    if let Some(borders) = model["borders"].as_array() {
        for border in borders {
            collect(border, &mut ids);
        }
    }
    Some(ids)
}

fn collect(node: &Value, ids: &mut Vec<String>) {
    if node["type"] == "tab"
        && let Some(id) = node["id"].as_str()
    {
        ids.push(id.to_owned());
    }
    if let Some(children) = node["children"].as_array() {
        for child in children {
            collect(child, ids);
        }
    }
}

/// `tabs` reordered by the layout; tabs the layout does not know keep their relative order
/// at the end.
pub fn order_tabs<'a>(tabs: impl IntoIterator<Item = &'a String>, layout_model: Option<&str>) -> Vec<String> {
    let tabs: Vec<String> = tabs.into_iter().cloned().collect();
    let Some(order) = layout_model.and_then(tab_order) else {
        return tabs;
    };
    let mut ordered: Vec<String> = order.into_iter().filter(|id| tabs.contains(id)).collect();
    for tab in tabs {
        if !ordered.contains(&tab) {
            ordered.push(tab);
        }
    }
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    const SPLIT: &str = r#"{
        "global": {}, "borders": [],
        "layout": {"type": "row", "children": [
            {"type": "tabset", "children": [{"type": "tab", "id": "t2"}, {"type": "tab", "id": "t1"}]},
            {"type": "row", "children": [
                {"type": "tabset", "children": [{"type": "tab", "id": "t9"}]}
            ]}
        ]}
    }"#;

    #[test]
    fn reads_tabs_depth_first() {
        assert_eq!(tab_order(SPLIT).unwrap(), vec!["t2", "t1", "t9"]);
        assert_eq!(tab_order("not json"), None);
    }

    #[test]
    fn orders_known_tabs_and_appends_the_rest() {
        let tabs = ["t1".to_owned(), "t2".to_owned(), "new".to_owned()];
        // t9 is in the layout but no longer a tab.
        assert_eq!(order_tabs(&tabs, Some(SPLIT)), vec!["t2", "t1", "new"]);
        assert_eq!(order_tabs(&tabs, None), vec!["t1", "t2", "new"]);
    }
}
