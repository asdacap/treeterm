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
    _tasks: Vec<Task<()>>,
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
