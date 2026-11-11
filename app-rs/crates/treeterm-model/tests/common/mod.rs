//! Shared harness: a real in-process daemon reached through the real client.

#![allow(dead_code)] // each test binary uses a different subset

use std::time::Duration;

use gpui_kit::{App, Entity, TestAppContext};
use treeterm_core::daemon::DaemonConnection;
use treeterm_core::rt::Rt;
use treeterm_testkit::TestDaemon;

pub struct Harness {
    /// `Some` until dropped; see the `Drop` impl.
    tokio: Option<tokio::runtime::Runtime>,
    pub daemon: TestDaemon,
    pub conn: DaemonConnection,
}

impl Drop for Harness {
    /// The in-process daemon waits for each shell on a blocking thread, and dropping a
    /// runtime waits for its blocking threads. Kill the shells, then let the runtime go
    /// without waiting.
    fn drop(&mut self) {
        if let Ok(Ok(sessions)) = futures::executor::block_on(self.conn.list_pty_sessions()) {
            for session in sessions {
                if futures::executor::block_on(self.conn.kill_pty(&session.id)).is_err() {}
            }
        }
        if let Some(tokio) = self.tokio.take() {
            tokio.shutdown_background();
        }
    }
}

pub fn harness() -> Harness {
    let tokio = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let daemon = {
        let _enter = tokio.enter();
        TestDaemon::start()
    };
    let rt = Rt::new(tokio.handle().clone());
    let conn = futures::executor::block_on(DaemonConnection::open(&rt, daemon.socket_path()))
        .unwrap()
        .unwrap();
    Harness { tokio: Some(tokio), daemon, conn }
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
