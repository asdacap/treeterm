//! What the session info pane shows for one connection: its bootstrap, tunnel and daemon
//! logs, and its port forwards. Port of the data side of `SessionInfoPane.tsx`.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{App, Context, Entity, Task, WeakEntity};
use treeterm_core::connection::{
    ConnectPhase, ConnectionId, ConnectionTarget, LogEvent, OutputKind, Persistence, PortForwardConfig,
    PortForwardId, PortForwardInfo,
};
use treeterm_core::settings::PortForwardSpec;

use crate::app_store::{AppStore, ConnState};

/// Lines kept per log.
const MAX_LINES: usize = 2000;
/// Port forward states are re-read this often while the pane is open.
const FORWARD_POLL: Duration = Duration::from_millis(500);

pub struct SessionInfo {
    id: ConnectionId,
    app: WeakEntity<AppStore>,
    logs: HashMap<OutputKind, Vec<String>>,
    /// Follows each log of the connection's current manager entry ([`Self::follow`]).
    log_tasks: HashMap<OutputKind, Task<()>>,
    forwards: Vec<PortForwardInfo>,
    error: Option<String>,
    /// Forwards whose output is shown, with what it printed and the task following it.
    forward_logs: HashMap<PortForwardId, (Vec<String>, Option<Task<()>>)>,
    _poll: Task<()>,
}

/// Why the "Add Port Forward" dialog refuses its fields (`PortForwardDialog`'s messages).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForwardFieldError {
    LocalPort,
    RemoteHost,
    RemotePort,
}

impl ForwardFieldError {
    pub fn message(self) -> &'static str {
        match self {
            ForwardFieldError::LocalPort => "Local port must be a number between 1 and 65535",
            ForwardFieldError::RemoteHost => "Remote host is required",
            ForwardFieldError::RemotePort => "Remote port must be a number between 1 and 65535",
        }
    }
}

/// The dialog's fields as a forward, checked in Electron's order. Ports read like
/// `parseInt`: leading digits, the rest ignored.
pub fn parse_forward_fields(local: &str, host: &str, remote: &str) -> Result<PortForwardSpec, ForwardFieldError> {
    let port = |text: &str| -> Option<u16> {
        let digits: String = text.trim().chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok().filter(|p| (1..=65535).contains(p)).and_then(|p| u16::try_from(p).ok())
    };
    let local_port = port(local).ok_or(ForwardFieldError::LocalPort)?;
    let host = host.trim();
    if host.is_empty() {
        return Err(ForwardFieldError::RemoteHost);
    }
    let remote_port = port(remote).ok_or(ForwardFieldError::RemotePort)?;
    Ok(PortForwardSpec { local_port, remote_host: host.to_owned(), remote_port, extra: Default::default() })
}

/// A connection as Electron's `ConnectionInfo` (`Connection.toInfo`), keys in its order.
pub fn connection_json(id: &ConnectionId, target: &ConnectionTarget, state: &ConnState) -> serde_json::Value {
    let id = id.to_string();
    let target = match target {
        ConnectionTarget::Local => serde_json::json!({ "type": "local" }),
        ConnectionTarget::Remote(config) => serde_json::json!({ "type": "remote", "config": &**config }),
    };
    match state {
        ConnState::Connecting(phase) => {
            let phase = match phase {
                ConnectPhase::Bootstrap => "bootstrap",
                ConnectPhase::Tunnel => "tunnel",
                ConnectPhase::Daemon => "daemon",
            };
            serde_json::json!({ "id": id, "target": target, "status": "connecting", "connectPhase": phase })
        }
        ConnState::Connected(_) => serde_json::json!({ "id": id, "target": target, "status": "connected" }),
        ConnState::Reconnecting { attempt, error } => serde_json::json!({
            "id": id, "target": target, "status": "reconnecting", "error": error, "attempt": attempt
        }),
        ConnState::Failed { error, hash_mismatch } => {
            let kind = if *hash_mismatch { "daemon-hash-mismatch" } else { "generic" };
            serde_json::json!({ "id": id, "target": target, "status": "error", "error": error, "errorKind": kind })
        }
    }
}

