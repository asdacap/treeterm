//! Review's file list as a tree (`DiffFileTree.tsx`): directories first, single-child
//! directory chains collapsed (`src/renderer`), and the filters the list and the diff share.

use std::cmp::Ordering;

use treeterm_core::files::favourites::locale_compare;
use treeterm_core::git::FileChangeStats;

/// One row of the tree. Files refer to their index in the list the tree was built from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeNode {
    Dir {
        /// What the row shows: one directory, or a collapsed chain (`src/renderer`).
        name: String,
        /// The full path of the deepest directory of the chain.
        path: String,
        children: Vec<TreeNode>,
        /// Line counts of every text file below (binary files count nothing).
        additions: u64,
        deletions: u64,
    },
    File {
        name: String,
        index: usize,
    },
}

impl TreeNode {
    fn name(&self) -> &str {
        match self {
            TreeNode::Dir { name, .. } | TreeNode::File { name, .. } => name,
        }
    }
}

/// `buildTree`: `paths[i]` with `stats[i]`.
pub fn build_tree(files: &[(&str, FileChangeStats)]) -> Vec<TreeNode> {
    let mut root: Vec<TreeNode> = Vec::new();
    for (index, (path, _)) in files.iter().enumerate() {
        let parts: Vec<&str> = path.split('/').collect();
        let mut level = &mut root;
        for (depth, part) in parts.iter().enumerate() {
            if depth == parts.len() - 1 {
                level.push(TreeNode::File { name: (*part).to_owned(), index });
                break;
            }
            let at = level.iter().position(|node| matches!(node, TreeNode::Dir { name, .. } if name == part));
            let at = match at {
                Some(at) => at,
                None => {
                    level.push(TreeNode::Dir {
                        name: (*part).to_owned(),
                        path: parts[..=depth].join("/"),
                        children: Vec::new(),
                        additions: 0,
                        deletions: 0,
                    });
                    level.len() - 1
                }
            };
            let TreeNode::Dir { children, .. } = &mut level[at] else {
                unreachable!("found as a directory")
            };
            level = children;
        }
    }
    let mut nodes: Vec<TreeNode> = root.into_iter().map(collapse).collect();
    sort(&mut nodes);
    for node in &mut nodes {
        count(node, files);
    }
    nodes
}

/// `src/` holding only `renderer/` becomes `src/renderer`.
fn collapse(node: TreeNode) -> TreeNode {
    match node {
        TreeNode::Dir { name, path, children, additions, deletions } => {
            let mut children: Vec<TreeNode> = children.into_iter().map(collapse).collect();
            if children.len() == 1 && matches!(children[0], TreeNode::Dir { .. }) {
                let Some(TreeNode::Dir { name: child, path, children, .. }) = children.pop() else {
                    unreachable!("checked above")
                };
                return TreeNode::Dir { name: format!("{name}/{child}"), path, children, additions, deletions };
            }
            TreeNode::Dir { name, path, children, additions, deletions }
        }
        file @ TreeNode::File { .. } => file,
    }
}

/// Directories first, then files, each by name (`localeCompare`).
fn sort(nodes: &mut [TreeNode]) {
    nodes.sort_by(|a, b| match (a, b) {
        (TreeNode::Dir { .. }, TreeNode::File { .. }) => Ordering::Less,
        (TreeNode::File { .. }, TreeNode::Dir { .. }) => Ordering::Greater,
        _ => locale_compare(a.name(), b.name()),
    });
    for node in nodes {
        if let TreeNode::Dir { children, .. } = node {
            sort(children);
        }
    }
}

