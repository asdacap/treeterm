//! A connection's live system metrics (`SystemMonitor.tsx`): the core monitor script run on
//! the daemon's host every few seconds, with CPU and memory histories, and killing a process.
//!
//! Each run takes the connection's daemon of the moment, as Electron's `exec:start` resolves
//! the connection by id: while it is down a run fails with why (shown as the error), polling
//! carries on, and the metrics come back by themselves once it is connected again.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use gpui_kit::{App, Context, Entity, Global, Task};
use treeterm_core::connection::ConnectionId;
use treeterm_core::exec::{ExecApi, ExecRequest, ExecTimeout, collect_output};
use treeterm_core::system_monitor::{MONITOR_SCRIPT, SystemMetrics, UsageHistory, parse_metrics};

use crate::app_store::AppStore;

/// The time between the end of a run and the start of the next.
pub const POLL_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub enum Metrics {
    Loading,
    Loaded(SystemMetrics),
    Failed(String),
}

/// Where a command of the monitor runs: the connection's daemon when it starts, or why it
/// has none.
type ExecSource = Box<dyn Fn(&App) -> Result<Arc<dyn ExecApi>, String>>;

/// Every connection's CPU and memory samples for as long as the app runs, as Electron's
/// module-level `cpuHistory` / `memoryHistory` maps: leaving the Monitor sub-tab, closing the
/// session pane or reconnecting keeps a connection's graphs.
#[derive(Default)]
struct Histories(HashMap<ConnectionId, Usage>);

impl Global for Histories {}

#[derive(Default)]
struct Usage {
    cpu: UsageHistory,
    memory: UsageHistory,
}

pub struct SystemMonitor {
    connection: ConnectionId,
    exec: ExecSource,
    metrics: Metrics,
    updated_at: Option<SystemTime>,
    _poll: Task<()>,
    _kill: Option<Task<()>>,
}

fn request(command: &str, args: Vec<String>) -> ExecRequest {
    ExecRequest {
        cwd: "/".to_owned(),
        command: command.to_owned(),
        args,
        env: HashMap::new(),
        timeout: ExecTimeout::After(Duration::from_secs(15)),
    }
}

impl SystemMonitor {
    /// Starts polling connection `id`, on its daemon of the moment (Main's `getClient`); stops
    /// when dropped.
    pub fn new(app: Entity<AppStore>, id: ConnectionId, cx: &mut Context<Self>) -> Self {
        let connection = id.clone();
        let exec = move |cx: &App| match app.read(cx).manager().client(&connection) {
            Ok(daemon) => Ok(Arc::new(daemon.exec_client()) as Arc<dyn ExecApi>),
            Err(down) => Err(down.to_string()),
        };
        Self::with_exec(id, exec, cx)
    }

    /// Starts polling `connection`, whose commands run where `exec` says when they start;
    /// stops when dropped.
    pub fn with_exec(
        connection: ConnectionId,
        exec: impl Fn(&App) -> Result<Arc<dyn ExecApi>, String> + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            connection,
            exec: Box::new(exec),
            metrics: Metrics::Loading,
            updated_at: None,
            _poll: Self::poll(cx),
            _kill: None,
        }
    }

    /// Back to "Collecting…" and poll again from now (Electron's Retry / refresh button).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.metrics = Metrics::Loading;
        self._poll = Self::poll(cx);
        cx.notify();
    }

    /// When the shown metrics were collected.
    pub fn updated_at(&self) -> Option<SystemTime> {
        self.updated_at
    }

    fn poll(cx: &mut Context<Self>) -> Task<()> {
        let executor = cx.background_executor().clone();
        cx.spawn(async move |this, cx| {
            loop {
                let Ok(exec) = this.update(cx, |monitor, cx| (monitor.exec)(cx)) else {
                    return; // the monitor closed
                };
                let result = match exec {
                    Ok(exec) => {
                        let script = request("sh", vec!["-c".to_owned(), MONITOR_SCRIPT.to_owned()]);
                        collect_output(exec.start(script)).await.map_err(|e| e.to_string())
                    }
                    Err(down) => Err(down),
                };
                let metrics = match result {
                    Ok(output) => Metrics::Loaded(parse_metrics(&String::from_utf8_lossy(&output.stdout))),
                    Err(error) => Metrics::Failed(error),
                };
                if this.update(cx, |monitor, cx| monitor.record(metrics, cx)).is_err() {
                    return;
                }
                executor.timer(POLL_INTERVAL).await;
            }
        })
    }

    fn record(&mut self, metrics: Metrics, cx: &mut Context<Self>) {
        if let Metrics::Loaded(m) = &metrics {
            self.updated_at = Some(SystemTime::now());
            let usage = cx.default_global::<Histories>().0.entry(self.connection.clone()).or_default();
            usage.cpu.push(m.cpu.usage_percent);
            usage.memory.push(m.memory.usage_percent);
        }
        self.metrics = metrics;
        cx.notify();
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    fn usage<'a>(&self, cx: &'a App) -> Option<&'a Usage> {
        cx.try_global::<Histories>().and_then(|histories| histories.0.get(&self.connection))
    }

    /// The connection's CPU usage over its last runs, oldest first, also those of earlier
    /// monitors of it.
    pub fn cpu_samples(&self, cx: &App) -> Vec<f64> {
        self.usage(cx).map(|usage| usage.cpu.samples().collect()).unwrap_or_default()
    }

    /// The connection's memory usage over its last runs, as [`Self::cpu_samples`].
    pub fn memory_samples(&self, cx: &App) -> Vec<f64> {
        self.usage(cx).map(|usage| usage.memory.samples().collect()).unwrap_or_default()
    }

    /// `kill -<signal> <pid>` on the daemon's host now; a failure is logged.
    fn send_signal(&self, pid: u32, signal: &str, cx: &App) -> impl Future<Output = ()> + use<> {
        let kill = request("kill", vec![format!("-{signal}"), pid.to_string()]);
        let run = (self.exec)(cx).map(|exec| collect_output(exec.start(kill)));
        async move {
            let result = match run {
                Ok(run) => run.await.map_err(|e| e.to_string()),
                Err(down) => Err(down),
            };
            match result {
                Ok(output) if output.exit_code == 0 => {}
                Ok(output) => tracing::warn!(pid, stderr = %String::from_utf8_lossy(&output.stderr), "kill failed"),
                Err(error) => tracing::warn!(pid, %error, "kill failed"),
            }
        }
    }

    /// `kill -<signal> <pid>` on the daemon's host.
    pub fn kill(&mut self, pid: u32, signal: &str, cx: &mut Context<Self>) {
        let send = self.send_signal(pid, signal, cx);
        self._kill = Some(cx.spawn(async move |_, _| send.await));
    }

    /// The process list's Stop / Kill: signal `pid`, then collect again once it is done
    /// (`sendSignal` → `onRefresh`).
    pub fn signal(&mut self, pid: u32, signal: &str, cx: &mut Context<Self>) {
        let send = self.send_signal(pid, signal, cx);
        self._kill = Some(cx.spawn(async move |this, cx| {
            send.await;
            match this.update(cx, |monitor, cx| monitor.refresh(cx)) {
                Ok(()) | Err(_) => {} // Err: the monitor closed
            }
        }));
    }
}
