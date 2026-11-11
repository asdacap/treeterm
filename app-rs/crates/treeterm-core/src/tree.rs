//! Workspace tree ordering (port of the root/children logic in `SessionPanel.tsx`).
//!
//! Roots are: workspaces not loaded yet (always top-level), workspaces without a parent,
//! and orphans whose parent is not in the session. Siblings are ordered by
//! `metadata.sortOrder`; unloaded workspaces sort last. Ties keep session order.

use std::collections::HashSet;

use crate::wire::workspace_file::{Parent, WorkspaceFile};

/// A workspace as far as the tree is concerned.
#[derive(Debug, Clone, Copy)]
pub enum TreeNode<'a> {
    /// Loading or failed: no body to read a parent or order from.
    Unloaded { id: &'a str },
    Loaded { id: &'a str, file: &'a WorkspaceFile },
}

impl<'a> TreeNode<'a> {
    fn id(&self) -> &'a str {
        match self {
            TreeNode::Unloaded { id } | TreeNode::Loaded { id, .. } => id,
        }
    }

    fn sort_key(&self) -> SortKey {
        match self {
            TreeNode::Unloaded { .. } => SortKey::Last,
            TreeNode::Loaded { file, .. } => SortKey::Order(sort_order(file)),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SortKey {
    Order(i64),
    Last,
}

/// `parseInt(metadata.sortOrder || '0')`: leading integer, `0` when absent or unparsable.
pub fn sort_order(file: &WorkspaceFile) -> i64 {
    let raw = file.metadata.get("sortOrder").map_or("", String::as_str).trim_start();
    let (sign, digits) = match raw.strip_prefix('-') {
        Some(rest) => (-1, rest),
        None => (1, raw.strip_prefix('+').unwrap_or(raw)),
    };
    let end = digits.find(|c: char| !c.is_ascii_digit()).unwrap_or(digits.len());
    digits[..end].parse::<i64>().map_or(0, |n| sign * n)
}

/// One visible row, in display (pre-order) order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub id: String,
    pub depth: usize,
    pub has_children: bool,
}

/// Flatten the tree for display. `nodes` must be in session (ref list) order.
/// `collapsed` holds ids whose children are hidden.
pub fn flatten(nodes: &[TreeNode<'_>], collapsed: &HashSet<String>) -> Vec<TreeRow> {
    let present: HashSet<&str> = nodes.iter().map(TreeNode::id).collect();
    let mut roots: Vec<&TreeNode<'_>> = nodes
        .iter()
        .filter(|node| match node {
            TreeNode::Unloaded { .. } => true,
            TreeNode::Loaded { file, .. } => match file.parent() {
                Parent::Root => true,
                Parent::Child(parent) => !present.contains(parent),
            },
        })
        .collect();
    roots.sort_by_key(|node| node.sort_key());

    let mut rows = Vec::with_capacity(nodes.len());
    for root in roots {
        push_subtree(root, 0, nodes, collapsed, &mut rows);
    }
    rows
}

fn children<'n, 'a>(id: &str, nodes: &'n [TreeNode<'a>]) -> Vec<&'n TreeNode<'a>> {
    let mut kids: Vec<&TreeNode<'a>> = nodes
        .iter()
        .filter(|node| match node {
            TreeNode::Unloaded { .. } => false,
            TreeNode::Loaded { file, .. } => file.parent() == Parent::Child(id),
        })
        .collect();
    kids.sort_by_key(|node| node.sort_key());
    kids
}

fn push_subtree(
    node: &TreeNode<'_>,
    depth: usize,
    nodes: &[TreeNode<'_>],
    collapsed: &HashSet<String>,
    rows: &mut Vec<TreeRow>,
) {
    let id = node.id();
    let kids = children(id, nodes);
    rows.push(TreeRow {
        id: id.to_owned(),
        depth,
        has_children: !kids.is_empty(),
    });
    if collapsed.contains(id) {
        return;
    }
    for kid in kids {
        push_subtree(kid, depth + 1, nodes, collapsed, rows);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wire::workspace_file::parse_workspace_file;

    fn file(parent: Option<&str>, sort_order: Option<&str>) -> WorkspaceFile {
        let mut json = serde_json::json!({
            "name": "w", "status": "active", "isGitRepo": true, "isWorktree": false,
            "appStates": {}, "metadata": {}, "createdAt": 1, "lastActivity": 1, "parentHash": ""
        });
        if let Some(parent) = parent {
            json["parentId"] = parent.into();
        }
        if let Some(order) = sort_order {
            json["metadata"]["sortOrder"] = order.into();
        }
        parse_workspace_file(json.to_string().as_bytes()).unwrap()
    }

    fn ids(rows: &[TreeRow]) -> Vec<(&str, usize)> {
        rows.iter().map(|r| (r.id.as_str(), r.depth)).collect()
    }

    #[test]
    fn nests_children_under_parents_in_sort_order() {
        let root = file(None, Some("0"));
        let b = file(Some("root"), Some("2"));
        let a = file(Some("root"), Some("1"));
        let grand = file(Some("a"), None);
        let nodes = [
            TreeNode::Loaded { id: "root", file: &root },
            TreeNode::Loaded { id: "b", file: &b },
            TreeNode::Loaded { id: "a", file: &a },
            TreeNode::Loaded { id: "grand", file: &grand },
        ];
        let rows = flatten(&nodes, &HashSet::new());
        assert_eq!(ids(&rows), vec![("root", 0), ("a", 1), ("grand", 2), ("b", 1)]);
        assert!(rows[0].has_children && rows[1].has_children && !rows[3].has_children);
    }

    #[test]
    fn unloaded_and_orphans_are_roots_with_unloaded_last() {
        let orphan = file(Some("gone"), Some("5"));
        let top = file(None, Some("1"));
        let nodes = [
            TreeNode::Unloaded { id: "loading" },
            TreeNode::Loaded { id: "orphan", file: &orphan },
            TreeNode::Loaded { id: "top", file: &top },
        ];
        let rows = flatten(&nodes, &HashSet::new());
        assert_eq!(ids(&rows), vec![("top", 0), ("orphan", 0), ("loading", 0)]);
    }

    #[test]
    fn ties_keep_session_order_and_collapsed_hides_children() {
        let p = file(None, None);
        let x = file(Some("p"), None);
        let y = file(Some("p"), None);
        let nodes = [
            TreeNode::Loaded { id: "p", file: &p },
            TreeNode::Loaded { id: "x", file: &x },
            TreeNode::Loaded { id: "y", file: &y },
        ];
        assert_eq!(ids(&flatten(&nodes, &HashSet::new())), vec![("p", 0), ("x", 1), ("y", 1)]);
        let collapsed = HashSet::from(["p".to_owned()]);
        let rows = flatten(&nodes, &collapsed);
        assert_eq!(ids(&rows), vec![("p", 0)]);
        assert!(rows[0].has_children);
    }

    #[test]
    fn sort_order_parses_like_parse_int() {
        for (raw, expected) in [("12", 12), (" 7x", 7), ("-3", -3), ("abc", 0), ("", 0)] {
            assert_eq!(sort_order(&file(None, Some(raw))), expected, "{raw:?}");
        }
        assert_eq!(sort_order(&file(None, None)), 0);
    }
}
