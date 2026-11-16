//! The session pane's TTYs tab (`SessionTtyList.tsx`): the PTYs of one connection, grouped
//! by the workspaces of its session.
//!
//! Electron resolves the connection's current daemon client by id on every list, kill and
//! attach, so they keep working across a reconnect. Here each connection generation has its
//! own `DaemonConnection` and a retired one cannot redial, so the list follows the
//! connection's session: a new generation gets a new process list on its connection. Without
//! one, the list fails as Electron's does, with `getClient`'s reason.

use gpui_kit::{App, AppContext as _, Context, Entity, Subscription};
use treeterm_core::connection::ConnectionId;

use crate::active_processes::{
    ActiveProcesses, ProcessList, PtyGroup, group_by_workspace, workspace_labels,
};
use crate::app_store::{AppStore, SessionSlot};
use crate::session_store::SessionStore;
use crate::terminal::TerminalModel;

pub struct SessionTtyList {
    app: Entity<AppStore>,
    id: ConnectionId,
    shown: Shown,
    _app: Subscription,
}

/// The session generation the list is on.
enum Shown {
    /// The connection has no connected session.
    NotConnected,
    Session {
        session: Entity<SessionStore>,
        processes: Entity<ActiveProcesses>,
        /// Regroup as workspaces load, change or go; show what the daemon lists.
        _observe: [Subscription; 2],
    },
}

/// What the tab's body shows.
#[derive(Debug, Clone, PartialEq)]
pub enum TtyListBody {
    Loading,
    /// "Failed to load TTY sessions", and why; also while the connection is down.
    Failed(String),
    /// The PTYs by workspace; empty when the daemon runs none ("No TTY sessions").
    Loaded(Vec<PtyGroup>),
}

impl SessionTtyList {
    pub fn new(app: &Entity<AppStore>, id: ConnectionId, cx: &mut Context<Self>) -> Self {
        let observe = cx.observe(app, |list, app, cx| list.follow(&app, cx));
        let mut list = Self {
            app: app.clone(),
            id,
            shown: Shown::NotConnected,
            _app: observe,
        };
        list.follow(app, cx);
        list
    }

    /// Move to the connection's current session generation, listing its PTYs.
    fn follow(&mut self, app: &Entity<AppStore>, cx: &mut Context<Self>) {
        let session = app
            .read(cx)
            .session_slot(&self.id)
            .and_then(SessionSlot::session)
            .cloned();
        match (&self.shown, session) {
            (Shown::Session { session: shown, .. }, Some(session)) if *shown == session => {}
            // Why it is down may have changed.
            (Shown::NotConnected, None) => cx.notify(),
            (Shown::Session { .. }, None) => {
                self.shown = Shown::NotConnected;
                cx.notify();
            }
            (Shown::NotConnected | Shown::Session { .. }, Some(session)) => {
                let conn = session.read(cx).connection().clone();
                let settings = app.read(cx).settings().read(cx).settings().terminal.clone();
                let processes = cx.new(|cx| ActiveProcesses::new(conn, settings, cx));
                let observe = [
                    cx.observe(&session, |_, _, cx| cx.notify()),
                    cx.observe(&processes, |_, _, cx| cx.notify()),
                ];
                self.shown = Shown::Session {
                    session,
                    processes,
                    _observe: observe,
                };
                cx.notify();
            }
        }
    }

    pub fn body(&self, cx: &App) -> TtyListBody {
        let Shown::Session {
            session, processes, ..
        } = &self.shown
        else {
            // Electron lists through `getClient`, which says why the connection has no
            // daemon (`Connection <id> is reconnecting: …`).
            return match self.app.read(cx).manager().client(&self.id) {
                Err(down) => TtyListBody::Failed(down.to_string()),
                // Connected again, its session still loading: listed once it has.
                Ok(_) => TtyListBody::Loading,
            };
        };
        match processes.read(cx).list() {
            ProcessList::Loading => TtyListBody::Loading,
            ProcessList::Failed(message) => TtyListBody::Failed(message.clone()),
            ProcessList::Loaded(sessions) => TtyListBody::Loaded(group_by_workspace(
                sessions,
                &workspace_labels(session.read(cx), cx),
            )),
        }
    }

    /// The PTY shown in the viewer, with its live terminal.
    pub fn selected<'a>(&self, cx: &'a App) -> Option<(&'a str, &'a Entity<TerminalModel>)> {
        match &self.shown {
            Shown::Session { processes, .. } => processes.read(cx).selected(),
            Shown::NotConnected => None,
        }
    }

    /// Refresh: "Loading…" until the daemon answers.
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.update_processes(cx, ActiveProcesses::reload);
    }

    /// Open: attach a viewer to `id`.
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_processes(cx, |processes, cx| processes.select(id, cx));
    }

    /// Kill `id`, closing its viewer, then list again.
    pub fn kill(&mut self, id: &str, cx: &mut Context<Self>) {
        self.update_processes(cx, |processes, cx| processes.kill(id, cx));
    }

    /// Close the viewer; the PTY keeps running.
    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        self.update_processes(cx, ActiveProcesses::deselect);
    }

    fn update_processes(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ActiveProcesses, &mut Context<ActiveProcesses>),
    ) {
        match &self.shown {
            Shown::Session { processes, .. } => processes.update(cx, f),
            // The tab shows why it cannot list: there is nothing to act on.
            Shown::NotConnected => {}
        }
    }
}
