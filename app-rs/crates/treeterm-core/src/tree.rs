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

/// Where a dragged workspace lands relative to the one it is dropped on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropPosition {
    Before,
    After,
    /// Becomes the target's last child.
    Onto,
}

/// A workspace's place in the tree as `moveWorkspace` decides it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub id: String,
    pub parent_id: Option<String>,
    pub sort_order: i64,
}

/// `moveWorkspace`: the new parent and sort orders after dropping `drag` on `target` at
/// `position`, for every workspace whose placement changes. `None` when the move is a no-op
/// or would make a workspace its own ancestor. `workspaces` is `(id, parent, sort order)`.
pub fn plan_move(
    workspaces: &[(String, Option<String>, i64)],
    drag: &str,
    target: &str,
    position: DropPosition,
) -> Option<Vec<Placement>> {
    if drag == target {
        return None;
    }
    let parent_of = |id: &str| workspaces.iter().find(|(w, _, _)| w == id).map(|(_, parent, _)| parent.clone());
    let old_parent = parent_of(drag)?;
    let target_parent = parent_of(target)?;
    let new_parent = match position {
        DropPosition::Onto => Some(target.to_owned()),
        DropPosition::Before | DropPosition::After => target_parent,
    };
    // Cycle check: the new parent must not be the dragged workspace or below it.
    let mut current = new_parent.clone();
    let mut steps = 0;
    while let Some(id) = current {
        if id == drag || steps > workspaces.len() {
            return None;
        }
        current = parent_of(&id).flatten();
        steps += 1;
    }
    let siblings = |parent: &Option<String>| {
        let mut list: Vec<&(String, Option<String>, i64)> =
            workspaces.iter().filter(|(id, p, _)| p == parent && id != drag).collect();
        list.sort_by_key(|(_, _, order)| *order);
        list.into_iter().map(|(id, _, _)| id.clone()).collect::<Vec<String>>()
    };
    let mut ordered = siblings(&new_parent);
    let at = match position {
        DropPosition::Onto => ordered.len(),
        DropPosition::Before => ordered.iter().position(|id| id == target)?,
        DropPosition::After => ordered.iter().position(|id| id == target)? + 1,
    };
    ordered.insert(at, drag.to_owned());
    let mut out: Vec<Placement> = ordered
        .into_iter()
        .enumerate()
        .map(|(i, id)| Placement { id, parent_id: new_parent.clone(), sort_order: i as i64 })
        .collect();
    // The siblings left behind close the gap (`reindexSiblings`).
    if old_parent != new_parent {
        out.extend(siblings(&old_parent).into_iter().enumerate().map(|(i, id)| Placement {
            id,
            parent_id: old_parent.clone(),
            sort_order: i as i64,
        }));
    }
    // Only what changes.
    out.retain(|p| workspaces.iter().any(|(id, parent, order)| *id == p.id && (parent != &p.parent_id || *order != p.sort_order)));
    Some(out)
}

#[cfg(test)]
mod move_tests {
    use super::*;

    fn ws(id: &str, parent: Option<&str>, order: i64) -> (String, Option<String>, i64) {
        (id.to_owned(), parent.map(str::to_owned), order)
    }

    fn sample() -> Vec<(String, Option<String>, i64)> {
        vec![ws("a", None, 0), ws("b", None, 1), ws("c", None, 2), ws("a1", Some("a"), 0), ws("a2", Some("a"), 1)]
    }

    #[test]
    fn reorders_among_siblings() {
        let plan = plan_move(&sample(), "c", "a", DropPosition::Before).unwrap();
        let orders: Vec<(&str, i64)> = plan.iter().map(|p| (p.id.as_str(), p.sort_order)).collect();
        assert_eq!(orders, vec![("c", 0), ("a", 1), ("b", 2)]);
    }

    #[test]
    fn dropping_onto_reparents_as_the_last_child_and_closes_the_gap() {
        let plan = plan_move(&sample(), "b", "a", DropPosition::Onto).unwrap();
        assert!(plan.contains(&Placement { id: "b".into(), parent_id: Some("a".into()), sort_order: 2 }));
        assert!(plan.contains(&Placement { id: "c".into(), parent_id: None, sort_order: 1 }));
        assert_eq!(plan.len(), 2);
    }

    #[test]
    fn moving_across_parents_places_after_the_target() {
        let plan = plan_move(&sample(), "c", "a1", DropPosition::After).unwrap();
        assert!(plan.contains(&Placement { id: "c".into(), parent_id: Some("a".into()), sort_order: 1 }));
        assert!(plan.contains(&Placement { id: "a2".into(), parent_id: Some("a".into()), sort_order: 2 }));
    }

    #[test]
    fn a_workspace_cannot_move_under_itself() {
        assert_eq!(plan_move(&sample(), "a", "a1", DropPosition::Onto), None);
        assert_eq!(plan_move(&sample(), "a", "a2", DropPosition::After), None);
        assert_eq!(plan_move(&sample(), "a", "a", DropPosition::Before), None);
    }
}
