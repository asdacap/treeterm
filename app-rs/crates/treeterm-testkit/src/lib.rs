//! A real daemon (`treeterm_daemon::serve`) running in-process, for contract tests.

use std::path::{Path, PathBuf};

use tempfile::TempDir;
use tokio::net::UnixListener;
use tokio::sync::oneshot;

/// Runs until dropped. Socket and workspace files live in a fresh temp directory under
/// `/tmp`, kept short because macOS caps socket paths at ~104 bytes.
pub struct TestDaemon {
    dir: TempDir,
    socket_path: PathBuf,
    workspaces_dir: PathBuf,
    shutdown: Option<oneshot::Sender<()>>,
}

impl TestDaemon {
    /// Start serving. Must be called on a tokio runtime, which keeps driving the daemon.
    pub fn start() -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tt")
            .tempdir_in("/tmp")
            .expect("create temp dir for test daemon");
        let socket_path = dir.path().join("d.sock");
        let workspaces_dir = dir.path().join("workspaces");
        std::fs::create_dir_all(&workspaces_dir).expect("create workspaces dir");
        let listener = UnixListener::bind(&socket_path).expect("bind test daemon socket");
        let (tx, rx) = oneshot::channel::<()>();
        let config = treeterm_daemon::DaemonConfig {
            workspaces_dir: workspaces_dir.clone(),
        };
        tokio::spawn(async move {
            let shutdown = async {
                // Err = the TestDaemon was dropped without sending; shut down either way.
                if rx.await.is_err() {}
            };
            if let Err(error) = treeterm_daemon::serve(config, listener, shutdown).await {
                panic!("test daemon failed: {error}");
            }
        });
        Self {
            dir,
            socket_path,
            workspaces_dir,
            shutdown: Some(tx),
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }

    pub fn workspaces_dir(&self) -> &Path {
        &self.workspaces_dir
    }

    pub fn root(&self) -> &Path {
        self.dir.path()
    }
}

impl Drop for TestDaemon {
    fn drop(&mut self) {
        if let Some(tx) = self.shutdown.take() {
            // The serving task may already be gone (runtime shutting down); nothing to stop then.
            if tx.send(()).is_err() {}
        }
    }
}
