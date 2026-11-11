//! "Auto Open All Recent Worktrees" (`AutoOpenWorktreesDialog.tsx`): the root workspace's
//! worktrees that are recorded in the worktree registry (optionally all of them), placed in a
//! hierarchy by git ancestry, opened as child workspaces.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::{Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Task, Window, div, px};
use treeterm_core::worktree_hierarchy::{WorktreeRef, detect_worktree_hierarchy};
use treeterm_core::worktree_registry::WorktreeRegistryEntry;
use treeterm_model::session_store::{AutoOpenItem, SessionStore};

use crate::theme::colors;

struct Row {
    path: String,
    branch: String,
    parent_path: Option<String>,
    entry: Option<WorktreeRegistryEntry>,
    depth: usize,
}

enum Detection {
    Loading,
    Failed(String),
    Loaded(Vec<Row>),
}

pub struct AutoOpenDialog {
    session: Entity<SessionStore>,
    root_id: String,
    include_unknown: bool,
    detection: Detection,
    checked: HashSet<String>,
    _load: Option<Task<()>>,
}

/// Depth of each node from its parent chain (root children are 0).
fn depths(rows: &[(String, Option<String>)]) -> HashMap<String, usize> {
    let parents: HashMap<&str, Option<&str>> = rows.iter().map(|(p, parent)| (p.as_str(), parent.as_deref())).collect();
    rows.iter()
        .map(|(path, _)| {
            let mut depth = 0;
            let mut current = parents.get(path.as_str()).copied().flatten();
            while let Some(parent) = current {
                depth += 1;
                if depth > rows.len() {
                    break; // a cycle cannot happen, but never loop forever
                }
                current = parents.get(parent).copied().flatten();
            }
            (path.clone(), depth)
        })
        .collect()
}