impl SessionInfo {
    pub fn new(app: &Entity<AppStore>, id: ConnectionId, cx: &mut Context<Self>) -> Self {
        let executor = cx.background_executor().clone();
        let poll = cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |info, cx| info.refresh_forwards(cx)).is_err() {
                    return;
                }
                executor.timer(FORWARD_POLL).await;
            }
        });
        let mut info = Self {
            id,
            app: app.downgrade(),
            logs: HashMap::new(),
            log_tasks: HashMap::new(),
            forwards: Vec::new(),
            error: None,
            forward_logs: HashMap::new(),
            _poll: poll,
        };
        for kind in [OutputKind::Bootstrap, OutputKind::Tunnel, OutputKind::Daemon] {
            info.follow(kind, cx);
        }
        info
    }

    /// Show the log's scrollback on the connection as it is now, and follow it: its new
    /// lines, and a new tunnel starting it over. A connect that replaces the connection
    /// (Retry, Refresh daemon, Ignore mismatch) ends the stream, and the replacing
    /// connection's log is followed instead, as Electron re-subscribes on every connection
    /// change. No connection: an empty log, as Electron's empty scrollback.
    fn follow(&mut self, kind: OutputKind, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let watch = app.read(cx).manager().watch_output(&self.id, kind);
        match watch {
            Ok(watch) => {
                self.logs.insert(kind, watch.initial);
                let mut updates = watch.updates;
                let task = cx.spawn(async move |this, cx| {
                    while let Some(event) = updates.next().await {
                        if this.update(cx, |info, cx| info.on_log(kind, event, cx)).is_err() {
                            return;
                        }
                    }
                    match this.update(cx, |info, cx| info.follow(kind, cx)) {
                        Ok(()) | Err(_) => {} // Err: the pane closed
                    }
                });
                self.log_tasks.insert(kind, task);
            }
            Err(error) => {
                tracing::debug!(%error, "no log for this connection");
                self.logs.remove(&kind);
                self.log_tasks.remove(&kind);
            }
        }
        cx.notify();
    }

    pub fn id(&self) -> &ConnectionId {
        &self.id
    }

    /// The Info tab: session id, how the connection is made, and its status.
    pub fn info_lines(&self, cx: &App) -> Vec<String> {
        let Some(app) = self.app.upgrade() else {
            return Vec::new();
        };
        let Some(slot) = app.read(cx).session_slot(&self.id) else {
            return vec!["Disconnected".to_owned()];
        };
        let mut lines = Vec::new();
        if let Some(session) = slot.session() {
            lines.push(format!("Session ID: {}", session.read(cx).session_id()));
        }
        match &slot.target {
            ConnectionTarget::Local => lines.push("Connection: Local".to_owned()),
            ConnectionTarget::Remote(config) => {
                lines.push("Connection: SSH".to_owned());
                lines.push(format!("Host: {}", config.host));
                lines.push(format!("User: {}", config.user));
                lines.push(format!("Port: {}", config.port));
                match &config.identity_file {
                    treeterm_core::settings::IdentityFile::Path(path) => lines.push(format!("Identity File: {path}")),
                    treeterm_core::settings::IdentityFile::SshDefault => {}
                }
                // The error only once the connection stopped (Electron: Error or Disconnected).
                let (status, error) = match &slot.state {
                    ConnState::Connecting(_) => ("connecting", None),
                    ConnState::Connected(_) => ("connected", None),
                    ConnState::Reconnecting { .. } => ("reconnecting", None),
                    ConnState::Failed { error, .. } => ("error", Some(error)),
                };
                lines.push(format!("Status: {status}"));
                lines.extend(error.map(|error| format!("Error: {error}")));
            }
        }
        lines
    }

    /// The JSON tab (`sessionData`): the session the tree shows, with its connection as
    /// Electron's `ConnectionInfo`, at any status. While the connection is down, the session
    /// is the one kept from its last generation, as Electron's store outlives them; before
    /// its first session, the connection alone. `Null` once disconnected (the pane closes).
    pub fn debug_json(&self, cx: &App) -> serde_json::Value {
        let Some(app) = self.app.upgrade() else {
            return serde_json::Value::Null;
        };
        let Some(slot) = app.read(cx).session_slot(&self.id) else {
            return serde_json::Value::Null;
        };
        let mut value = match slot.shown_session() {
            Some(session) => session.read(cx).debug_json(cx),
            None => serde_json::json!({}),
        };
        value["connection"] = connection_json(&slot.id, &slot.target, &slot.state);
        value
    }

    /// The session the JSON tab shows ([`Self::debug_json`]), which it follows.
    pub fn session(&self, cx: &App) -> Option<Entity<crate::session_store::SessionStore>> {
        let app = self.app.upgrade()?;
        app.read(cx).session_slot(&self.id)?.shown_session().cloned()
    }

    pub fn log(&self, kind: OutputKind) -> &[String] {
        self.logs.get(&kind).map(Vec::as_slice).unwrap_or_default()
    }

    pub fn forwards(&self) -> &[PortForwardInfo] {
        &self.forwards
    }

    /// Why the last port-forward action failed.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    fn on_log(&mut self, kind: OutputKind, event: LogEvent, cx: &mut Context<Self>) {
        let log = self.logs.entry(kind).or_default();
        match event {
            LogEvent::Line(line) => {
                log.push(line);
                let excess = log.len().saturating_sub(MAX_LINES);
                log.drain(..excess);
            }
            LogEvent::Cleared => log.clear(),
        }
        cx.notify();
    }

    fn refresh_forwards(&mut self, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let forwards = app.read(cx).manager().list_port_forwards(&self.id);
        if forwards != self.forwards {
            self.forwards = forwards;
            cx.notify();
        }
    }

    /// Start a forward; `saved` ones are written to the saved connection and come back on
    /// every connect (`PortForwardDialog` with "persist").
    pub fn add_forward(&mut self, spec: PortForwardSpec, saved: bool, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let config = PortForwardConfig {
            id: PortForwardId::random(),
            connection_id: self.id.clone(),
            spec,
            persistence: if saved { Persistence::Saved } else { Persistence::Ephemeral },
        };
        let result = app.read(cx).manager().add_port_forward(config);
        self.error = match result {
            Ok(_) => None,
            Err(error) => Some(error.to_string()),
        };
        if saved {
            app.update(cx, |store, cx| store.save_port_forwards(&self.id, cx));
        }
        self.refresh_forwards(cx);
        cx.notify();
    }

    pub fn remove_forward(&mut self, forward: &PortForwardId, cx: &mut Context<Self>) {
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let saved = self
            .forwards
            .iter()
            .any(|f| &f.config.id == forward && f.config.persistence == Persistence::Saved);
        app.read(cx).manager().remove_port_forward(forward);
        if saved {
            app.update(cx, |store, cx| store.save_port_forwards(&self.id, cx));
        }
        self.refresh_forwards(cx);
    }

    /// Show or hide a forward's ssh output ("Log" / "Hide"): shown ones follow new lines.
    pub fn toggle_forward_log(&mut self, forward: &PortForwardId, cx: &mut Context<Self>) {
        if self.forward_logs.remove(forward).is_some() {
            cx.notify();
            return;
        }
        let Some(app) = self.app.upgrade() else {
            return;
        };
        let watch = app.read(cx).manager().watch_port_forward_output(forward);
        let (lines, task) = match watch {
            Ok(watch) => {
                let (id, mut updates) = (forward.clone(), watch.updates);
                let task = cx.spawn(async move |this, cx| {
                    while let Some(line) = updates.next().await {
                        let pushed = this.update(cx, |info, cx| {
                            if let Some((log, _)) = info.forward_logs.get_mut(&id) {
                                log.push(line);
                                let excess = log.len().saturating_sub(MAX_LINES);
                                log.drain(..excess);
                                cx.notify();
                            }
                        });
                        if pushed.is_err() {
                            return;
                        }
                    }
                });
                (watch.initial, Some(task))
            }
            Err(error) => (vec![format!("Failed to watch output: {error}")], None),
        };
        self.forward_logs.insert(forward.clone(), (lines, task));
        cx.notify();
    }

    /// The forward's output while shown ([`Self::toggle_forward_log`]).
    pub fn forward_log(&self, forward: &PortForwardId) -> Option<&[String]> {
        self.forward_logs.get(forward).map(|(lines, _)| lines.as_slice())
    }

    /// A failed or stopped forward again, as a new forward with the same spec.
    pub fn restart_forward(&mut self, forward: &PortForwardId, cx: &mut Context<Self>) {
        let Some(old) = self.forwards.iter().find(|f| &f.config.id == forward).cloned() else {
            return;
        };
        if let Some(app) = self.app.upgrade() {
            app.read(cx).manager().remove_port_forward(forward);
        }
        self.add_forward(old.config.spec, old.config.persistence == Persistence::Saved, cx);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use treeterm_core::connection::{ConnectPhase, ConnectionId, ConnectionTarget};

    use super::{ForwardFieldError, connection_json, parse_forward_fields};
    use crate::app_store::ConnState;

    fn keys(value: &serde_json::Value) -> Vec<&str> {
        value.as_object().unwrap().keys().map(String::as_str).collect()
    }

    #[test]
    fn a_connection_reads_as_electrons_connection_info_at_each_status() {
        let (id, local) = (ConnectionId::new("c1"), ConnectionTarget::Local);
        let tunnel = connection_json(&id, &local, &ConnState::Connecting(ConnectPhase::Tunnel));
        assert_eq!(
            tunnel,
            serde_json::json!({
                "id": "c1", "target": { "type": "local" }, "status": "connecting", "connectPhase": "tunnel"
            })
        );
        assert_eq!(keys(&tunnel), ["id", "target", "status", "connectPhase"]);
        for (phase, name) in [(ConnectPhase::Bootstrap, "bootstrap"), (ConnectPhase::Daemon, "daemon")] {
            assert_eq!(connection_json(&id, &local, &ConnState::Connecting(phase))["connectPhase"], name);
        }

        let lost = ConnState::Reconnecting { attempt: 3, error: "Connection lost".to_owned() };
        let reconnecting = connection_json(&id, &local, &lost);
        assert_eq!(keys(&reconnecting), ["id", "target", "status", "error", "attempt"]);
        assert_eq!(
            (&reconnecting["status"], &reconnecting["error"], &reconnecting["attempt"]),
            (&"reconnecting".into(), &"Connection lost".into(), &3.into())
        );

        let failed = |hash_mismatch| ConnState::Failed { error: "refused".to_owned(), hash_mismatch };
        let generic = connection_json(&id, &local, &failed(false));
        assert_eq!(keys(&generic), ["id", "target", "status", "error", "errorKind"]);
        assert_eq!((&generic["status"], &generic["error"]), (&"error".into(), &"refused".into()));
        assert_eq!(generic["errorKind"], "generic");
        assert_eq!(connection_json(&id, &local, &failed(true))["errorKind"], "daemon-hash-mismatch");
    }

    #[test]
    fn a_remote_connection_carries_its_ssh_config() {
        let config = treeterm_testkit::fake_ssh::remote_config();
        let id = ConnectionId::new(config.id.clone());
        let target = ConnectionTarget::Remote(Arc::new(config));
        let json = connection_json(&id, &target, &ConnState::Connecting(ConnectPhase::Bootstrap));
        assert_eq!(json["id"], "remote-1");
        assert_eq!(keys(&json["target"]), ["type", "config"]);
        assert_eq!(json["target"]["type"], "remote");
        let config = &json["target"]["config"];
        assert_eq!((&config["host"], &config["user"]), (&"example.com".into(), &"test".into()));
    }

    #[test]
    fn forward_fields_are_checked_in_order_like_parse_int() {
        let spec = parse_forward_fields(" 8080", " localhost ", "3000").unwrap();
        assert_eq!((spec.local_port, spec.remote_host.as_str(), spec.remote_port), (8080, "localhost", 3000));
        assert_eq!(parse_forward_fields("80x", "h", "9px").map(|s| (s.local_port, s.remote_port)), Ok((80, 9)));
        assert_eq!(parse_forward_fields("", "", ""), Err(ForwardFieldError::LocalPort));
        assert_eq!(parse_forward_fields("0", "h", "1"), Err(ForwardFieldError::LocalPort));
        assert_eq!(parse_forward_fields("8080", "  ", ""), Err(ForwardFieldError::RemoteHost));
        assert_eq!(parse_forward_fields("8080", "h", "65536"), Err(ForwardFieldError::RemotePort));
        assert_eq!(parse_forward_fields("8080", "h", "-1"), Err(ForwardFieldError::RemotePort));
    }
}
