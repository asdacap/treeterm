//! "Auto Open All Recent Worktrees" (`AutoOpenWorktreesDialog.tsx`): the root workspace's
//! worktrees that are recorded in the worktree registry (optionally all of them), placed in a
//! hierarchy by git ancestry, opened as child workspaces. Ok waits for the whole batch: the
//! dialog closes once every worktree is open, or shows why the batch stopped.

use std::collections::{HashMap, HashSet};

use gpui_kit::component::WindowExt as _;
use gpui_kit::prelude::*;
use gpui_kit::{App, Context, Entity, SharedString, Subscription, Task, Window, div, px};
use treeterm_core::worktree_hierarchy::{WorktreeRef, detect_worktree_hierarchy};
use treeterm_core::worktree_registry::WorktreeRegistryEntry;
use treeterm_model::session_store::{AutoOpenItem, SessionStore};

use crate::theme::colors;
use crate::ui;

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

/// The Ok of the checked rows (`isProcessing`).
enum Opening {
    Idle,
    /// Waits for the batch, which runs on in the session if the dialog goes away.
    Waiting { _wait: Task<()> },
}

pub struct AutoOpenDialog {
    session: Entity<SessionStore>,
    root_id: String,
    include_unknown: bool,
    detection: Detection,
    checked: HashSet<String>,
    /// Why opening failed.
    error: Option<SharedString>,
    opening: Opening,
    /// The dialog has been closed; a later event must not close the dialog under it.
    closed: bool,
    _load: Option<Task<()>>,
    /// The dialog goes when its root does (Electron renders it only while the root is loaded).
    _root: Subscription,
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
    fn new(session: Entity<SessionStore>, root_id: String, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let root = cx.observe_in(&session, window, |dialog, session, window, cx| {
            if session.read(cx).workspace_store(&dialog.root_id).is_none() {
                dialog.close(window, cx);
            }
        });
        let mut dialog = Self {
            session,
            root_id,
            include_unknown: false,
            detection: Detection::Loading,
            checked: HashSet::new(),
            error: None,
            opening: Opening::Idle,
            closed: false,
            _load: None,
            _root: root,
        };
        dialog.load(cx);
        dialog
    }

    /// Close the dialog, once.
    fn close(&mut self, window: &mut Window, cx: &mut App) {
        if !self.closed {
            self.closed = true;
            window.close_dialog(cx);
        }
    }

