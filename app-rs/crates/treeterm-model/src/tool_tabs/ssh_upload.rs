//! The Upload tab (`SshUploadPane.tsx`, ssh sessions only): copy a local file into a
//! directory on the session's host with `scp` (`ConnectionManager::upload_file`).

use std::path::{Path, PathBuf};

use gpui_kit::{App, Context, Subscription, Task};
use treeterm_core::connection::ConnectionTarget;

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
    /// Uploads wait until the connection is back.
    Down(&'static str),
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

/// The last path segment (`basename`), for POSIX or Windows separators.
pub fn basename(path: &str) -> &str {
    path.split(['/', '\\']).rfind(|part| !part.is_empty()).unwrap_or(path)
}

/// `<dir>/<file name>` on the host.
pub fn destination(remote_dir: &str, local: &Path) -> String {
    let local = local.to_string_lossy();
    format!("{}/{}", remote_dir.trim().trim_end_matches('/'), basename(&local))
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
        let status = match &slot.state {
            ConnState::Connected(_) => LinkStatus::Connected,
            ConnState::Connecting(_) => LinkStatus::Down("connecting"),
            ConnState::Reconnecting { .. } => LinkStatus::Down("reconnecting"),
            ConnState::Failed { .. } => LinkStatus::Down("disconnected"),
        };
        UploadTarget::Remote { name: config.display_name(), user: config.user.clone(), status }
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
}
