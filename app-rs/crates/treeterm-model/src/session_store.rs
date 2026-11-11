//! One daemon session: its workspace refs and each workspace's body.
//!
//! The ref list comes from `SessionWatch`; each workspace body is observed through
//! `WatchFile` on `<workspace_data_dir>/<id>.json`. Both watches deliver the current
//! state first, so there is no separate initial fetch.
//!
//! This is the read side of `src/renderer/store/createSessionStore.ts`. The write side
//! (ref publishing with version CAS, body CAS writes, locks) is ported on top of it.

use std::collections::{HashMap, HashSet};

use futures::StreamExt;
use gpui_kit::{Context, Task};
use treeterm_core::daemon::{ApiError, DaemonConnection};
use treeterm_core::tree::{self, TreeNode, TreeRow};
use treeterm_core::wire::workspace_file::{WorkspaceFile, parse_workspace_file};
use treeterm_proto::treeterm::file_watch_event::State as FileState;
use treeterm_proto::treeterm::{FileWatchEvent, Session, SessionWatchEvent, WorkspaceRef};

pub enum SessionState {
    /// Waiting for the first SessionWatch event.
    Waiting,
    Live(Session),
    /// The session watch failed or ended; the connection layer must rebuild this store.
    Failed(String),
}

pub enum WorkspaceEntry {
    Loading,
    Error(String),
    Loaded(Box<WorkspaceFile>),
}

struct WorkspaceSlot {
    entry: WorkspaceEntry,
    /// Dropping it cancels the file watch.
    _watch: Task<()>,
}

pub struct SessionStore {
    conn: DaemonConnection,
    state: SessionState,
    workspaces: HashMap<String, WorkspaceSlot>,
    _session_watch: Task<()>,
}

impl SessionStore {
    pub fn new(conn: DaemonConnection, client_id: String, cx: &mut Context<Self>) -> Self {
        let mut events = conn.watch_session(&client_id);
        let session_watch = cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                if this
                    .update(cx, |store, cx| store.on_session_event(event, cx))
                    .is_err()
                {
                    return; // store dropped
                }
            }
            match this.update(cx, |store, cx| {
                store.fail("session watch ended: connection to the daemon lost", cx)
            }) {
                Ok(()) | Err(_) => {} // Err: store dropped, nothing to report to
            }
        });
        Self {
            conn,
            state: SessionState::Waiting,
            workspaces: HashMap::new(),
            _session_watch: session_watch,
        }
    }

    pub fn state(&self) -> &SessionState {
        &self.state
    }

    pub fn connection(&self) -> &DaemonConnection {
        &self.conn
    }

    /// The session's workspace refs in session order; empty until the session is live.
    pub fn refs(&self) -> &[WorkspaceRef] {
        match &self.state {
            SessionState::Live(session) => &session.workspace_refs,
            SessionState::Waiting | SessionState::Failed(_) => &[],
        }
    }

    /// The workspace's entry. `id` must come from [`Self::refs`] or [`Self::tree_rows`] of
    /// the current state: every live ref has an entry.
    pub fn workspace(&self, id: &str) -> &WorkspaceEntry {
        &self.workspaces[id].entry
    }

    /// Visible tree rows in display order; empty until the session is live.
    pub fn tree_rows(&self, collapsed: &HashSet<String>) -> Vec<TreeRow> {
        let SessionState::Live(session) = &self.state else {
            return Vec::new();
        };
        let nodes: Vec<TreeNode<'_>> = session
            .workspace_refs
            .iter()
            .map(|r| match &self.workspaces[&r.id].entry {
                WorkspaceEntry::Loaded(file) => TreeNode::Loaded { id: &r.id, file },
                WorkspaceEntry::Loading | WorkspaceEntry::Error(_) => {
                    TreeNode::Unloaded { id: &r.id }
                }
            })
            .collect();
        tree::flatten(&nodes, collapsed)
    }

    fn fail(&mut self, reason: &str, cx: &mut Context<Self>) {
        tracing::error!(reason, "session store failed");
        self.state = SessionState::Failed(reason.to_owned());
        cx.notify();
    }

    fn on_session_event(
        &mut self,
        event: Result<SessionWatchEvent, ApiError>,
        cx: &mut Context<Self>,
    ) {
        let session = match event.map(|e| e.session) {
            Ok(Some(session)) => session,
            Ok(None) => return self.fail("SessionWatch event without a session", cx),
            Err(error) => return self.fail(&format!("session watch failed: {error}"), cx),
        };
        if let SessionState::Live(current) = &self.state
            && session.version < current.version
        {
            return; // stale snapshot overtaken by a newer one
        }
        self.reconcile_workspaces(&session, cx);
        self.state = SessionState::Live(session);
        cx.notify();
    }

    fn reconcile_workspaces(&mut self, session: &Session, cx: &mut Context<Self>) {
        let wanted: HashSet<&str> = session.workspace_refs.iter().map(|r| r.id.as_str()).collect();
        self.workspaces.retain(|id, _| wanted.contains(id.as_str()));
        for r in &session.workspace_refs {
            if self.workspaces.contains_key(&r.id) {
                continue;
            }
            let watch = self.watch_workspace(&session.workspace_data_dir, r.id.clone(), cx);
            self.workspaces.insert(
                r.id.clone(),
                WorkspaceSlot {
                    entry: WorkspaceEntry::Loading,
                    _watch: watch,
                },
            );
        }
    }

    fn watch_workspace(&self, data_dir: &str, id: String, cx: &mut Context<Self>) -> Task<()> {
        let mut events = self.conn.watch_file(data_dir, &format!("{id}.json"));
        cx.spawn(async move |this, cx| {
            while let Some(event) = events.next().await {
                let entry = entry_from_event(event);
                if this
                    .update(cx, |store, cx| store.set_entry(&id, entry, cx))
                    .is_err()
                {
                    return; // store dropped
                }
            }
            let ended = WorkspaceEntry::Error("workspace watch ended".to_owned());
            match this.update(cx, |store, cx| store.set_entry(&id, ended, cx)) {
                Ok(()) | Err(_) => {} // Err: store dropped
            }
        })
    }

    fn set_entry(&mut self, id: &str, entry: WorkspaceEntry, cx: &mut Context<Self>) {
        // The ref may have been removed while the event was in flight.
        let Some(slot) = self.workspaces.get_mut(id) else {
            return;
        };
        slot.entry = entry;
        cx.notify();
    }
}

fn entry_from_event(event: Result<FileWatchEvent, ApiError>) -> WorkspaceEntry {
    match event.map(|e| e.state) {
        Ok(Some(FileState::Present(file))) => match parse_workspace_file(&file.content) {
            Ok(body) => WorkspaceEntry::Loaded(Box::new(body)),
            Err(error) => WorkspaceEntry::Error(error.to_string()),
        },
        Ok(Some(FileState::Absent(_))) => {
            WorkspaceEntry::Error("workspace file is missing".to_owned())
        }
        Ok(None) => WorkspaceEntry::Error("daemon sent an empty file event".to_owned()),
        Err(error) => WorkspaceEntry::Error(format!("workspace watch failed: {error}")),
    }
}
