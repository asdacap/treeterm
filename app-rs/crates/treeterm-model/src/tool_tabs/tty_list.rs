//! The TTYs tab (`TtyListBrowser.tsx`): the daemon's PTYs started in this workspace's
//! directory, flagged as orphans when no tab of the session references them, with open
//! (attach a terminal tab), kill and "kill all orphans". Listing and killing are those of
//! "Window → Active Processes" ([`crate::active_processes`]).

use std::collections::HashSet;

use gpui_kit::{App, Context, Task, WeakEntity};
use serde_json::{Value, json};
use treeterm_core::daemon::DaemonConnection;
use treeterm_proto::treeterm::PtySessionInfo;

pub use crate::active_processes::{ProcessList, relative_time};
use crate::session_store::SessionStore;
use crate::workspace_store::WorkspaceStore;

/// One PTY of the workspace.
#[derive(Debug, Clone, PartialEq)]
pub struct TtyRow {
    pub info: PtySessionInfo,
    /// No tab of the session references it (left behind by a reconnect, say).
    pub orphan: bool,
}

/// Every PTY id a tab of `session` references (`collectReferencedPtyIds`).
pub fn referenced_pty_ids(session: &SessionStore, cx: &App) -> HashSet<String> {
    session
        .workspace_ids()
        .filter_map(|id| session.workspace_store(id))
        .flat_map(|store| {
            store
                .read(cx)
                .file()
                .app_states
                .values()
                .filter_map(|tab| tab.state.get("ptyId").and_then(Value::as_str).map(str::to_owned))
                .collect::<Vec<_>>()
        })
        .collect()
}

pub struct TtyList {
    conn: DaemonConnection,
    workspace: WeakEntity<WorkspaceStore>,
    path: String,
    list: ProcessList,
    kill_error: Option<String>,
    _list: Option<Task<()>>,
    _kill: Option<Task<()>>,
}

impl TtyList {
    pub fn new(conn: DaemonConnection, workspace: WeakEntity<WorkspaceStore>, path: String, cx: &mut Context<Self>) -> Self {
        let mut list = Self {
            conn,
            workspace,
            path,
            list: ProcessList::Loading,
            kill_error: None,
            _list: None,
            _kill: None,
        };
        list.refresh(cx);
        list
    }

    /// The workspace directory the PTYs are filtered by.
    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn list(&self) -> &ProcessList {
        &self.list
    }

    pub fn kill_error(&self) -> Option<&str> {
        self.kill_error.as_deref()
    }

    /// The workspace's PTYs, once listed; `referenced` from [`referenced_pty_ids`].
    pub fn rows(&self, referenced: &HashSet<String>) -> Vec<TtyRow> {
        match &self.list {
            ProcessList::Loaded(sessions) => sessions
                .iter()
                .filter(|session| session.cwd == self.path)
                .map(|session| TtyRow { info: session.clone(), orphan: !referenced.contains(&session.id) })
                .collect(),
            ProcessList::Loading | ProcessList::Failed(_) => Vec::new(),
        }
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.list = ProcessList::Loading;
        let list = self.conn.list_pty_sessions();
        self._list = Some(cx.spawn(async move |this, cx| {
            let list = match list.await {
                Ok(Ok(sessions)) => ProcessList::Loaded(sessions),
                Ok(Err(error)) => ProcessList::Failed(error.to_string()),
                Err(lost) => ProcessList::Failed(lost.to_string()),
            };
            match this.update(cx, |model, cx| {
                model.list = list;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }

    /// "Open": attach a new terminal tab to the PTY.
    pub fn open(&self, pty_id: &str, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.upgrade() else {
            return;
        };
        let opened = workspace.update(cx, |w, cx| w.add_tab_with("terminal", json!({ "ptyId": pty_id }), cx));
        if opened.is_none() {
            tracing::error!("the terminal application is not registered");
        }
    }

    pub fn kill(&mut self, pty_id: &str, cx: &mut Context<Self>) {
        self.kill_all(vec![pty_id.to_owned()], cx);
    }

    /// "Kill all orphans".
    pub fn kill_orphans(&mut self, referenced: &HashSet<String>, cx: &mut Context<Self>) {
        let orphans: Vec<String> = self.rows(referenced).into_iter().filter(|row| row.orphan).map(|row| row.info.id).collect();
        if !orphans.is_empty() {
            self.kill_all(orphans, cx);
        }
    }

    /// Kill `ids`, then list again.
    fn kill_all(&mut self, ids: Vec<String>, cx: &mut Context<Self>) {
        let kills: Vec<_> = ids.iter().map(|id| (id.clone(), self.conn.kill_pty(id))).collect();
        self._kill = Some(cx.spawn(async move |this, cx| {
            let mut failures = Vec::new();
            for (id, kill) in kills {
                match kill.await {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => failures.push(format!("{id}: {error}")),
                    Err(lost) => failures.push(format!("{id}: {lost}")),
                }
            }
            match this.update(cx, |model, cx| {
                model.kill_error = (!failures.is_empty()).then(|| format!("Failed to kill {}", failures.join("; ")));
                model.refresh(cx);
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
    }
}

/// `active <relative time>` and `created <relative time>` read from the daemon's
/// millisecond timestamps.
pub fn now_millis() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or_default()
}
