//! The session pane's SSH > Monitor (`SystemMonitor.tsx`) on a scripted host: what a run
//! shows and the history kept per connection.

use std::sync::{Arc, Mutex};

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::connection::ConnectionId;
use treeterm_core::exec::{ExecApi, ExecError, ExecEvent, ExecRequest};
use treeterm_model::system_monitor::{Metrics, POLL_INTERVAL, SystemMonitor};

const LINUX_OUTPUT: &str = "OS=Linux\nCPU_USAGE=23.5\nCPU_CORES=4\nMEM_TOTAL=8589934592\nMEM_USED=4294967296\nDISK_START\n/\t524288000\t209715200\t40\nDISK_END\nLOAD_AVG=1.20,0.85,0.60\nUPTIME=14 days, 3:22\nPROC_START\nroot      1234  45.2  2.1  12345  6789 ?        S    10:00   1:23 /usr/bin/node server.js\nPROC_END";

struct Call {
    request: ExecRequest,
    events: mpsc::UnboundedSender<Result<ExecEvent, ExecError>>,
}

/// The daemon's host: each command runs until the test ends it.
#[derive(Default)]
struct Host {
    calls: Mutex<Vec<Call>>,
}

impl ExecApi for Host {
    fn start(&self, request: ExecRequest) -> BoxStream<'static, Result<ExecEvent, ExecError>> {
        let (events, stream) = mpsc::unbounded();
        self.calls.lock().unwrap().push(Call { request, events });
        stream.boxed()
    }
}

impl Host {
    fn requests(&self) -> Vec<ExecRequest> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call.request.clone())
            .collect()
    }

    fn send(&self, call: usize, events: Vec<Result<ExecEvent, ExecError>>) {
        let calls = self.calls.lock().unwrap();
        for event in events {
            calls[call].events.unbounded_send(event).unwrap();
        }
    }

    /// Command `call` prints `stdout` and exits with `code`.
    fn exit(&self, call: usize, stdout: &str, code: i32) {
        self.send(
            call,
            vec![
                Ok(ExecEvent::Stdout(stdout.as_bytes().to_vec())),
                Ok(ExecEvent::Exit { code }),
            ],
        );
    }

    /// Whether command `call` was cancelled (its output is no longer read).
    fn cancelled(&self, call: usize) -> bool {
        self.calls.lock().unwrap()[call].events.is_closed()
    }
}

fn monitor(cx: &mut TestAppContext, connection: &str, host: &Arc<Host>) -> Entity<SystemMonitor> {
    let host = host.clone();
    let monitor = cx.new(|cx| {
        SystemMonitor::with_exec(ConnectionId::new(connection), move |_| Ok(host.clone()), cx)
    });
    cx.run_until_parked();
    monitor
}

fn metrics(cx: &mut TestAppContext, monitor: &Entity<SystemMonitor>) -> Metrics {
    monitor.read_with(cx, |m, _| m.metrics().clone())
}

fn failed(cx: &mut TestAppContext, monitor: &Entity<SystemMonitor>) -> String {
    match metrics(cx, monitor) {
        Metrics::Failed(why) => why,
        other => panic!("not failed: {other:?}"),
    }
}

fn cpu_samples(cx: &mut TestAppContext, monitor: &Entity<SystemMonitor>) -> Vec<f64> {
    monitor.read_with(cx, |m, cx| m.cpu_samples(cx))
}

#[gpui_kit::test]
fn a_run_that_exits_0_shows_its_metrics_and_adds_to_the_graphs(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    assert!(matches!(metrics(cx, &monitor), Metrics::Loading));
    host.exit(0, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    match metrics(cx, &monitor) {
        Metrics::Loaded(m) => {
            assert_eq!((m.cpu.usage_percent, m.cpu.core_count), (23.5, 4));
            assert_eq!(m.processes[0].pid, 1234);
        }
        other => panic!("not loaded: {other:?}"),
    }
    assert_eq!(cpu_samples(cx, &monitor), [23.5]);
    assert_eq!(monitor.read_with(cx, |m, cx| m.memory_samples(cx)), [50.]);
    assert!(monitor.read_with(cx, |m, _| m.updated_at().is_some()));
}

#[gpui_kit::test]
fn a_connection_without_a_daemon_fails_each_run_until_it_has_one(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let up = Arc::new(Mutex::new(false));
    let monitor = cx.new(|cx| {
        let (host, up) = (host.clone(), up.clone());
        SystemMonitor::with_exec(
            ConnectionId::new("c1"),
            move |_| match *up.lock().unwrap() {
                true => Ok(host.clone()),
                false => Err("Connection c1 is connecting".to_owned()),
            },
            cx,
        )
    });
    cx.run_until_parked();
    assert_eq!(failed(cx, &monitor), "Connection c1 is connecting");
    assert!(host.requests().is_empty());

    *up.lock().unwrap() = true;
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    host.exit(0, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
}

#[gpui_kit::test]
fn a_connections_graphs_outlive_its_monitor(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let first = monitor(cx, "c1", &host);
    host.exit(0, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    cx.update(|_| drop(first));
    cx.run_until_parked();

    // Shown again: still collecting, with the graphs it had.
    let again = monitor(cx, "c1", &host);
    assert!(matches!(metrics(cx, &again), Metrics::Loading));
    assert_eq!(cpu_samples(cx, &again), [23.5]);
    let other = monitor(cx, "c2", &host);
    assert!(cpu_samples(cx, &other).is_empty());

    host.exit(
        1,
        &LINUX_OUTPUT.replace("CPU_USAGE=23.5", "CPU_USAGE=40"),
        0,
    );
    cx.run_until_parked();
    assert_eq!(cpu_samples(cx, &again), [23.5, 40.]);
    assert!(cpu_samples(cx, &other).is_empty());
}

#[gpui_kit::test]
fn closing_the_monitor_cancels_its_runs(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    assert!(!host.cancelled(0));
    cx.update(|_| drop(monitor));
    cx.run_until_parked();
    assert!(host.cancelled(0));
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 1);
}