impl AutoOpenDialog {
    fn new(session: Entity<SessionStore>, root_id: String, cx: &mut Context<Self>) -> Self {
        let mut dialog = Self {
            session,
            root_id,
            include_unknown: false,
            detection: Detection::Loading,
            checked: HashSet::new(),
            _load: None,
        };
        dialog.load(cx);
        dialog
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        self.detection = Detection::Loading;
        let session = self.session.read(cx);
        let Some(root) = session.workspace_store(&self.root_id) else {
            self.detection = Detection::Failed("Root workspace not found".to_owned());
            return;
        };
        let ws = root.read(cx).workspace();
        let root_ref = WorktreeRef { path: ws.path.clone(), branch: ws.file.git_branch.clone().unwrap_or_default() };
        let repo = ws.file.git_root_path.clone().unwrap_or_else(|| ws.path.clone());
        let open: HashSet<String> = session
            .workspace_ids()
            .filter_map(|id| session.workspace_store(id))
            .map(|w| w.read(cx).workspace().path.clone())
            .collect();
        let (git, registry, include_unknown) = (session.connection().git(), session.worktree_registry(), self.include_unknown);
        self._load = Some(cx.spawn(async move |this, cx| {
            let result: Result<Vec<Row>, String> = async {
                let worktrees = git.list_worktrees(&repo).await.map_err(|e| e.to_string())?;
                let entries = match registry {
                    Some(registry) => registry.list().await.map_err(|e| e.to_string())?,
                    None => Vec::new(),
                };
                let by_path: HashMap<String, WorktreeRegistryEntry> = entries.into_iter().map(|e| (e.path.clone(), e)).collect();
                let candidates: Vec<WorktreeRef> = worktrees
                    .into_iter()
                    .filter(|w| w.path != root_ref.path && !open.contains(&w.path))
                    .filter(|w| include_unknown || by_path.contains_key(&w.path))
                    .map(|w| WorktreeRef { path: w.path, branch: w.branch })
                    .collect();
                let is_ancestor = |a: String, b: String| {
                    let (git, repo) = (git.clone(), repo.clone());
                    async move { git.is_ancestor(&repo, &a, &b).await.map_err(|e| e.to_string()) }
                };
                let nodes = detect_worktree_hierarchy(&candidates, &root_ref, is_ancestor).await?;
                let depth = depths(&nodes.iter().map(|n| (n.path.clone(), n.parent_path.clone())).collect::<Vec<_>>());
                Ok(nodes
                    .into_iter()
                    .map(|n| Row {
                        depth: depth.get(&n.path).copied().unwrap_or(0),
                        entry: by_path.get(&n.path).cloned(),
                        path: n.path,
                        branch: n.branch,
                        parent_path: n.parent_path,
                    })
                    .collect())
            }
            .await;
            match this.update(cx, |dialog, cx| {
                dialog.detection = match result {
                    Ok(rows) => {
                        dialog.checked = rows.iter().map(|r| r.path.clone()).collect();
                        Detection::Loaded(rows)
                    }
                    Err(error) => Detection::Failed(format!("Failed to detect worktrees: {error}")),
                };
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: dialog closed
            }
        }));
        cx.notify();
    }

    /// Open the checked rows. Returns whether the dialog may close.
    fn confirm(&mut self, cx: &mut Context<Self>) -> bool {
        let Detection::Loaded(rows) = &self.detection else {
            return false;
        };
        let items: Vec<AutoOpenItem> = rows
            .iter()
            .filter(|r| self.checked.contains(&r.path))
            .map(|r| AutoOpenItem {
                path: r.path.clone(),
                branch: r.branch.clone(),
                parent_path: r.parent_path.clone(),
                display_name: r.entry.as_ref().and_then(|e| e.display_name.clone()),
                description: r.entry.as_ref().and_then(|e| e.description.clone()),
            })
            .collect();
        if items.is_empty() {
            return true;
        }
        let root = self.root_id.clone();
        match self.session.update(cx, |s, cx| s.auto_open_worktrees(&root, items, cx)) {
            Ok(()) => true,
            Err(error) => {
                self.detection = Detection::Failed(error);
                cx.notify();
                false
            }
        }
    }
}

impl Render for AutoOpenDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (secondary, error) = (c.text_secondary, c.error);
        let body = match &self.detection {
            Detection::Loading => div().text_sm().text_color(secondary).child("Detecting worktrees…").into_any_element(),
            Detection::Failed(message) => div().text_sm().text_color(error).child(message.clone()).into_any_element(),
            Detection::Loaded(rows) if rows.is_empty() => div()
                .text_sm()
                .text_color(secondary)
                .child("No recent worktrees to open.")
                .into_any_element(),
            Detection::Loaded(rows) => {
                let mut list = v_flex().id("auto-open-rows").max_h(px(360.)).overflow_y_scroll().gap_1();
                for (i, row) in rows.iter().enumerate() {
                    let path = row.path.clone();
                    let label = row
                        .entry
                        .as_ref()
                        .and_then(|e| e.display_name.clone())
                        .filter(|n| !n.is_empty())
                        .unwrap_or_else(|| row.branch.clone());
                    list = list.child(
                        h_flex()
                            .pl(px(16. * row.depth as f32))
                            .gap_2()
                            .child(
                                Checkbox::new(SharedString::from(format!("auto-open-{i}")))
                                    .checked(self.checked.contains(&row.path))
                                    .on_click(cx.listener(move |dialog, checked: &bool, _, cx| {
                                        if *checked {
                                            dialog.checked.insert(path.clone());
                                        } else {
                                            dialog.checked.remove(&path);
                                        }
                                        cx.notify();
                                    })),
                            )
                            .child(div().text_sm().child(label))
                            .child(div().text_xs().text_color(secondary).truncate().child(row.path.clone()))
                            .when(row.entry.is_none(), |d| d.child(div().text_xs().text_color(secondary).child("(not recent)"))),
                    );
                }
                list.into_any_element()
            }
        };
        v_flex()
            .gap_3()
            .child(
                Checkbox::new("include-unknown")
                    .label("Include worktrees that are not in the recent list")
                    .checked(self.include_unknown)
                    .on_click(cx.listener(|dialog, checked: &bool, _, cx| {
                        dialog.include_unknown = *checked;
                        dialog.load(cx);
                    })),
            )
            .child(body)
    }
}

pub fn open(session: Entity<SessionStore>, root_id: String, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| AutoOpenDialog::new(session, root_id, cx));
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let confirm = view.clone();
        dialog
            .title("Auto Open Recent Worktrees")
            .w(px(620.))
            .child(view.clone())
            .footer(h_flex().justify_end().child(Button::new("auto-open").primary().small().label("Open").on_click(
                move |_, window, cx| {
                    if confirm.update(cx, |dialog, cx| dialog.confirm(cx)) {
                        window.close_dialog(cx);
                    }
                },
            )))
    });
}

#[cfg(test)]
mod tests {
    use super::depths;

    #[test]
    fn depth_follows_the_parent_chain() {
        let rows = vec![
            ("/a".to_owned(), None),
            ("/b".to_owned(), Some("/a".to_owned())),
            ("/c".to_owned(), Some("/b".to_owned())),
        ];
        let depth = depths(&rows);
        assert_eq!((depth["/a"], depth["/b"], depth["/c"]), (0, 1, 2));
    }
}
