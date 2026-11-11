//! A Web App tab: a dev server started in a terminal with `$PORT` set to a free port, and
//! its page once the server answers. Port of `applications/webApp/{renderer,ports,forward}.ts`.
//!
//! Ports are probed with curl through the daemon, so the check runs where the server runs
//! (this machine or the ssh host). On an ssh session the page is reached through an
//! ephemeral port forward owned by the tab.

use std::collections::HashMap;
use std::time::Duration;

use gpui_kit::{AsyncApp, Context, EventEmitter, Task, WeakEntity};
use treeterm_core::connection::{
    ConnectionId, Persistence, PortForwardConfig, PortForwardId, PortForwardStatus,
};
use treeterm_core::exec::{ExecClient, ExecRequest, ExecTimeout};
use treeterm_core::settings::PortForwardSpec;

use crate::app_store::AppStore;

const PORT_RANGE_START: u16 = 20000;
const PORT_RANGE_SIZE: u16 = 25000;
const FIND_PORT_ATTEMPTS: usize = 20;
const POLL_INTERVAL: Duration = Duration::from_secs(1);
const FORWARD_ATTEMPTS: usize = 5;
const FORWARD_POLL: Duration = Duration::from_millis(500);
/// curl's "couldn't connect": nothing listens on the port.
const CURL_COULDNT_CONNECT: i32 = 7;

/// `$PORT` → the port (`resolveCommand`).
pub fn resolve_command(command: &str, port: u16) -> String {
    command.replace("$PORT", &port.to_string())
}

/// A port in 20000–44999 from a random number.
pub fn port_from(random: u128) -> u16 {
    PORT_RANGE_START + (random % u128::from(PORT_RANGE_SIZE)) as u16
}

fn random_port() -> u16 {
    port_from(uuid::Uuid::new_v4().as_u128())
}

/// The forward's id (`forwardIdForTab`).
pub fn forward_id_for_tab(tab_id: &str) -> PortForwardId {
    PortForwardId::new(format!("webapp-{tab_id}"))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebAppPhase {
    AllocatingPort,
    WaitingForServer { port: u16 },
    Forwarding { port: u16 },
    Ready { url: String },
    Error(String),
}

/// Ports to persist in the tab state (`port`, `localPort`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WebAppEvent {
    /// The server's port: start the dev server with it.
    PortAssigned(u16),
    /// The local end of the ssh forward (kept so the page's origin survives restarts).
    LocalPortAssigned(u16),
}

impl EventEmitter<WebAppEvent> for WebAppModel {}

/// Where the server runs, when it is not this machine.
#[derive(Clone)]
pub struct RemoteHost {
    pub app: WeakEntity<AppStore>,
    pub connection: ConnectionId,
}

pub struct WebAppModel {
    exec: ExecClient,
    tab_id: String,
    port: Option<u16>,
    local_port: Option<u16>,
    remote: Option<RemoteHost>,
    phase: WebAppPhase,
    _run: Option<Task<()>>,
}

/// curl's exit code for `http://localhost:<port>/` on the daemon's host.
async fn probe(exec: &ExecClient, port: u16) -> Result<i32, String> {
    let request = ExecRequest {
        cwd: "/".to_owned(),
        command: "curl".to_owned(),
        args: ["-s", "-o", "/dev/null", "--connect-timeout", "1", "--max-time", "2"]
            .iter()
            .map(|a| (*a).to_owned())
            .chain([format!("http://localhost:{port}/")])
            .collect(),
        env: HashMap::new(),
        timeout: ExecTimeout::After(Duration::from_secs(10)),
    };
    exec.run(request).await.map(|output| output.exit_code).map_err(|e| format!("curl: {e}"))
}

impl WebAppModel {
    pub fn new(
        exec: ExecClient,
        tab_id: String,
        port: Option<u16>,
        local_port: Option<u16>,
        remote: Option<RemoteHost>,
        cx: &mut Context<Self>,
    ) -> Self {
        let mut model = Self {
            exec,
            tab_id,
            port,
            local_port,
            remote,
            phase: WebAppPhase::AllocatingPort,
            _run: None,
        };
        model.start(cx);
        model
    }

