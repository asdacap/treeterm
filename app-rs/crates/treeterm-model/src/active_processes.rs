//! "Window → Active Processes": every PTY the daemon is running, a live view of the selected
//! one, and a way to stop it. Port of `ActiveProcessesDialog.tsx` + `utils/ttyDisplay.ts`.

use gpui_kit::{AppContext as _, Context, Entity, Task};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::settings::TerminalSettings;
use treeterm_proto::treeterm::PtySessionInfo;
use treeterm_term::fit::WidthLimit;
use treeterm_term::palette::DEFAULT_BACKGROUND;

use crate::terminal::{TerminalModel, TerminalOptions};

#[derive(Debug, Clone)]
pub enum ProcessList {
    Loading,
    Loaded(Vec<PtySessionInfo>),
    Failed(String),
}

pub struct ActiveProcesses {
    conn: DaemonConnection,
    settings: TerminalSettings,
    list: ProcessList,
    /// The selected PTY and a live terminal attached to it.
    selected: Option<(String, Entity<TerminalModel>)>,
    stop_error: Option<String>,
    _list: Option<Task<()>>,
    _stop: Option<Task<()>>,
}

impl ActiveProcesses {
    pub fn new(conn: DaemonConnection, settings: TerminalSettings, cx: &mut Context<Self>) -> Self {
        let mut model = Self {
            conn,
            settings,
            list: ProcessList::Loading,
            selected: None,
            stop_error: None,
            _list: None,
            _stop: None,
        };
        model.refresh(cx);
        model
    }

    pub fn list(&self) -> &ProcessList {
        &self.list
    }

    pub fn selected(&self) -> Option<(&str, &Entity<TerminalModel>)> {
        self.selected.as_ref().map(|(id, terminal)| (id.as_str(), terminal))
    }

    pub fn stop_error(&self) -> Option<&str> {
        self.stop_error.as_deref()
    }

    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        let list = self.conn.list_pty_sessions();
        self._list = Some(cx.spawn(async move |this, cx| {
            let list = match list.await {
                Ok(Ok(sessions)) => ProcessList::Loaded(sessions),
                Ok(Err(error)) => ProcessList::Failed(error.to_string()),
                Err(lost) => ProcessList::Failed(lost.to_string()),
            };
            match this.update(cx, |model, cx| {
                // A selection whose PTY is gone is cleared, as in Electron.
                if let (ProcessList::Loaded(sessions), Some((id, _))) = (&list, &model.selected)
                    && !sessions.iter().any(|s| &s.id == id)
                {
                    model.selected = None;
                }
                model.list = list;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: dialog closed
            }
        }));
    }

    /// Attach a live terminal to `id` (it takes over the PTY's size while shown, like
    /// Electron's `PtyViewer`).
    pub fn select(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_ref().is_some_and(|(shown, _)| shown == id) {
            return;
        }
        let options = TerminalOptions {
            pty_id: id.to_owned(),
            background: DEFAULT_BACKGROUND,
            settings: self.settings.clone(),
            keep_on_exit: true,
            width_limit: WidthLimit::Off,
            idle_timeout: None,
            // A viewer, not a workspace: nobody meters it.
            traffic: treeterm_core::traffic::ByteCounter::default(),
        };
        let conn = self.conn.clone();
        let terminal = cx.new(|cx| TerminalModel::new(conn, options, cx));
        self.selected = Some((id.to_owned(), terminal));
        self.stop_error = None;
        cx.notify();
    }

    /// Kill the selected PTY, then re-list.
    pub fn stop(&mut self, cx: &mut Context<Self>) {
        let Some((id, _)) = self.selected.take() else {
            return;
        };
        self.kill_then_refresh(id, cx);
    }

    /// Kill any PTY (the session pane's per-row Kill), closing its viewer if it is shown,
    /// then re-list.
    pub fn kill(&mut self, id: &str, cx: &mut Context<Self>) {
        if self.selected.as_ref().is_some_and(|(shown, _)| shown == id) {
            self.selected = None;
        }
        self.list = ProcessList::Loading;
        self.kill_then_refresh(id.to_owned(), cx);
    }

    /// Close the viewer; the PTY keeps running.
    pub fn deselect(&mut self, cx: &mut Context<Self>) {
        if self.selected.take().is_some() {
            cx.notify();
        }
    }

    /// List again from scratch: "Loading…" until the daemon answers (`SessionTtyList`'s
    /// Refresh).
    pub fn reload(&mut self, cx: &mut Context<Self>) {
        self.list = ProcessList::Loading;
        cx.notify();
        self.refresh(cx);
    }