/// `getNodeStats`: a directory's line counts are its text files'.
fn count(node: &mut TreeNode, files: &[(&str, FileChangeStats)]) -> (u64, u64) {
    match node {
        TreeNode::File { index, .. } => match files[*index].1 {
            FileChangeStats::Text { additions, deletions } => (additions, deletions),
            FileChangeStats::Binary { .. } | FileChangeStats::Unknown => (0, 0),
        },
        TreeNode::Dir { children, additions, deletions, .. } => {
            let (mut added, mut deleted) = (0, 0);
            for child in children {
                let (a, d) = count(child, files);
                added += a;
                deleted += d;
            }
            (*additions, *deletions) = (added, deleted);
            (added, deleted)
        }
    }
}

/// `getSortedFilePaths`: the files' indices in tree order (the diff lists them so).
pub fn tree_order(nodes: &[TreeNode]) -> Vec<usize> {
    let mut order = Vec::new();
    fn walk(nodes: &[TreeNode], order: &mut Vec<usize>) {
        for node in nodes {
            match node {
                TreeNode::File { index, .. } => order.push(*index),
                TreeNode::Dir { children, .. } => walk(children, order),
            }
        }
    }
    walk(nodes, &mut order);
    order
}

/// `filterFilesByDir` then `filterFilesBySearch`: under `dir` (or the path itself), and
/// containing `query`, case-insensitively.
pub fn file_matches(path: &str, dir: Option<&str>, query: &str) -> bool {
    let in_dir = match dir {
        Some(dir) => path == dir || path.strip_prefix(dir).is_some_and(|rest| rest.starts_with('/')),
        None => true,
    };
    let needle = query.trim().to_lowercase();
    in_dir && (needle.is_empty() || path.to_lowercase().contains(&needle))
}

/// One file's changed lines in a unified patch, as runs of 1-based line numbers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedLines {
    /// Removed lines, numbered in the original file.
    pub deleted: Vec<std::ops::RangeInclusive<u32>>,
    /// Added lines, numbered in the modified file.
    pub added: Vec<std::ops::RangeInclusive<u32>>,
    /// The last line the patch shows on each side (the whole file, with full context).
    pub original_lines: u32,
    pub modified_lines: u32,
}

