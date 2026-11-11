//! What the session info pane shows for one connection: its bootstrap, tunnel and daemon
//! logs, and its port forwards. Port of the data side of `SessionInfoPane.tsx`.

use std::collections::HashMap;
use std::time::Duration;

use futures::StreamExt as _;
use gpui_kit::{App, Context, Entity, Task, WeakEntity};
use treeterm_core::connection::{
    ConnectionId, ConnectionTarget, OutputKind, Persistence, PortForwardConfig, PortForwardId, PortForwardInfo,
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
    forwards: Vec<PortForwardInfo>,
    error: Option<String>,
    /// Forwards whose output is shown, with what it printed and the task following it.
    forward_logs: HashMap<PortForwardId, (Vec<String>, Option<Task<()>>)>,
    _tasks: Vec<Task<()>>,
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

impl SessionInfo {
    pub fn new(app: &Entity<AppStore>, id: ConnectionId, cx: &mut Context<Self>) -> Self {
        let mut tasks = Vec::new();
        let mut logs = HashMap::new();
        for kind in [OutputKind::Bootstrap, OutputKind::Tunnel, OutputKind::Daemon] {
            match app.read(cx).manager().watch_output(&id, kind) {
                Ok(watch) => {
                    logs.insert(kind, watch.initial);
                    let mut updates = watch.updates;
                    tasks.push(cx.spawn(async move |this, cx| {
                        while let Some(line) = updates.next().await {
                            if this.update(cx, |info, cx| info.push_line(kind, line, cx)).is_err() {
                                return;
                            }
                        }
                    }));
                }
                Err(error) => tracing::debug!(%error, "no log for this connection"),
            }
        }
        let executor = cx.background_executor().clone();
        tasks.push(cx.spawn(async move |this, cx| {
            loop {
                if this.update(cx, |info, cx| info.refresh_forwards(cx)).is_err() {
                    return;
                }
                executor.timer(FORWARD_POLL).await;
            }
        }));
        Self {
            id,
            app: app.downgrade(),
            logs,
            forwards: Vec::new(),
            error: None,
            forward_logs: HashMap::new(),
            _tasks: tasks,
        }
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
                let (status, error) = match &slot.state {
                    ConnState::Connecting(_) => ("connecting", None),
                    ConnState::Connected(_) => ("connected", None),
                    ConnState::Reconnecting { error, .. } => ("reconnecting", Some(error)),
                    ConnState::Failed { error, .. } => ("error", Some(error)),
                };
                lines.push(format!("Status: {status}"));
                lines.extend(error.map(|error| format!("Error: {error}")));
            }
        }
        lines
    }

    /// The JSON tab: the session's state with its connection; `Null` while not connected.
    pub fn debug_json(&self, cx: &App) -> serde_json::Value {
        let Some(app) = self.app.upgrade() else {
            return serde_json::Value::Null;
        };
        let Some(slot) = app.read(cx).session_slot(&self.id) else {
            return serde_json::Value::Null;
        };
        let Some(session) = slot.session() else {
            return serde_json::Value::Null;
        };
        let mut value = session.read(cx).debug_json(cx);
        let connection = match &slot.target {
            ConnectionTarget::Local => serde_json::json!({ "id": self.id.to_string(), "target": { "type": "local" } }),
            ConnectionTarget::Remote(config) => {
                serde_json::json!({ "id": self.id.to_string(), "target": { "type": "remote", "config": &**config } })
            }
        };
        if let Some(object) = value.as_object_mut() {
            object.insert("connection".to_owned(), connection);
        }
        value
    }

    /// The session these logs belong to, while connected (the TTYs tab lists its PTYs).
    pub fn session(&self, cx: &App) -> Option<Entity<crate::session_store::SessionStore>> {
        let app = self.app.upgrade()?;
        app.read(cx).session_slot(&self.id)?.session().cloned()
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

    fn push_line(&mut self, kind: OutputKind, line: String, cx: &mut Context<Self>) {
        let log = self.logs.entry(kind).or_default();
        log.push(line);
        let excess = log.len().saturating_sub(MAX_LINES);
        log.drain(..excess);
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
    use super::{ForwardFieldError, parse_forward_fields};

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
