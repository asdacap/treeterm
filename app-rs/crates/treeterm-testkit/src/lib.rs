//! A real daemon (`treeterm_daemon::serve`) running in-process, for contract tests.

pub mod fake_ssh;

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

/// A daemon that accepts gRPC connections and fails every call with one status: a daemon too
/// old for this app (`Status::unimplemented`), or one whose session cannot be loaded. Runs
/// until dropped.
pub struct RefusingDaemon {
    _dir: TempDir,
    socket_path: PathBuf,
    _server: tokio::task::JoinHandle<()>,
}

impl RefusingDaemon {
    /// Start serving. Must be called on a tokio runtime, which keeps driving it.
    pub fn start(status: tonic::Status) -> Self {
        let dir = tempfile::Builder::new()
            .prefix("tt")
            .tempdir_in("/tmp")
            .expect("create temp dir for the refusing daemon");
        let socket_path = dir.path().join("r.sock");
        let listener = UnixListener::bind(&socket_path).expect("bind refusing daemon socket");
        let server = tokio::spawn(async move {
            let incoming = tokio_stream::wrappers::UnixListenerStream::new(listener);
            if let Err(error) = tonic::transport::Server::builder()
                .add_service(Refuse(status))
                .serve_with_incoming(incoming)
                .await
            {
                panic!("refusing daemon failed: {error}");
            }
        });
        Self {
            _dir: dir,
            socket_path,
            _server: server,
        }
    }

    pub fn socket_path(&self) -> &Path {
        &self.socket_path
    }
}

impl Drop for RefusingDaemon {
    fn drop(&mut self) {
        self._server.abort();
    }
}

/// The daemon's service, answering every method with the status.
#[derive(Clone)]
struct Refuse(tonic::Status);

impl tonic::server::NamedService for Refuse {
    const NAME: &'static str = "treeterm.TreeTermDaemon";
}

impl tonic::codegen::Service<tonic::codegen::http::Request<tonic::body::Body>> for Refuse {
    type Response = tonic::codegen::http::Response<tonic::body::Body>;
    type Error = std::convert::Infallible;
    type Future = std::future::Ready<Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        std::task::Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: tonic::codegen::http::Request<tonic::body::Body>) -> Self::Future {
        std::future::ready(Ok(self.0.clone().into_http()))
    }
}
