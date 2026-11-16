//! The session pane's TTYs tab (`SessionTtyList.tsx`): the PTYs of one connection, grouped
//! by the workspaces of its session.
//!
//! As in Electron, it lists when it is shown, on Refresh and after a Kill, and Open attaches
//! a viewer, each through the connection's daemon of the moment (`getClient`), so they keep
//! working across a reconnect. While the connection is down a list fails with why, as does a
//! viewer's attach and a Kill; what was listed and an open viewer stay meanwhile, and after
//! the reconnect, until the next Refresh. PTYs are grouped by the workspaces of the session
//! the connection shows: the connected one, else the last one, kept while it is down.

use gpui_kit::{App, Context, Entity, Subscription, Task};
use treeterm_core::connection::ConnectionId;
use treeterm_core::daemon::DaemonConnection;

use crate::active_processes::{
    ProcessList, PtyGroup, attach_viewer, group_by_workspace, workspace_labels,
};
use crate::app_store::{AppStore, SessionSlot};
use crate::session_store::SessionStore;
use crate::terminal::TerminalModel;

pub struct SessionTtyList {
    app: Entity<AppStore>,
    id: ConnectionId,
    /// What the last list found (`loadState`).
    list: ProcessList,
    /// The PTY open in the viewer (`viewingPtyId`).
    viewer: Option<(String, TtyViewer)>,
    /// The session whose workspaces name the groups.
    labels: Labels,
    _list: Option<Task<()>>,
    _app: Subscription,
}

/// The session whose workspaces name the groups (`workspacesMap`).
enum Labels {
    /// The connection has not shown one yet: every PTY is "Unknown".
    Nothing,
    /// Regrouped as its workspaces load, change or go.
    Session {
        session: Entity<SessionStore>,
        _observe: Subscription,
    },
}

/// The viewer of the PTY opened (`PtyViewer`).
pub enum TtyViewer {
    /// A live terminal, attached through the connection's daemon when it was opened.
    Terminal(Entity<TerminalModel>),
    /// The connection had no daemon then: "Failed to attach to process", and why.
    NoDaemon(String),
}

/// What the tab's body shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TtyListBody {
    Loading,
    /// "Failed to load TTY sessions", and why; also when listed while the connection is down.
    Failed(String),
    /// The PTYs by workspace; empty when the daemon runs none ("No TTY sessions").
    Loaded(Vec<PtyGroup>),
}

impl SessionTtyList {
    pub fn new(app: &Entity<AppStore>, id: ConnectionId, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(app, |list, _, cx| list.follow(cx));
        let mut list = Self {
            app: app.clone(),
            id,
            list: ProcessList::Loading,
            viewer: None,
            labels: Labels::Nothing,
            _list: None,
            _app: observe,
        };
        list.follow(cx);
        list.list(cx);
        list
    }

    /// Group by the workspaces of the session the connection shows now.
    fn follow(&mut self, cx: &mut Context<Self>) {
        let shown = self
            .app
            .read(cx)
            .session_slot(&self.id)
            .and_then(SessionSlot::shown_session)
            .cloned();
        match (&self.labels, shown) {
            (Labels::Session { session, .. }, Some(shown)) if *session == shown => {}
            // None to show (yet): the last one's names stay, as Electron's tab keeps the
            // store it was shown with.
            (Labels::Nothing | Labels::Session { .. }, None) => {}
            (Labels::Nothing | Labels::Session { .. }, Some(session)) => {
                let observe = cx.observe(&session, |_, _, cx| cx.notify());
                self.labels = Labels::Session {
                    session,
                    _observe: observe,
                };
                cx.notify();
            }
        }
    }

    /// The connection's daemon now, or why it has none (`getClient`).
    fn client(&self, cx: &App) -> Result<DaemonConnection, String> {
        self.app
            .read(cx)
            .manager()
            .client(&self.id)
            .map_err(|down| down.to_string())
    }

    /// List the PTYs through the connection's daemon: "Loading…" until it answers.
    fn list(&mut self, cx: &mut Context<Self>) {
        let conn = match self.client(cx) {
            Ok(conn) => conn,
            Err(down) => {
                self._list = None;
                self.list = ProcessList::Failed(down);
                cx.notify();
                return;
            }
        };
        self.list = ProcessList::Loading;
        let listed = conn.list_pty_sessions();
        self._list = Some(cx.spawn(async move |this, cx| {
            let list = match listed.await {
                Ok(Ok(sessions)) => ProcessList::Loaded(sessions),
                Ok(Err(error)) => ProcessList::Failed(error.to_string()),
                Err(lost) => ProcessList::Failed(lost.to_string()),
            };
            match this.update(cx, |this, cx| {
                this.list = list;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        }));
        cx.notify();
    }

    pub fn body(&self, cx: &App) -> TtyListBody {
        match &self.list {
            ProcessList::Loading => TtyListBody::Loading,
            ProcessList::Failed(message) => TtyListBody::Failed(message.clone()),
            ProcessList::Loaded(sessions) => {
                let labels = match &self.labels {
                    Labels::Session { session, .. } => workspace_labels(session.read(cx), cx),
                    Labels::Nothing => Vec::new(),
                };
                TtyListBody::Loaded(group_by_workspace(sessions, &labels))
            }
        }
    }

    /// The PTY open in the viewer, and what the viewer has.
    pub fn viewer(&self) -> Option<(&str, &TtyViewer)> {
        self.viewer
            .as_ref()
            .map(|(id, viewer)| (id.as_str(), viewer))
    }

    /// Refresh: list again.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.list(cx);
    }

    /// Open: attach a viewer to `id`.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.viewer.as_ref().is_some_and(|(shown, _)| shown == id) {
            return;
        }
        let viewer = match self.client(cx) {
            Ok(conn) => {
                let settings = self
                    .app
                    .read(cx)
                    .settings()
                    .read(cx)
                    .settings()
                    .terminal
                    .clone();
                TtyViewer::Terminal(attach_viewer(conn, id, &settings, cx))
            }
            Err(down) => TtyViewer::NoDaemon(down),
        };
        self.viewer = Some((id.to_owned(), viewer));
        cx.notify();
    }

    /// Kill `id`, closing its viewer if it is open, then list again once the daemon has
    /// killed it. A failed kill is only logged, as in Electron.
    pub fn kill(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.viewer.as_ref().is_some_and(|(shown, _)| shown == id) {
            self.viewer = None;
        }
        let conn = match self.client(cx) {
            Ok(conn) => conn,
            Err(down) => {
                tracing::error!(pty = id, error = %down, "failed to kill PTY");
                self.list(cx);
                return;
            }
        };
        self._list = None;
        self.list = ProcessList::Loading;
        let killed = conn.kill_pty(id);
        let pty = id.to_owned();
        // Killed even if the tab closes meanwhile.
        cx.spawn(async move |this, cx| {
            match killed.await {
                Ok(Ok(())) => {}
                Ok(Err(error)) => tracing::error!(pty = %pty, %error, "failed to kill PTY"),
                Err(lost) => tracing::error!(pty = %pty, error = %lost, "failed to kill PTY"),
            }
            match this.update(cx, |this, cx| this.list(cx)) {
                Ok(()) | Err(_) => {} // Err: the tab closed
            }
        })
        .detach();
        cx.notify();
    }

    /// Close the viewer; the PTY keeps running.
    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        if self.viewer.take().is_some() {
            cx.notify();
        }
    }
}
