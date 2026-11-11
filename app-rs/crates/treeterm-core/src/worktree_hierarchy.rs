//! Worktree hierarchy detection (`lib/worktreeHierarchy.ts`): given candidate worktrees and
//! a root anchor (the workspace the user right-clicked), infer a parent/child forest from git
//! ancestry. A worktree's parent is its nearest proper ancestor among the candidates (or the
//! root anchor when none is). Branches with identical tips become siblings rather than
//! nesting, which keeps the result acyclic.

use std::collections::HashMap;
use std::future::Future;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeRef {
    pub path: String,
    pub branch: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HierarchyNode {
    pub path: String,
    pub branch: String,
    /// The parent worktree's path, or `None` to attach directly under the root anchor.
    pub parent_path: Option<String>,
}

/// `is_ancestor(a, b)`: is branch `a` an ancestor of branch `b` (`git merge-base
/// --is-ancestor`)? Each pair is asked at most once.
pub async fn detect_worktree_hierarchy<F, Fut, E>(
    candidates: &[WorktreeRef],
    root: &WorktreeRef,
    is_ancestor: F,
) -> Result<Vec<HierarchyNode>, E>
where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<bool, E>>,
{
    let mut cache: HashMap<(String, String), bool> = HashMap::new();
    // Universe = root anchor + candidates. Git forbids one branch in two worktrees, so
    // paths identify members.
    let universe: Vec<&WorktreeRef> = std::iter::once(root).chain(candidates).collect();
    // m is a *proper* ancestor of w: an ancestor, while w is not one of m (equal tips would be
    // mutual ancestors and make cycles).
    let mut proper = HashMap::new();
    for m in &universe {
        for w in &universe {
            if m.path == w.path {
                continue;
            }
            let mw = ask(&mut cache, &is_ancestor, &m.branch, &w.branch).await?;
            let wm = ask(&mut cache, &is_ancestor, &w.branch, &m.branch).await?;
            proper.insert((m.path.clone(), w.path.clone()), mw && !wm);
        }
    }
    let is_proper = |m: &WorktreeRef, w: &WorktreeRef| proper.get(&(m.path.clone(), w.path.clone())).copied().unwrap_or(false);
    // rank(w) = how many proper ancestors w has; a proper ancestor always ranks lower, so the
    // highest-ranked proper ancestor is the nearest and the forest stays acyclic.
    let rank: HashMap<&str, usize> = universe
        .iter()
        .map(|w| (w.path.as_str(), universe.iter().filter(|m| is_proper(m, w)).count()))
        .collect();
    let mut nodes = Vec::new();
    for w in candidates {
        let ancestors: Vec<&&WorktreeRef> = universe.iter().filter(|m| is_proper(m, w)).collect();
        let Some(max_rank) = ancestors.iter().map(|a| rank.get(a.path.as_str()).copied().unwrap_or(0)).max() else {
            nodes.push(HierarchyNode { path: w.path.clone(), branch: w.branch.clone(), parent_path: None });
            continue;
        };
        let mut nearest: Vec<&&WorktreeRef> = ancestors
            .into_iter()
            .filter(|a| rank.get(a.path.as_str()).copied().unwrap_or(0) == max_rank)
            .collect();
        nearest.sort_by(|a, b| a.path.cmp(&b.path));
        // Tie-break among equally near ancestors: the root anchor, else the smallest path.
        let chosen = nearest.iter().find(|a| a.path == root.path).or(nearest.first()).map_or(root, |a| **a);
        nodes.push(HierarchyNode {
            path: w.path.clone(),
            branch: w.branch.clone(),
            parent_path: (chosen.path != root.path).then(|| chosen.path.clone()),
        });
    }
    Ok(nodes)
}

/// `is_ancestor(a, b)`, asked once per pair.
async fn ask<F, Fut, E>(cache: &mut HashMap<(String, String), bool>, is_ancestor: &F, a: &str, b: &str) -> Result<bool, E>
where
    F: Fn(String, String) -> Fut,
    Fut: Future<Output = Result<bool, E>>,
{
    if a == b {
        return Ok(true);
    }
    let key = (a.to_owned(), b.to_owned());
    if let Some(known) = cache.get(&key) {
        return Ok(*known);
    }
    let answer = is_ancestor(a.to_owned(), b.to_owned()).await?;
    cache.insert(key, answer);
    Ok(answer)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tips (equal → mutual ancestors) and proper ancestors per branch.
    struct Graph {
        tip: HashMap<&'static str, &'static str>,
        proper: HashMap<&'static str, Vec<&'static str>>,
    }

    fn graph(tip: &[(&'static str, &'static str)], proper: &[(&'static str, &[&'static str])]) -> Graph {
        Graph {
            tip: tip.iter().copied().collect(),
            proper: proper.iter().map(|(b, a)| (*b, a.to_vec())).collect(),
        }
    }

    fn detect(candidates: &[&str], g: &Graph) -> Vec<HierarchyNode> {
        let wt = |b: &str| WorktreeRef { path: format!("/wt/{b}"), branch: b.to_owned() };
        let candidates: Vec<WorktreeRef> = candidates.iter().map(|b| wt(b)).collect();
        let is_ancestor = |a: String, b: String| {
            let answer = a == b
                || matches!((g.tip.get(a.as_str()), g.tip.get(b.as_str())), (Some(x), Some(y)) if x == y)
                || g.proper.get(b.as_str()).is_some_and(|list| list.contains(&a.as_str()));
            async move { Ok::<bool, ()>(answer) }
        };
        futures::executor::block_on(detect_worktree_hierarchy(&candidates, &wt("master"), is_ancestor)).unwrap()
    }

    fn parent_of<'a>(nodes: &'a [HierarchyNode], path: &str) -> Option<&'a str> {
        nodes.iter().find(|n| n.path == path).and_then(|n| n.parent_path.as_deref())
    }

    #[test]
    fn attaches_a_direct_child_under_the_root_anchor() {
        let g = graph(&[("master", "c0"), ("feat", "c1")], &[("feat", &["master"])]);
        assert_eq!(
            detect(&["feat"], &g),
            vec![HierarchyNode { path: "/wt/feat".into(), branch: "feat".into(), parent_path: None }]
        );
    }

    #[test]
    fn nests_a_grandchild_under_its_nearest_ancestor() {
        let g = graph(&[("master", "c0"), ("feat", "c1"), ("sub", "c2")], &[("feat", &["master"]), ("sub", &["master", "feat"])]);
        let nodes = detect(&["feat", "sub"], &g);
        assert_eq!(parent_of(&nodes, "/wt/feat"), None);
        assert_eq!(parent_of(&nodes, "/wt/sub"), Some("/wt/feat"));
    }

    #[test]
    fn places_incomparable_branches_as_siblings_under_the_root() {
        let g = graph(&[("master", "c0"), ("a", "c1"), ("b", "c2")], &[("a", &["master"]), ("b", &["master"])]);
        assert!(detect(&["a", "b"], &g).iter().all(|n| n.parent_path.is_none()));
    }

    #[test]
    fn breaks_equal_tip_ties_by_attaching_both_to_the_root() {
        let g = graph(&[("master", "c0"), ("a", "c1"), ("b", "c1")], &[("a", &["master"]), ("b", &["master"])]);
        let parents: Vec<Option<String>> = detect(&["a", "b"], &g).into_iter().map(|n| n.parent_path).collect();
        assert_eq!(parents, vec![None, None]);
    }

    #[test]
    fn falls_back_to_root_for_an_unrelated_orphan_worktree() {
        let g = graph(&[("master", "c0"), ("orphan", "x9")], &[]);
        assert_eq!(detect(&["orphan"], &g)[0].parent_path, None);
    }

    #[test]
    fn chooses_the_deeper_of_two_proper_ancestors_as_the_parent() {
        let g = graph(&[("master", "c0"), ("mid", "c1"), ("leaf", "c2")], &[("mid", &["master"]), ("leaf", &["master", "mid"])]);
        let nodes = detect(&["leaf", "mid"], &g);
        assert_eq!(parent_of(&nodes, "/wt/leaf"), Some("/wt/mid"));
        assert_eq!(parent_of(&nodes, "/wt/mid"), None);
    }

    #[test]
    fn returns_an_empty_list_when_there_are_no_candidates() {
        let g = graph(&[("master", "c0")], &[]);
        assert!(detect(&[], &g).is_empty());
    }
}