    pub fn phase(&self) -> &WebAppPhase {
        &self.phase
    }

    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// Run the start-up again (after an error).
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        self.start(cx);
    }

    /// The tab closed: drop its forward.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        self._run = None;
        if let Some(remote) = &self.remote
            && let Some(app) = remote.app.upgrade()
        {
            app.read(cx).manager().remove_port_forward(&forward_id_for_tab(&self.tab_id));
        }
    }

    fn set_phase(&mut self, phase: WebAppPhase, cx: &mut Context<Self>) {
        self.phase = phase;
        cx.notify();
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.phase = match self.port {
            Some(port) => WebAppPhase::WaitingForServer { port },
            None => WebAppPhase::AllocatingPort,
        };
        let (exec, known_port, remote, tab_id, preferred) =
            (self.exec.clone(), self.port, self.remote.clone(), self.tab_id.clone(), self.local_port);
        let executor = cx.background_executor().clone();
        self._run = Some(cx.spawn(async move |this, cx| {
            let result: Result<(), String> = async {
                let port = match known_port {
                    Some(port) => port,
                    None => {
                        let mut found = None;
                        for _ in 0..FIND_PORT_ATTEMPTS {
                            let port = random_port();
                            if probe(&exec, port).await? == CURL_COULDNT_CONNECT {
                                found = Some(port);
                                break;
                            }
                        }
                        let port = found.ok_or_else(|| format!("No free port found after {FIND_PORT_ATTEMPTS} attempts"))?;
                        this.update(cx, |model, cx| {
                            model.port = Some(port);
                            cx.emit(WebAppEvent::PortAssigned(port));
                            model.set_phase(WebAppPhase::WaitingForServer { port }, cx);
                        })
                        .map_err(|_| "closed".to_owned())?;
                        port
                    }
                };
                // Any HTTP answer will do (curl exits 0 whatever the status).
                while probe(&exec, port).await? != 0 {
                    executor.timer(POLL_INTERVAL).await;
                }
                let url = match &remote {
                    None => format!("http://localhost:{port}/"),
                    Some(remote) => {
                        this.update(cx, |model, cx| model.set_phase(WebAppPhase::Forwarding { port }, cx))
                            .map_err(|_| "closed".to_owned())?;
                        let local = ensure_forward(remote, &tab_id, port, preferred, &executor, cx).await?;
                        this.update(cx, |model, cx| {
                            model.local_port = Some(local);
                            cx.emit(WebAppEvent::LocalPortAssigned(local));
                        })
                        .map_err(|_| "closed".to_owned())?;
                        format!("http://localhost:{local}/")
                    }
                };
                this.update(cx, |model, cx| model.set_phase(WebAppPhase::Ready { url }, cx))
                    .map_err(|_| "closed".to_owned())
            }
            .await;
            if let Err(message) = result {
                match this.update(cx, |model, cx| model.set_phase(WebAppPhase::Error(message), cx)) {
                    Ok(()) | Err(_) => {} // Err: the tab closed
                }
            }
        }));
    }
}

/// The forward's status once it leaves Connecting; `None` when it does not exist.
async fn settled_forward(
    remote: &RemoteHost,
    id: &PortForwardId,
    executor: &gpui_kit::BackgroundExecutor,
    cx: &mut AsyncApp,
) -> Result<Option<(u16, PortForwardStatus)>, String> {
    loop {
        let found = remote
            .app
            .read_with(cx, |app, _| {
                app.manager()
                    .list_port_forwards(&remote.connection)
                    .into_iter()
                    .find(|f| &f.config.id == id)
                    .map(|f| (f.config.spec.local_port, f.status))
            })
            .map_err(|_| "the session went away".to_owned())?;
        match found {
            Some((_, PortForwardStatus::Connecting)) => executor.timer(FORWARD_POLL).await,
            other => return Ok(other),
        }
    }
}

/// An active forward from a local port to `localhost:<remote_port>` on the ssh host,
/// reusing a live one (`ensureForward`). Returns the local port.
async fn ensure_forward(
    remote: &RemoteHost,
    tab_id: &str,
    remote_port: u16,
    preferred: Option<u16>,
    executor: &gpui_kit::BackgroundExecutor,
    cx: &mut AsyncApp,
) -> Result<u16, String> {
    let id = forward_id_for_tab(tab_id);
    let remove = |cx: &mut AsyncApp| {
        remote
            .app
            .read_with(cx, |app, _| app.manager().remove_port_forward(&id))
            .map_err(|_| "the session went away".to_owned())
    };
    match settled_forward(remote, &id, executor, cx).await? {
        Some((local, PortForwardStatus::Active)) => return Ok(local),
        Some(_) => remove(cx)?,
        None => {}
    }
    for attempt in 0..FORWARD_ATTEMPTS {
        let local = match (attempt, preferred) {
            (0, Some(port)) => port,
            _ => random_port(),
        };
        let config = PortForwardConfig {
            id: id.clone(),
            connection_id: remote.connection.clone(),
            spec: PortForwardSpec {
                local_port: local,
                remote_host: "localhost".to_owned(),
                remote_port,
                extra: Default::default(),
            },
            persistence: Persistence::Ephemeral,
        };
        remote
            .app
            .read_with(cx, |app, _| app.manager().add_port_forward(config))
            .map_err(|_| "the session went away".to_owned())?
            .map_err(|e| e.to_string())?;
        if let Some((_, PortForwardStatus::Active)) = settled_forward(remote, &id, executor, cx).await? {
            return Ok(local);
        }
        // Most likely the local port is taken: try another.
        remove(cx)?;
    }
    Err(format!("Could not forward port {remote_port} after {FORWARD_ATTEMPTS} attempts"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn port_placeholders_are_replaced_everywhere() {
        assert_eq!(resolve_command("vite --port $PORT --strictPort # $PORT", 4321), "vite --port 4321 --strictPort # 4321");
        assert_eq!(resolve_command("npm start", 1), "npm start");
    }

    #[test]
    fn random_ports_stay_in_range() {
        assert_eq!(port_from(0), 20000);
        assert_eq!(port_from(24999), 44999);
        assert_eq!(port_from(25000), 20000);
        assert_eq!(forward_id_for_tab("tab-1").as_str(), "webapp-tab-1");
    }
}
