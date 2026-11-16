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
    /// The links the session had before it was rebuilt, not seen again since: its
    /// workspaces load again one by one, and a child loading under the parent it had is not
    /// new.
    reloading: HashSet<ChildEdge>,
}

impl TreeViewState {
    /// Nothing collapsed; `edges` are the session's parent links now.
    pub fn new(edges: HashSet<ChildEdge>) -> Self {
        Self {
            collapsed: HashSet::new(),
            edges,
            reloading: HashSet::new(),
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
            if !self.reloading.remove(edge) {
                self.collapsed.remove(&edge.parent);
            }
        }
        // A child seen under another parent has moved: the link it had is gone.
        self.reloading
            .retain(|old| !edges.iter().any(|edge| edge.child == old.child));
        self.edges = edges;
    }

    /// The session is rebuilt from the daemon (the connection's next generation): what is
    /// collapsed stays collapsed while its workspaces load again. A child that was not
    /// there before the rebuild still opens its parent.
    pub fn reload(&mut self) {
        self.reloading.extend(self.edges.drain());
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
    fn a_rebuilt_session_keeps_its_collapsed_rows_collapsed_while_it_loads_again() {
        let mut view = TreeViewState::new(edges(&[("a", "root"), ("b", "root"), ("c", "other")]));
        view.toggle("root");
        view.reload();
        // The new session's snapshot (placeholders only), then its workspaces one by one.
        view.observe(edges(&[]));
        view.observe(edges(&[("a", "root")]));
        view.observe(edges(&[("a", "root"), ("c", "other")]));
        view.observe(edges(&[("a", "root"), ("b", "root"), ("c", "other")]));
        assert!(view.is_collapsed("root"));
        assert_eq!(view.collapsed(), &HashSet::from(["root".to_owned()]));
    }

    #[test]
    fn a_child_new_since_the_rebuild_opens_its_parent() {
        let mut view = TreeViewState::new(edges(&[("a", "root")]));
        view.toggle("root");
        view.reload();
        view.observe(edges(&[("a", "root"), ("new", "root")]));
        assert!(!view.is_collapsed("root"));
    }

    #[test]
    fn once_loaded_again_a_child_that_comes_back_opens_its_parent() {
        let mut view = TreeViewState::new(edges(&[("a", "root")]));
        view.toggle("root");
        view.reload();
        view.observe(edges(&[("a", "root")]));
        view.observe(edges(&[]));
        assert!(view.is_collapsed("root"));
        view.observe(edges(&[("a", "root")]));
        assert!(!view.is_collapsed("root"));
    }

    #[test]
    fn a_child_moved_while_rebuilding_opens_its_new_parent_and_later_its_old_one() {
        let mut view = TreeViewState::new(edges(&[("a", "old")]));
        view.toggle("old");
        view.toggle("new");
        view.reload();
        view.observe(edges(&[("a", "new")]));
        assert!(!view.is_collapsed("new"));
        assert!(view.is_collapsed("old"));
        // Its link to the old parent is forgotten: moving back is gaining a child.
        view.observe(edges(&[("a", "old")]));
        assert!(!view.is_collapsed("old"));
    }

    #[test]
    fn a_leaf_gaining_its_first_child_stays_expanded() {
        let mut view = TreeViewState::new(edges(&[]));
        view.observe(edges(&[("a", "root")]));
        assert!(!view.is_collapsed("root"));
        assert!(view.collapsed().is_empty());
    }
}
