//! A connection's live system metrics (`SystemMonitor.tsx`): the core monitor script run on
//! the daemon's host every few seconds, with CPU and memory histories, and killing a process.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{Context, Task};
use treeterm_core::exec::{ExecClient, ExecRequest, ExecTimeout};
use treeterm_core::system_monitor::{MONITOR_SCRIPT, SystemMetrics, UsageHistory, parse_metrics};

const POLL_INTERVAL: Duration = Duration::from_secs(4);

#[derive(Debug, Clone)]
pub enum Metrics {
    Loading,
    Loaded(SystemMetrics),
    Failed(String),
}

pub struct SystemMonitor {
    exec: ExecClient,
    metrics: Metrics,
    cpu: UsageHistory,
    memory: UsageHistory,
    updated_at: Option<std::time::SystemTime>,
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
    /// Starts polling; stops when dropped.
    pub fn new(exec: ExecClient, cx: &mut Context<Self>) -> Self {
        let poll = Self::poll(exec.clone(), cx);
        Self {
            exec,
            metrics: Metrics::Loading,
            cpu: UsageHistory::default(),
            memory: UsageHistory::default(),
            updated_at: None,
            _poll: poll,
            _kill: None,
        }
    }

    /// Back to "Collecting…" and poll again from now (Electron's Retry / refresh button).
    pub fn refresh(&mut self, cx: &mut Context<Self>) {
        self.metrics = Metrics::Loading;
        self._poll = Self::poll(self.exec.clone(), cx);
        cx.notify();
    }

    /// When the shown metrics were collected.
    pub fn updated_at(&self) -> Option<std::time::SystemTime> {
        self.updated_at
    }

    fn poll(exec: ExecClient, cx: &mut Context<Self>) -> Task<()> {
        let executor = cx.background_executor().clone();
        let poller = exec;
        cx.spawn(async move |this, cx| {
            loop {
                let result = poller
                    .run(request("sh", vec!["-c".to_owned(), MONITOR_SCRIPT.to_owned()]))
                    .await
                    .map_err(|e| e.to_string());
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
            self.updated_at = Some(std::time::SystemTime::now());
            self.cpu.push(m.cpu.usage_percent);
            self.memory.push(m.memory.usage_percent);
        }
        self.metrics = metrics;
        cx.notify();
    }

    pub fn metrics(&self) -> &Metrics {
        &self.metrics
    }

    pub fn cpu_history(&self) -> &UsageHistory {
        &self.cpu
    }

    pub fn memory_history(&self) -> &UsageHistory {
        &self.memory
    }

    /// `kill -<signal> <pid>` on the daemon's host.
    pub fn kill(&mut self, pid: u32, signal: &str, cx: &mut Context<Self>) {
        let run = self.exec.run(request("kill", vec![format!("-{signal}"), pid.to_string()]));
        self._kill = Some(cx.spawn(async move |_, _| match run.await {
            Ok(output) if output.exit_code == 0 => {}
            Ok(output) => tracing::warn!(pid, stderr = %String::from_utf8_lossy(&output.stderr), "kill failed"),
            Err(error) => tracing::warn!(pid, %error, "kill failed"),
        }));
    }

    /// The process list's Stop / Kill: signal `pid`, then collect again once it is done
    /// (`sendSignal` → `onRefresh`).
    pub fn signal(&mut self, pid: u32, signal: &str, cx: &mut Context<Self>) {
        let run = self.exec.run(request("kill", vec![format!("-{signal}"), pid.to_string()]));
        self._kill = Some(cx.spawn(async move |this, cx| {
            match run.await {
                Ok(output) if output.exit_code == 0 => {}
                Ok(output) => tracing::warn!(pid, stderr = %String::from_utf8_lossy(&output.stderr), "kill failed"),
                Err(error) => tracing::warn!(pid, %error, "kill failed"),
            }
            match this.update(cx, |monitor, cx| monitor.refresh(cx)) {
                Ok(()) | Err(_) => {} // Err: the monitor closed
            }
        }));
    }
}
