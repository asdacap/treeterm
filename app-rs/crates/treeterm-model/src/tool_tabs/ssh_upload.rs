//! The Upload tab (`SshUploadPane.tsx`, ssh sessions only): copy a local file into a
//! directory on the session's host with `scp` (`ConnectionManager::upload_file`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::{App, AppContext as _, Context, Entity, Subscription, Task};
use treeterm_core::connection::{ConnectionId, ConnectionTarget, StatusKind};
use treeterm_core::fs::FsClient;

use crate::app_store::ConnState;
use crate::web_app::RemoteHost;

/// Where uploads go, as the connection is now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadTarget {
    /// The workspace is on this machine ("only available for SSH sessions").
    Local,
    /// The connection is gone from the app.
    Gone,
    Remote {
        /// The label, or `user@host`.
        name: String,
        user: String,
        status: LinkStatus,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkStatus {
    Connected,
    /// Uploads wait until the connection is back. Shown as Electron's status word.
    Down(StatusKind),
}

/// The connection's state as the tab tells it.
pub fn link_status(state: &ConnState) -> LinkStatus {
    match state {
        ConnState::Connected(_) => LinkStatus::Connected,
        ConnState::Connecting(_) => LinkStatus::Down(StatusKind::Connecting),
        ConnState::Reconnecting { .. } => LinkStatus::Down(StatusKind::Reconnecting),
        ConnState::Failed { .. } => LinkStatus::Down(StatusKind::Error),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    Idle,
    Uploading,
    /// Uploaded to this remote path.
    Done(String),
    Failed(String),
}

pub struct SshUpload {
    remote: Option<RemoteHost>,
    local_path: Option<PathBuf>,
    /// `None` until edited: the user's home directory on the host.
    remote_dir: Option<String>,
    upload: UploadState,
    _upload: Option<Task<()>>,
    _connection: Option<Subscription>,
}

/// The Upload tabs' models of the ssh connections, by connection, workspace and tab. Each
/// connection generation (every reconnect) builds its workspaces' tabs anew; an Upload tab
/// takes its model from here, so the chosen file, the directory and an upload under way, with
/// its result, survive reconnects, as Electron's `SshUploadPane` stays mounted through them. A
/// model goes when its tab is closed, its workspace removed or its connection closed.
#[derive(Default)]
pub struct UploadTabs {
    tabs: HashMap<UploadTabKey, Entity<SshUpload>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct UploadTabKey {
    connection: ConnectionId,
    workspace: String,
    tab: String,
}

impl UploadTabKey {
    fn new(connection: &ConnectionId, workspace: &str, tab: &str) -> Self {
        Self { connection: connection.clone(), workspace: workspace.to_owned(), tab: tab.to_owned() }
    }
}

impl UploadTabs {
    /// The model of Upload tab `tab` of `workspace` on `remote`'s connection, made on first use.
    pub fn tab(
        &mut self,
        remote: &RemoteHost,
        workspace: &str,
        tab: &str,
        cx: &mut Context<Self>,
    ) -> Entity<SshUpload> {
        self.tabs
            .entry(UploadTabKey::new(&remote.connection, workspace, tab))
            .or_insert_with(|| {
                let remote = remote.clone();
                cx.new(|cx| SshUpload::new(Some(remote), cx))
            })
            .clone()
    }

    /// The model kept for that tab, if any.
    pub fn get(&self, connection: &ConnectionId, workspace: &str, tab: &str) -> Option<&Entity<SshUpload>> {
        self.tabs.get(&UploadTabKey::new(connection, workspace, tab))
    }

    /// The tab was closed.
    pub fn forget_tab(&mut self, connection: &ConnectionId, workspace: &str, tab: &str) {
        self.tabs.remove(&UploadTabKey::new(connection, workspace, tab));
    }

    /// The workspace was removed from the connection's session.
    pub fn forget_workspace(&mut self, connection: &ConnectionId, workspace: &str) {
        self.tabs.retain(|key, _| !(key.connection == *connection && key.workspace == workspace));
    }

    /// The connection was closed for good.
    pub fn forget_connection(&mut self, connection: &ConnectionId) {
        self.tabs.retain(|key, _| key.connection != *connection);
    }
}

/// The last path segment (`basename`), for POSIX or Windows separators.
pub fn basename(path: &str) -> &str {
    path.split(['/', '\\']).rfind(|part| !part.is_empty()).unwrap_or(path)
}

/// `<dir>/<file name>` on the host.
pub fn destination(remote_dir: &str, local: &Path) -> String {
    let local = local.to_string_lossy();
    format!("{}/{}", remote_dir.trim().trim_end_matches('/'), basename(&local))
}

/// Where "Browse..." starts: the directory as typed, trimmed, else the user's home on the
/// host (`remoteDir.trim() || /home/<user>`).
pub fn browse_start(remote_dir: &str, target: &UploadTarget) -> String {
    match (remote_dir.trim(), target) {
        ("", UploadTarget::Remote { user, .. }) => format!("/home/{user}"),
        (dir, _) => dir.to_owned(),
    }
}

impl SshUpload {
    pub fn new(remote: Option<RemoteHost>, cx: &mut Context<Self>) -> Self {
        // Follow the connection's status (connected, reconnecting, gone).
        let connection = remote
            .as_ref()
            .and_then(|remote| remote.app.upgrade())
            .map(|app| cx.observe(&app, |_, _, cx| cx.notify()));
        Self { remote, local_path: None, remote_dir: None, upload: UploadState::Idle, _upload: None, _connection: connection }
    }

    pub fn target(&self, cx: &App) -> UploadTarget {
        let Some(remote) = &self.remote else {
            return UploadTarget::Local;
        };
        let Some(app) = remote.app.upgrade() else {
            return UploadTarget::Gone;
        };
        let Some(slot) = app.read(cx).session_slot(&remote.connection) else {
            return UploadTarget::Gone;
        };
        let ConnectionTarget::Remote(config) = &slot.target else {
            return UploadTarget::Local;
        };
        UploadTarget::Remote { name: config.display_name(), user: config.user.clone(), status: link_status(&slot.state) }
    }

    /// The host's file system for "Browse...": the connected session's, none while the
    /// connection is down.
    pub fn fs(&self, cx: &App) -> Option<FsClient> {
        let remote = self.remote.as_ref()?;
        let app = remote.app.upgrade()?;
        let session = app.read(cx).session_slot(&remote.connection)?.session()?;
        Some(session.read(cx).connection().fs_client())
    }

    pub fn local_path(&self) -> Option<&Path> {
        self.local_path.as_deref()
    }

    /// "Choose file…".
    pub fn set_local_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.local_path = Some(path);
        self.upload = UploadState::Idle;
        cx.notify();
    }

    /// The destination directory: as typed, else `/home/<user>`.
    pub fn remote_dir(&self, cx: &App) -> String {
        match (&self.remote_dir, self.target(cx)) {
            (Some(dir), _) => dir.clone(),
            (None, UploadTarget::Remote { user, .. }) => format!("/home/{user}"),
            (None, UploadTarget::Local | UploadTarget::Gone) => String::new(),
        }
    }

    /// See [`browse_start`].
    pub fn browse_start(&self, cx: &App) -> String {
        browse_start(&self.remote_dir(cx), &self.target(cx))
    }

    pub fn set_remote_dir(&mut self, dir: &str, cx: &mut Context<Self>) {
        self.remote_dir = Some(dir.to_owned());
        cx.notify();
    }

    pub fn upload_state(&self) -> &UploadState {
        &self.upload
    }

    pub fn can_upload(&self, cx: &App) -> bool {
        let connected = matches!(self.target(cx), UploadTarget::Remote { status: LinkStatus::Connected, .. });
        self.local_path.is_some() && !self.remote_dir(cx).trim().is_empty() && connected && self.upload != UploadState::Uploading
    }

    /// "Upload".
    pub fn upload(&mut self, cx: &mut Context<Self>) {
        if !self.can_upload(cx) {
            return;
        }
        let (Some(local), Some(remote)) = (self.local_path.clone(), self.remote.clone()) else {
            return;
        };
        let Some(app) = remote.app.upgrade() else {
            return;
        };
        let dest = destination(&self.remote_dir(cx), &local);
        let copy = match app.read(cx).manager().upload_file(&remote.connection, local, dest.clone()) {
            Ok(copy) => copy,
            Err(error) => {
                self.upload = UploadState::Failed(error.to_string());
                cx.notify();
                return;
            }
        };
        self.upload = UploadState::Uploading;
        self._upload = Some(cx.spawn(async move |this, cx| {
            let state = match copy.await {
                Ok(Ok(())) => UploadState::Done(dest),
                Ok(Err(error)) => UploadState::Failed(error.to_string()),
                Err(lost) => UploadState::Failed(lost.to_string()),
            };
            match this.update(cx, |model, cx| {
                model.upload = state;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {} // Err: tab closed
            }
        }));
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_file_keeps_its_name_in_the_remote_directory() {
        assert_eq!(basename("/Users/me/notes.txt"), "notes.txt");
        assert_eq!(basename("C:\\Users\\me\\a.bin"), "a.bin");
        assert_eq!(basename("dir/"), "dir");
        assert_eq!(destination(" /srv/drop/ ", Path::new("/tmp/a.tar")), "/srv/drop/a.tar");
        assert_eq!(destination("/", Path::new("/tmp/a.tar")), "/a.tar");
    }

    #[test]
    fn a_connection_down_is_named_with_electrons_status_word() {
        let down = |state: ConnState| match link_status(&state) {
            LinkStatus::Down(kind) => kind.to_string(),
            LinkStatus::Connected => panic!("not down"),
        };
        let connecting = ConnState::Connecting(treeterm_core::connection::ConnectPhase::Tunnel);
        assert_eq!(down(connecting), "connecting");
        assert_eq!(down(ConnState::Reconnecting { attempt: 2, error: "lost".to_owned() }), "reconnecting");
        assert_eq!(down(ConnState::Failed { error: "refused".to_owned(), hash_mismatch: false }), "error");
    }

    #[test]
    fn browsing_starts_at_the_typed_directory_else_the_home_directory() {
        let remote = UploadTarget::Remote { name: "me@host".to_owned(), user: "me".to_owned(), status: LinkStatus::Connected };
        assert_eq!(browse_start("  /srv/drop ", &remote), "/srv/drop");
        assert_eq!(browse_start("", &remote), "/home/me");
        assert_eq!(browse_start("   ", &remote), "/home/me");
        // Off ssh the tab shows no browser, and there is no home to fall back on.
        assert_eq!(browse_start("", &UploadTarget::Local), "");
    }
}
