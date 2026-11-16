//! The session pane's SSH > Monitor (`SystemMonitor.tsx`) on a scripted host: what each run
//! shows, the 4s cadence, the history kept per connection, Retry and Stop / Kill.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt as _;
use futures::channel::mpsc;
use futures::stream::BoxStream;
use gpui_kit::{AppContext as _, Entity, TestAppContext};
use treeterm_core::connection::ConnectionId;
use treeterm_core::exec::{ExecApi, ExecError, ExecEvent, ExecRequest, ExecTimeout};
use treeterm_core::system_monitor::MONITOR_SCRIPT;
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
fn runs_the_script_from_the_root_with_electrons_30s_timeout(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let _monitor = monitor(cx, "c1", &host);
    let requests = host.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        (requests[0].cwd.as_str(), requests[0].command.as_str()),
        ("/", "sh")
    );
    assert_eq!(requests[0].args, ["-c", MONITOR_SCRIPT]);
    assert_eq!(
        requests[0].timeout,
        ExecTimeout::After(Duration::from_secs(30))
    );
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
fn a_run_that_exits_non_zero_reads_command_exited_with_code(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    // A timed-out run: the daemon reports -1 with what it printed so far.
    host.exit(0, "OS=Linux\nCPU_USAGE=12\n", -1);
    cx.run_until_parked();
    assert_eq!(failed(cx, &monitor), "Command exited with code -1");
    assert!(cpu_samples(cx, &monitor).is_empty());
    assert!(monitor.read_with(cx, |m, _| m.updated_at().is_none()));

    // Polling carries on; the next good run shows the metrics again.
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    host.exit(1, "", 3);
    cx.run_until_parked();
    assert_eq!(failed(cx, &monitor), "Command exited with code 3");
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    host.exit(2, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
    assert_eq!(cpu_samples(cx, &monitor), [23.5]);
}

#[gpui_kit::test]
fn a_run_that_cannot_finish_reads_why(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    host.send(
        0,
        vec![Err(ExecError::System("spawn sh: not found".to_owned()))],
    );
    cx.run_until_parked();
    assert_eq!(failed(cx, &monitor), "spawn sh: not found");

    // A result the daemon never sent reads without the script.
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    host.send(
        1,
        vec![Err(ExecError::NoResult {
            command: format!("sh -c {MONITOR_SCRIPT}"),
            after: Duration::from_secs(40),
        })],
    );
    cx.run_until_parked();
    assert_eq!(failed(cx, &monitor), "No result from the daemon after 40s");
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
fn runs_start_every_4s_and_a_run_finishing_after_a_later_one_is_dropped(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    // The first run is still going when the next one is due.
    cx.executor()
        .advance_clock(POLL_INTERVAL - Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 1);
    cx.executor().advance_clock(Duration::from_millis(1));
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 2);

    host.exit(1, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
    // The first run's later failure is older news.
    host.exit(0, "", 1);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
    assert_eq!(cpu_samples(cx, &monitor), [23.5]);

    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 3);
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

#[gpui_kit::test]
fn refresh_shows_collecting_cancels_the_runs_and_runs_again_now(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    host.exit(0, "", 2);
    cx.run_until_parked();
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 2);

    monitor.update(cx, |m, cx| m.refresh(cx));
    assert!(matches!(metrics(cx, &monitor), Metrics::Loading));
    cx.run_until_parked();
    assert!(host.cancelled(1));
    assert_eq!(host.requests().len(), 3);
    // The next run is 4s after the refresh.
    cx.executor().advance_clock(POLL_INTERVAL);
    cx.run_until_parked();
    assert_eq!(host.requests().len(), 4);
    host.exit(2, LINUX_OUTPUT, 0);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
}

#[gpui_kit::test]
fn stop_and_kill_signal_the_process_then_collect_again(cx: &mut TestAppContext) {
    let host = Arc::new(Host::default());
    let monitor = monitor(cx, "c1", &host);
    host.exit(0, LINUX_OUTPUT, 0);
    cx.run_until_parked();

    monitor.update(cx, |m, cx| m.signal(1234, "TERM", cx));
    cx.run_until_parked();
    let kill = host.requests().remove(1);
    assert_eq!((kill.cwd.as_str(), kill.command.as_str()), ("/", "kill"));
    assert_eq!(kill.args, ["-TERM", "1234"]);
    assert_eq!(kill.timeout, ExecTimeout::After(Duration::from_secs(30)));
    // Still showing the metrics until the signal is sent.
    assert!(matches!(metrics(cx, &monitor), Metrics::Loaded(_)));
    host.exit(1, "", 0);
    cx.run_until_parked();
    assert!(matches!(metrics(cx, &monitor), Metrics::Loading));
    assert_eq!(host.requests()[2].command, "sh");

    // A kill that fails (the process is gone) is logged, and collects again all the same.
    monitor.update(cx, |m, cx| m.signal(1234, "9", cx));
    cx.run_until_parked();
    assert_eq!(host.requests()[3].args, ["-9", "1234"]);
    host.exit(3, "", 1);
    cx.run_until_parked();
    assert_eq!(host.requests()[4].command, "sh");
}