/// The changed lines of every file in `patch` (`git diff` output), by path (the new path;
/// the old one for a deleted file). The diff tints these rows.
pub fn changed_lines(patch: &str) -> std::collections::HashMap<String, ChangedLines> {
    fn push(runs: &mut Vec<std::ops::RangeInclusive<u32>>, line: u32) {
        match runs.last_mut() {
            Some(run) if *run.end() + 1 == line => *run = *run.start()..=line,
            Some(_) | None => runs.push(line..=line),
        }
    }
    let mut files: std::collections::HashMap<String, ChangedLines> = std::collections::HashMap::new();
    let (mut old_path, mut path): (Option<String>, Option<String>) = (None, None);
    let (mut old, mut new, mut in_hunk) = (0u32, 0u32, false);
    for line in patch.lines() {
        if line.starts_with("diff --git ") {
            (old_path, path, in_hunk) = (None, None, false);
        } else if !in_hunk && let Some(rest) = line.strip_prefix("--- ") {
            old_path = rest.strip_prefix("a/").map(str::to_owned);
        } else if !in_hunk && let Some(rest) = line.strip_prefix("+++ ") {
            path = rest.strip_prefix("b/").map(str::to_owned).or_else(|| old_path.clone());
        } else if let Some(rest) = line.strip_prefix("@@ -") {
            // `@@ -a[,b] +c[,d] @@`
            let mut parts = rest.split(' ');
            let start = |part: Option<&str>| part.and_then(|p| p.trim_start_matches('+').split(',').next()?.parse::<u32>().ok());
            (old, new) = (start(parts.next()).unwrap_or(0), start(parts.next()).unwrap_or(0));
            in_hunk = path.is_some();
        } else if in_hunk && let Some(file) = path.as_ref() {
            let entry = files.entry(file.clone()).or_default();
            match line.as_bytes().first() {
                Some(b'-') => {
                    push(&mut entry.deleted, old);
                    entry.original_lines = old;
                    old += 1;
                }
                Some(b'+') => {
                    push(&mut entry.added, new);
                    entry.modified_lines = new;
                    new += 1;
                }
                Some(b'\\') => {} // "\ No newline at end of file"
                Some(_) | None => {
                    (entry.original_lines, entry.modified_lines) = (old, new);
                    old += 1;
                    new += 1;
                }
            }
        }
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changed_lines_are_runs_per_side() {
        let patch = "diff --git a/a.txt b/a.txt\nindex 1..2 100644\n--- a/a.txt\n+++ b/a.txt\n@@ -1,4 +1,5 @@\n one\n-two\n-three\n+TWO\n+THREE\n+extra\n four\n\\ No newline at end of file\n@@ -10,2 +11,2 @@\n ten\n-eleven\n+ELEVEN\ndiff --git a/gone.txt b/gone.txt\ndeleted file mode 100644\n--- a/gone.txt\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-x\n-y\n";
        let lines = changed_lines(patch);
        assert_eq!(lines["a.txt"].deleted, vec![2..=3, 11..=11]);
        assert_eq!(lines["a.txt"].added, vec![2..=4, 12..=12]);
        assert_eq!((lines["a.txt"].original_lines, lines["a.txt"].modified_lines), (11, 12));
        assert_eq!(lines["gone.txt"], ChangedLines { deleted: std::iter::once(1..=2).collect(), added: vec![], original_lines: 2, modified_lines: 0 });
    }

    fn text(additions: u64, deletions: u64) -> FileChangeStats {
        FileChangeStats::Text { additions, deletions }
    }

    fn names(nodes: &[TreeNode]) -> Vec<String> {
        nodes.iter().map(|n| n.name().to_owned()).collect()
    }

    #[test]
    fn directories_come_first_and_single_child_chains_collapse() {
        let files = [
            ("z.txt", text(1, 0)),
            ("src/renderer/b.ts", text(2, 1)),
            ("src/renderer/A.ts", text(3, 0)),
            ("docs/x.md", FileChangeStats::Binary { byte_change: 4 }),
            ("docs/y/z.md", text(0, 5)),
        ];
        let tree = build_tree(&files);
        assert_eq!(names(&tree), ["docs", "src/renderer", "z.txt"]);
        let TreeNode::Dir { path, children, additions, deletions, .. } = &tree[1] else { panic!() };
        assert_eq!(path, "src/renderer");
        assert_eq!(names(children), ["A.ts", "b.ts"]);
        assert_eq!((*additions, *deletions), (5, 1));
        // `docs` has a file beside `y`, so it does not collapse; binary files count nothing.
        let TreeNode::Dir { children, additions, deletions, .. } = &tree[0] else { panic!() };
        assert_eq!(names(children), ["y", "x.md"]);
        assert_eq!((*additions, *deletions), (0, 5));
        assert_eq!(tree_order(&tree), [4, 3, 2, 1, 0]);
    }

    #[test]
    fn a_top_level_chain_collapses_to_its_deepest_directory() {
        let tree = build_tree(&[("a/b/c/d.rs", text(1, 1))]);
        let TreeNode::Dir { name, path, children, .. } = &tree[0] else { panic!() };
        assert_eq!((name.as_str(), path.as_str()), ("a/b/c", "a/b/c"));
        assert_eq!(children, &[TreeNode::File { name: "d.rs".into(), index: 0 }]);
    }

    #[test]
    fn filters_match_a_directory_and_a_case_insensitive_substring() {
        assert!(file_matches("src/a.rs", Some("src"), ""));
        assert!(file_matches("src", Some("src"), ""));
        assert!(!file_matches("srcs/a.rs", Some("src"), ""));
        assert!(file_matches("src/Main.rs", None, " main "));
        assert!(!file_matches("src/a.rs", None, "main"));
        assert!(!file_matches("lib/main.rs", Some("src"), "main"));
    }
}