    fn kill_then_refresh(&mut self, id: String, cx: &mut Context<Self>) {
        let kill = self.conn.kill_pty(&id);
        self._stop = Some(cx.spawn(async move |this, cx| {
            let failed = match kill.await {
                Ok(Ok(())) => None,
                Ok(Err(error)) => Some(error.to_string()),
                Err(lost) => Some(lost.to_string()),
            };
            match this.update(cx, |model, cx| {
                model.stop_error = failed.map(|e| format!("Failed to stop {id}: {e}"));
                model.refresh(cx);
            }) {
                Ok(()) | Err(_) => {}
            }
        }));
        cx.notify();
    }
}

/// A PTY's label: the name of the workspace whose path is its cwd, else the cwd's last
/// segment (`getDisplayName`). `workspaces` are `(path, name)` pairs.
pub fn display_name(cwd: &str, workspaces: &[(String, String)]) -> String {
    if let Some((_, name)) = workspaces.iter().find(|(path, _)| path == cwd) {
        return name.clone();
    }
    match cwd.rsplit('/').next() {
        Some(last) if !last.is_empty() => last.to_owned(),
        Some(_) | None => cwd.to_owned(),
    }
}

/// The PTYs of one workspace in the session pane's TTY list (`SessionTtyList`).
#[derive(Debug, Clone, PartialEq)]
pub struct PtyGroup {
    /// The workspace's name, or "Unknown".
    pub label: String,
    pub sessions: Vec<PtySessionInfo>,
}

/// PTYs grouped by the workspace whose path is their cwd, workspaces in first-seen order,
/// unmatched PTYs last under "Unknown" (`groupByWorkspace`). `workspaces` are `(path, name)`.
pub fn group_by_workspace(sessions: &[PtySessionInfo], workspaces: &[(String, String)]) -> Vec<PtyGroup> {
    let mut groups: Vec<(Option<&str>, PtyGroup)> = Vec::new();
    for session in sessions {
        // As Electron's path map: a later workspace with the same path wins.
        let workspace = workspaces.iter().rev().find(|(path, _)| *path == session.cwd);
        let key = workspace.map(|(path, _)| path.as_str());
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, group)) => group.sessions.push(session.clone()),
            None => {
                let label = workspace.map_or_else(|| "Unknown".to_owned(), |(_, name)| name.clone());
                groups.push((key, PtyGroup { label, sessions: vec![session.clone()] }));
            }
        }
    }
    // Stable: workspace groups keep their order, Unknown goes last.
    groups.sort_by_key(|(key, _)| key.is_none());
    groups.into_iter().map(|(_, group)| group).collect()
}

/// Compact "Ns/Nm/Nh/Nd ago" (`formatRelativeTime`); times in milliseconds.
pub fn relative_time(timestamp_ms: i64, now_ms: i64) -> String {
    let seconds = (now_ms - timestamp_ms).max(0) / 1000;
    if seconds < 60 {
        return format!("{seconds}s ago");
    }
    let minutes = seconds / 60;
    if minutes < 60 {
        return format!("{minutes}m ago");
    }
    let hours = minutes / 60;
    if hours < 24 {
        return format!("{hours}h ago");
    }
    format!("{}d ago", hours / 24)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn processes_are_named_after_their_workspace_or_cwd() {
        let workspaces = vec![("/repo/app".to_owned(), "App".to_owned())];
        assert_eq!(display_name("/repo/app", &workspaces), "App");
        assert_eq!(display_name("/home/me/src/", &workspaces), "/home/me/src/");
        assert_eq!(display_name("/home/me/src", &workspaces), "src");
    }

    fn pty(id: &str, cwd: &str) -> PtySessionInfo {
        PtySessionInfo { id: id.to_owned(), cwd: cwd.to_owned(), ..Default::default() }
    }

    #[test]
    fn ptys_group_by_workspace_with_unknown_last() {
        let workspaces = vec![("/a".to_owned(), "A".to_owned()), ("/b".to_owned(), "B".to_owned())];
        let sessions = [pty("1", "/tmp"), pty("2", "/b"), pty("3", "/a"), pty("4", "/b"), pty("5", "/x")];
        let groups = group_by_workspace(&sessions, &workspaces);
        let shape: Vec<(&str, Vec<&str>)> = groups
            .iter()
            .map(|g| (g.label.as_str(), g.sessions.iter().map(|s| s.id.as_str()).collect()))
            .collect();
        assert_eq!(shape, [("B", vec!["2", "4"]), ("A", vec!["3"]), ("Unknown", vec!["1", "5"])]);
        assert!(group_by_workspace(&[], &workspaces).is_empty());
    }

    #[test]
    fn relative_times_use_the_largest_whole_unit() {
        let now = 10_000_000_000;
        assert_eq!(relative_time(now - 5_000, now), "5s ago");
        assert_eq!(relative_time(now - 120_000, now), "2m ago");
        assert_eq!(relative_time(now - 3 * 3_600_000, now), "3h ago");
        assert_eq!(relative_time(now - 49 * 3_600_000, now), "2d ago");
        assert_eq!(relative_time(now + 1_000, now), "0s ago");
    }
}
