//! Shared harness: a real in-process daemon reached through the real client.

#![allow(dead_code)] // each test binary uses a different subset

use std::time::Duration;

use gpui_kit::{App, Entity, TestAppContext};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::rt::Rt;
use treeterm_testkit::{RefusingDaemon, TestDaemon};

pub struct Harness {
    pub daemon: TestDaemon,
    pub conn: DaemonConnection,
    pub rt: Rt,
}

/// One tokio runtime for every test in the binary, never shut down. tokio reaps child
/// processes through process-wide SIGCHLD state: with a runtime per test, a child spawned
/// after an earlier test's runtime went away could wait forever (seen as a hung
/// `git status` in the second test of a run).
fn runtime() -> &'static tokio::runtime::Runtime {
    static RUNTIME: std::sync::LazyLock<tokio::runtime::Runtime> = std::sync::LazyLock::new(|| {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(4)
            .enable_all()
            .build()
            .expect("test runtime")
    });
    &RUNTIME
}

impl Drop for Harness {
    /// Kill the test's shells; the daemon stops when `daemon` drops.
    fn drop(&mut self) {
        if let Ok(Ok(sessions)) = futures::executor::block_on(self.conn.list_pty_sessions()) {
            for session in sessions {
                if futures::executor::block_on(self.conn.kill_pty(&session.id)).is_err() {}
            }
        }
    }
}

/// Forwards connections on `listen` to `target` until dropped; dropping it cuts them all, as
/// a crashed daemon or a dead tunnel would.
pub struct Proxy {
    task: tokio::task::JoinHandle<()>,
}

impl Proxy {
    pub fn start(listen: &std::path::Path, target: &std::path::Path) -> Self {
        match std::fs::remove_file(listen) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("cannot clear {listen:?}: {error}"),
        }
        let tokio = runtime();
        let listener = {
            let _enter = tokio.enter();
            tokio::net::UnixListener::bind(listen).expect("bind proxy")
        };
        let target = target.to_owned();
        let task = tokio.spawn(async move {
            // Dropped with the task: aborts every proxied connection.
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let (mut inbound, _) = listener.accept().await.expect("proxy accept");
                let target = target.clone();
                connections.spawn(async move {
                    let mut outbound = tokio::net::UnixStream::connect(&target)
                        .await
                        .expect("proxy dial");
                    // Ends when either side closes; nothing to report then.
                    if tokio::io::copy_bidirectional(&mut inbound, &mut outbound)
                        .await
                        .is_err()
                    {}
                });
            }
        });
        Self { task }
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn harness() -> Harness {
    let tokio = runtime();
    let daemon = {
        let _enter = tokio.enter();
        TestDaemon::start()
    };
    let rt = Rt::new(tokio.handle().clone());
    let conn = futures::executor::block_on(DaemonConnection::open(&rt, daemon.socket_path()))
        .unwrap()
        .unwrap();
    Harness { daemon, conn, rt }
}

/// A daemon that fails every call with `status` (one too old for this app, say), and a
/// connection to it.
pub fn refusing_daemon(status: tonic::Status) -> (RefusingDaemon, DaemonConnection) {
    let tokio = runtime();
    let daemon = {
        let _enter = tokio.enter();
        RefusingDaemon::start(status)
    };
    let rt = Rt::new(tokio.handle().clone());
    let conn = futures::executor::block_on(DaemonConnection::open(&rt, daemon.socket_path()))
        .unwrap()
        .unwrap();
    (daemon, conn)
}

/// Daemon events arrive from tokio threads in real time; pump the GPUI test scheduler
/// until `done` holds.
pub fn wait_until<T: 'static>(
    cx: &mut TestAppContext,
    entity: &Entity<T>,
    done: impl Fn(&T) -> bool,
) {
    wait_until_app(cx, entity, |e, _| done(e));
}

/// Pump the GPUI test scheduler until `result` resolves.
pub fn recv<T>(cx: &mut TestAppContext, mut result: futures::channel::oneshot::Receiver<T>) -> T {
    for _ in 0..1000 {
        cx.run_until_parked();
        match result.try_recv() {
            Ok(Some(value)) => return value,
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(canceled) => panic!("the result was dropped: {canceled}"),
        }
    }
    panic!("no result");
}

/// [`wait_until`] for conditions that read other entities.
pub fn wait_until_app<T: 'static>(
    cx: &mut TestAppContext,
    entity: &Entity<T>,
    done: impl Fn(&T, &App) -> bool,
) {
    for _ in 0..1000 {
        cx.run_until_parked();
        if entity.read_with(cx, |e, cx| done(e, cx)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("condition not reached");
}
