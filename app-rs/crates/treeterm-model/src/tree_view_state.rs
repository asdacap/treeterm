//! A window's view of its session's workspace tree (`SessionPanel`'s `expanded`): which
//! workspaces have their children hidden.

use std::collections::HashSet;

/// A workspace shown under another: `child` is loaded and its `parentId` is `parent`, a
/// workspace of the same session (`SessionPanel`'s `getChildren`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChildEdge {
    pub child: String,
    pub parent: String,
}

/// Every workspace shows its children unless the user collapsed it, and a collapsed
/// workspace opens again when it gains a child ("Expand any workspace that gains
/// children"), whichever window or dialog added it.
pub struct TreeViewState {
    collapsed: HashSet<String>,
    /// The session's parent links when last observed, to tell which children are new.
    edges: HashSet<ChildEdge>,
}

impl TreeViewState {
    /// Nothing collapsed; `edges` are the session's parent links now.
    pub fn new(edges: HashSet<ChildEdge>) -> Self {
        Self {
            collapsed: HashSet::new(),
            edges,
        }
    }

    pub fn collapsed(&self) -> &HashSet<String> {
        &self.collapsed
    }

    pub fn is_collapsed(&self, id: &str) -> bool {
        self.collapsed.contains(id)
    }

    /// The row's chevron: hide or show `id`'s children.
    pub fn toggle(&mut self, id: &str) {
        if !self.collapsed.remove(id) {
            self.collapsed.insert(id.to_owned());
        }
    }

    /// Show `id`'s children (a workspace dropped onto it).
    pub fn expand(&mut self, id: &str) {
        self.collapsed.remove(id);
    }

    /// The session's parent links now: a parent with a child it did not have before shows
    /// its children again.
    pub fn observe(&mut self, edges: HashSet<ChildEdge>) {
        for edge in edges.difference(&self.edges) {
            self.collapsed.remove(&edge.parent);
        }
        self.edges = edges;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edges(pairs: &[(&str, &str)]) -> HashSet<ChildEdge> {
        pairs
            .iter()
            .map(|(child, parent)| ChildEdge {
                child: (*child).to_owned(),
                parent: (*parent).to_owned(),
            })
            .collect()
    }

    #[test]
    fn toggle_collapses_and_expands() {
        let mut view = TreeViewState::new(edges(&[("a", "root")]));
        assert!(!view.is_collapsed("root"));
        view.toggle("root");
        assert!(view.is_collapsed("root"));
        assert_eq!(view.collapsed(), &HashSet::from(["root".to_owned()]));
        view.toggle("root");
        assert!(!view.is_collapsed("root"));
    }

    #[test]
    fn expand_shows_a_collapsed_workspace_and_leaves_an_open_one_open() {
        let mut view = TreeViewState::new(edges(&[]));
        view.toggle("root");
        view.expand("root");
        assert!(!view.is_collapsed("root"));
        view.expand("root");
        assert!(!view.is_collapsed("root"));
    }

    #[test]
    fn a_new_child_expands_only_its_parent() {
        let mut view = TreeViewState::new(edges(&[("a", "root"), ("b", "other")]));
        view.toggle("root");
        view.toggle("other");
        view.observe(edges(&[("a", "root"), ("b", "other"), ("c", "root")]));
        assert!(!view.is_collapsed("root"));
        assert!(view.is_collapsed("other"));
    }

    #[test]
    fn a_known_child_does_not_undo_a_later_collapse() {
        let mut view = TreeViewState::new(edges(&[("a", "root")]));
        view.observe(edges(&[("a", "root"), ("b", "root")]));
        view.toggle("root");
        view.observe(edges(&[("a", "root"), ("b", "root")]));
        assert!(view.is_collapsed("root"));
        // A child leaving is no reason to open it either.
        view.observe(edges(&[("a", "root")]));
        assert!(view.is_collapsed("root"));
    }

    #[test]
    fn a_child_moved_to_a_new_parent_expands_the_new_parent() {
        let mut view = TreeViewState::new(edges(&[("a", "old"), ("b", "old"), ("c", "new")]));
        view.toggle("old");
        view.toggle("new");
        view.observe(edges(&[("a", "new"), ("b", "old"), ("c", "new")]));
        assert!(!view.is_collapsed("new"));
        assert!(view.is_collapsed("old"));
    }

    #[test]
    fn a_child_that_comes_back_expands_its_parent_again() {
        // Gone (removed, or reloading) and back: Electron sees the parent gain a child.
        let mut view = TreeViewState::new(edges(&[("a", "root")]));
        view.toggle("root");
        view.observe(edges(&[]));
        assert!(view.is_collapsed("root"));
        view.observe(edges(&[("a", "root")]));
        assert!(!view.is_collapsed("root"));
    }

    #[test]
    fn a_leaf_gaining_its_first_child_stays_expanded() {
        let mut view = TreeViewState::new(edges(&[]));
        view.observe(edges(&[("a", "root")]));
        assert!(!view.is_collapsed("root"));
        assert!(view.collapsed().is_empty());
    }
}