    fn processing(&self) -> bool {
        match self.opening {
            Opening::Idle => false,
            Opening::Waiting { .. } => true,
        }
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

    /// Open the checked rows (`handleConfirm`): the dialog closes once they are all open, or
    /// shows why not.
    fn confirm(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Detection::Loaded(rows) = &self.detection else {
            return;
        };
        let parents: HashMap<&str, Option<&str>> =
            rows.iter().map(|r| (r.path.as_str(), r.parent_path.as_deref())).collect();
        // A node whose detected parent is unchecked hangs from its nearest checked ancestor
        // (or the root), so the opened tree stays connected (`resolveParent`).
        let resolve = |path: &str| -> Option<String> {
            let mut parent = parents.get(path).copied().flatten();
            let mut hops = 0;
            while let Some(candidate) = parent {
                hops += 1;
                if self.checked.contains(candidate) {
                    return Some(candidate.to_owned());
                }
                if hops > rows.len() {
                    return None;
                }
                parent = parents.get(candidate).copied().flatten();
            }
            None
        };
        let items: Vec<AutoOpenItem> = rows
            .iter()
            .filter(|r| self.checked.contains(&r.path))
            .map(|r| AutoOpenItem {
                path: r.path.clone(),
                branch: r.branch.clone(),
                parent_path: resolve(&r.path),
                display_name: r.entry.as_ref().and_then(|e| e.display_name.clone()),
                description: r.entry.as_ref().and_then(|e| e.description.clone()),
            })
            .collect();
        if items.is_empty() {
            return self.close(window, cx);
        }
        let root = self.root_id.clone();
        let opened = self.session.update(cx, |s, cx| s.auto_open_worktrees(&root, items, cx));
        let wait = cx.spawn_in(window, async move |dialog, cx| {
            // Canceled: the batch went away with the session (its errors are never empty).
            let outcome = opened.await.unwrap_or_else(|_| Err("Failed to open worktrees".to_owned()));
            match dialog.update_in(cx, |dialog, window, cx| {
                dialog.opening = Opening::Idle;
                match outcome {
                    Ok(()) => dialog.close(window, cx),
                    Err(error) => dialog.error = Some(error.into()),
                }
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the dialog closed
            }
        });
        self.error = None;
        self.opening = Opening::Waiting { _wait: wait };
        cx.notify();
    }

    /// The rows in tree order (each followed by its children), with their depth.
    fn ordered(rows: &[Row]) -> Vec<&Row> {
        fn walk<'a>(parent: Option<&str>, rows: &'a [Row], out: &mut Vec<&'a Row>) {
            for row in rows.iter().filter(|r| r.parent_path.as_deref() == parent) {
                if out.iter().any(|seen| seen.path == row.path) {
                    continue;
                }
                out.push(row);
                walk(Some(&row.path), rows, out);
            }
        }
        let mut out = Vec::with_capacity(rows.len());
        walk(None, rows, &mut out);
        // Rows whose parent is not listed still show, at the end.
        for row in rows {
            if !out.iter().any(|seen| seen.path == row.path) {
                out.push(row);
            }
        }
        out
    }
}

impl Render for AutoOpenDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = colors(cx);
        let (primary, secondary, border, hover, accent, tertiary, bg) =
            (c.text_primary, c.text_secondary, c.border, c.bg_hover, c.accent, c.bg_tertiary, c.bg_primary);
        let root_name = self
            .session
            .read(cx)
            .workspace_store(&self.root_id)
            .map(|w| w.read(cx).file().name.clone())
            .unwrap_or_default();
        let message = |text: &'static str| {
            div().p(px(16.)).flex().justify_center().text_size(px(ui::BODY)).text_color(secondary).child(text).into_any_element()
        };
        let rows: Vec<gpui_kit::AnyElement> = match &self.detection {
            Detection::Loading => vec![message("Detecting worktrees...")],
            Detection::Failed(_) => vec![message("No recent worktrees found")],
            Detection::Loaded(rows) if rows.is_empty() => vec![message("No recent worktrees found")],
            Detection::Loaded(rows) => {
                let ordered = Self::ordered(rows);
                let count = ordered.len();
                ordered
                    .into_iter()
                    .enumerate()
                    .map(|(i, row)| {
                        let path = row.path.clone();
                        let name = row
                            .entry
                            .as_ref()
                            .and_then(|e| e.display_name.clone())
                            .unwrap_or_else(|| row.branch.clone());
                        let checked = self.checked.contains(&row.path);
                        // `.create-child-worktree-item`, indented 20px per level; the whole
                        // row toggles its (default, 13px) checkbox.
                        div()
                            .id(SharedString::from(format!("auto-open-{i}")))
                            .flex()
                            .items_center()
                            .pl(px(12. + 20. * row.depth as f32))
                            .pr(px(12.))
                            .py(px(10.))
                            .when(i + 1 < count, |d| d.border_b_1().border_color(border))
                            .cursor_pointer()
                            .hover(move |d| d.bg(hover))
                            .child(div().mr(px(8.)).child(ui::checkbox_box(checked, false, 13.)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(ui::BODY))
                                    .line_height(px(15.5))
                                    .font_weight(gpui_kit::FontWeight::MEDIUM)
                                    .text_color(primary)
                                    .child(name),
                            )
                            .child(div().max_w(px(180.)).truncate().text_size(px(ui::SMALL)).text_color(accent).child(row.branch.clone()))
                            .when(row.entry.is_none(), |d| {
                                d.child(
                                    div()
                                        .ml(px(8.))
                                        .flex_shrink_0()
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(px(3.))
                                        .bg(tertiary)
                                        .text_size(px(10.))
                                        .line_height(px(12.))
                                        .text_color(secondary)
                                        .child("unknown"),
                                )
                            })
                            .on_click(cx.listener(move |dialog, _, _, cx| {
                                if !dialog.checked.remove(&path) {
                                    dialog.checked.insert(path.clone());
                                }
                                cx.notify();
                            }))
                            .into_any_element()
                    })
                    .collect()
            }
        };
        let error = match (&self.error, &self.detection) {
            (Some(error), _) => Some(error.clone()),
            (None, Detection::Failed(error)) => Some(SharedString::from(error.clone())),
            (None, _) => None,
        };
        let loading = matches!(self.detection, Detection::Loading);
        let count = match &self.detection {
            Detection::Loaded(rows) => rows.iter().filter(|r| self.checked.contains(&r.path)).count(),
            Detection::Loading | Detection::Failed(_) => 0,
        };
        let include = self.include_unknown;
        let processing = self.processing();
        let disabled = processing || loading || count == 0;
        let ok = if processing { "Opening... Please wait".to_owned() } else { format!("Ok ({count})") };
        ui::dialog_root(cx)
            .child(ui::dialog_header("Auto Open All Recent Worktrees", "x", primary, cx))
            .child(
                div()
                    .p(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .mb(px(16.))
                            .px(px(12.))
                            .py(px(8.))
                            .rounded(px(4.))
                            .bg(tertiary)
                            .text_size(px(ui::BODY))
                            .child(div().text_color(secondary).child("Under:"))
                            .child(div().text_color(primary).font_weight(gpui_kit::FontWeight::MEDIUM).child(root_name)),
                    )
                    .child(
                        div()
                            .id("auto-open-rows")
                            .flex()
                            .flex_col()
                            .max_h(px(200.))
                            .overflow_y_scroll()
                            .border_1()
                            .border_color(border)
                            .rounded(px(4.))
                            .bg(bg)
                            .children(rows),
                    )
                    .child(
                        div()
                            .id("include-unknown")
                            .mt(px(12.))
                            .mb(px(4.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(ui::BODY))
                            .text_color(secondary)
                            .cursor_pointer()
                            .child(ui::checkbox_box(include, false, 14.))
                            .child("Include unknown worktrees (not in recent history)")
                            .on_click(cx.listener(move |dialog, _, _, cx| {
                                dialog.include_unknown = !include;
                                dialog.load(cx);
                            })),
                    )
                    .when_some(error, |d, error| {
                        d.child(
                            div()
                                .mt(px(12.))
                                .px(px(12.))
                                .py(px(8.))
                                .rounded(px(4.))
                                .bg(gpui_kit::rgba(0xf14c4c1a))
                                .text_color(gpui_kit::rgb(0xf14c4c))
                                .text_size(px(ui::BODY))
                                .child(error),
                        )
                    }),
            )
            .child(
                ui::dialog_actions(cx)
                    .child(
                        ui::dialog_button("auto-open-cancel", "Cancel", ui::ButtonKind::Secondary, false, cx)
                            .on_click(cx.listener(|dialog, _, window, cx| dialog.close(window, cx))),
                    )
                    .child(
                        ui::dialog_button("auto-open-ok", ok, ui::ButtonKind::Primary, disabled, cx)
                            .when(!disabled, |b| b.on_click(cx.listener(|dialog, _, window, cx| dialog.confirm(window, cx)))),
                    ),
            )
    }
}

pub fn open(session: Entity<SessionStore>, root_id: String, window: &mut Window, cx: &mut App) {
    let view = cx.new(|cx| AutoOpenDialog::new(session, root_id, window, cx));
    window.open_dialog(cx, move |dialog, _window, cx| ui::dialog_frame(dialog, 400., cx).child(view.clone()));
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
