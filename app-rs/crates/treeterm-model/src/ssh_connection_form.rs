//! The SSH connection picker's form (`ConnectionPicker.tsx`) apart from its text inputs: what
//! a Connect from the form saves, the port forwards it carries, the daemon options and the
//! error. The picker stays mounted in Electron, so all of it outlives closing the dialog.

use treeterm_core::connection::OutdatedDaemon;
use treeterm_core::settings::{ConnectionLabel, IdentityFile, PortForwardSpec, SshConnectionConfig};

use crate::session_info::parse_forward_fields;

/// The form's text fields, as typed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionFields {
    pub host: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
    pub label: String,
}

impl Default for ConnectionFields {
    /// The form as it first opens, and as a Connect from it leaves it: port 22, the rest
    /// empty.
    fn default() -> Self {
        Self {
            host: String::new(),
            user: String::new(),
            port: "22".to_owned(),
            identity_file: String::new(),
            label: String::new(),
        }
    }
}

impl ConnectionFields {
    /// "Fill": a saved connection as the form shows it; no identity file or label is empty.
    pub fn of_saved(config: &SshConnectionConfig) -> Self {
        Self {
            host: config.host.clone(),
            user: config.user.clone(),
            port: config.port.to_string(),
            identity_file: match &config.identity_file {
                IdentityFile::SshDefault => String::new(),
                IdentityFile::Path(path) => path.clone(),
            },
            label: match &config.label {
                ConnectionLabel::UserAtHost => String::new(),
                ConnectionLabel::Custom(label) => label.clone(),
            },
        }
    }
}

/// A connection from the form (`handleConnect`): host and user required, port defaults to
/// 22, an empty label is saved as `user@host`, the id is new.
pub fn build_config(
    fields: &ConnectionFields,
    port_forwards: Vec<PortForwardSpec>,
    now_ms: u128,
) -> Result<SshConnectionConfig, String> {
    let (host, user) = (fields.host.trim(), fields.user.trim());
    if host.is_empty() || user.is_empty() {
        return Err("Host and user are required".to_owned());
    }
    let port = match fields.port.trim() {
        "" => 22,
        text => text.parse::<u16>().ok().filter(|p| *p > 0).ok_or_else(|| format!("Invalid port “{text}”"))?,
    };
    let identity = fields.identity_file.trim();
    let label = match fields.label.trim() {
        "" => format!("{user}@{host}"),
        label => label.to_owned(),
    };
    Ok(SshConnectionConfig {
        id: format!("ssh-{host}-{now_ms}"),
        host: host.to_owned(),
        user: user.to_owned(),
        port,
        identity_file: if identity.is_empty() { IdentityFile::SshDefault } else { IdentityFile::Path(identity.to_owned()) },
        label: ConnectionLabel::Custom(label),
        port_forwards,
        extra: Default::default(),
    })
}

/// The picker's state besides its text inputs.
#[derive(Debug, Default)]
pub struct ConnectionForm {
    /// The forwards the next Connect from the form carries.
    port_forwards: Vec<PortForwardSpec>,
    /// "Refresh remote daemon".
    refresh_daemon: bool,
    /// "Allow outdated daemon".
    allow_outdated: bool,
    /// Why the last Connect from the form did not go ahead.
    error: Option<String>,
}

impl ConnectionForm {
    pub fn port_forwards(&self) -> &[PortForwardSpec] {
        &self.port_forwards
    }

    pub fn refresh_daemon(&self) -> bool {
        self.refresh_daemon
    }

    pub fn allow_outdated(&self) -> bool {
        self.allow_outdated
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// `ssh.connect`'s daemon options: Refresh wins over Allow outdated.
    pub fn outdated(&self) -> OutdatedDaemon {
        match (self.refresh_daemon, self.allow_outdated) {
            (true, _) => OutdatedDaemon::Refresh,
            (false, true) => OutdatedDaemon::Accept,
            (false, false) => OutdatedDaemon::Report,
        }
    }

    pub fn toggle_refresh_daemon(&mut self) {
        self.refresh_daemon = !self.refresh_daemon;
    }

    /// Ignored while "Refresh remote daemon" is checked, which disables the checkbox (it
    /// keeps its value).
    pub fn toggle_allow_outdated(&mut self) {
        if !self.refresh_daemon {
            self.allow_outdated = !self.allow_outdated;
        }
    }

    /// `handleConnect`: the connection to save and connect to, or `None` with the error to
    /// show. A Connect clears the error and the port forwards (the view resets the fields to
    /// [`ConnectionFields::default`]); the forward inputs and both options stay.
    pub fn submit(&mut self, fields: &ConnectionFields, now_ms: u128) -> Option<SshConnectionConfig> {
        match build_config(fields, self.port_forwards.clone(), now_ms) {
            Ok(config) => {
                self.error = None;
                self.port_forwards.clear();
                Some(config)
            }
            Err(error) => {
                self.error = Some(error);
                None
            }
        }
    }

    /// `handleConnectSaved`: connecting a saved connection clears the error.
    pub fn connect_saved(&mut self) {
        self.error = None;
    }

    /// "Fill": the saved connection's forwards replace the form's; returns its fields for
    /// the inputs.
    pub fn fill(&mut self, config: &SshConnectionConfig) -> ConnectionFields {
        self.port_forwards = config.port_forwards.clone();
        ConnectionFields::of_saved(config)
    }

    /// "Add": the forward the three inputs make, if they make one (ports read like
    /// `parseInt`, 1–65535, and a remote host); anything else is ignored without a message,
    /// as in Electron. Returns whether it was added, so the view resets the inputs.
    pub fn add_forward(&mut self, local: &str, remote_host: &str, remote_port: &str) -> bool {
        match parse_forward_fields(local, remote_host, remote_port) {
            Ok(forward) => {
                self.port_forwards.push(forward);
                true
            }
            Err(error) => {
                tracing::debug!(?error, "port forward not added");
                false
            }
        }
    }

    /// A pending forward's "x".
    pub fn remove_forward(&mut self, index: usize) {
        self.port_forwards.remove(index);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(host: &str, user: &str, port: &str) -> ConnectionFields {
        ConnectionFields { host: host.into(), user: user.into(), port: port.into(), ..ConnectionFields::default() }
    }

    fn forward(local: u16, host: &str, remote: u16) -> PortForwardSpec {
        PortForwardSpec { local_port: local, remote_host: host.into(), remote_port: remote, extra: Default::default() }
    }

    #[test]
    fn host_and_user_are_required() {
        assert_eq!(build_config(&fields("", "me", "22"), vec![], 1).unwrap_err(), "Host and user are required");
        assert_eq!(build_config(&fields("box", " ", "22"), vec![], 1).unwrap_err(), "Host and user are required");
    }

    #[test]
    fn a_new_connection_gets_defaults_and_a_fresh_id() {
        let config = build_config(&fields(" box.lan ", "me", ""), vec![], 42).unwrap();
        assert_eq!(config.id, "ssh-box.lan-42");
        assert_eq!(config.port, 22);
        assert_eq!(config.identity_file, IdentityFile::SshDefault);
        assert_eq!(config.display_name(), "me@box.lan");
        assert_eq!(build_config(&fields("box", "me", "70000"), vec![], 1).unwrap_err(), "Invalid port “70000”");
        assert!(build_config(&fields("box", "me", "0"), vec![], 1).is_err());
        let typed = ConnectionFields { identity_file: " ~/.ssh/k ".into(), label: " Box ".into(), ..fields("box", "me", "2222") };
        let config = build_config(&typed, vec![], 1).unwrap();
        assert_eq!((config.port, config.identity_file, config.label), (
            2222,
            IdentityFile::Path("~/.ssh/k".into()),
            ConnectionLabel::Custom("Box".into())
        ));
    }

    /// `label || `${user}@${host}``: settings.json gets the label, so Fill shows it.
    #[test]
    fn an_empty_label_is_saved_as_user_at_host() {
        let config = build_config(&fields("box.lan", "me", "22"), vec![], 1).unwrap();
        assert_eq!(config.label, ConnectionLabel::Custom("me@box.lan".into()));
        assert_eq!(serde_json::to_value(&config).unwrap()["label"], "me@box.lan");
        assert_eq!(ConnectionFields::of_saved(&config).label, "me@box.lan");
    }

    #[test]
    fn fill_copies_a_saved_connection_and_its_forwards() {
        let mut form = ConnectionForm::default();
        assert!(form.add_forward("1", "h", "2"));
        let saved = SshConnectionConfig {
            identity_file: IdentityFile::Path("~/.ssh/k".into()),
            label: ConnectionLabel::UserAtHost,
            port_forwards: vec![forward(8080, "localhost", 3000)],
            ..build_config(&fields("box", "me", "2222"), vec![], 1).unwrap()
        };
        let filled = form.fill(&saved);
        assert_eq!(filled, ConnectionFields {
            host: "box".into(),
            user: "me".into(),
            port: "2222".into(),
            identity_file: "~/.ssh/k".into(),
            label: String::new(),
        });
        assert_eq!(form.port_forwards(), [forward(8080, "localhost", 3000)]);
    }

    /// `handleConnect` clears the error and the forwards; a failed one keeps the forwards.
    #[test]
    fn a_connect_from_the_form_clears_the_error_and_forwards_and_keeps_the_options() {
        let mut form = ConnectionForm::default();
        assert!(form.add_forward("8080", "localhost", "3000"));
        form.toggle_refresh_daemon();

        assert_eq!(form.submit(&fields("box", "", "22"), 1), None);
        assert_eq!(form.error(), Some("Host and user are required"));
        assert_eq!(form.port_forwards().len(), 1);

        let config = form.submit(&fields("box", "me", "22"), 7).expect("connects");
        assert_eq!(config.port_forwards, [forward(8080, "localhost", 3000)]);
        assert_eq!(form.error(), None);
        assert!(form.port_forwards().is_empty());
        assert!(form.refresh_daemon());
    }

    #[test]
    fn connecting_a_saved_connection_clears_the_error() {
        let mut form = ConnectionForm::default();
        assert_eq!(form.submit(&fields("", "", ""), 1), None);
        form.connect_saved();
        assert_eq!(form.error(), None);
    }

    /// Ports read like `parseInt`: leading digits, 1–65535; a blank host is ignored.
    #[test]
    fn add_forward_takes_valid_input_and_ignores_the_rest() {
        let mut form = ConnectionForm::default();
        assert!(form.add_forward("80abc", "localhost", "8.5"));
        assert!(form.add_forward(" 8080", " db ", "5432"));
        for (local, host, remote) in [("0", "h", "1"), ("1", "h", "65536"), ("8080", " ", "3000"), ("x", "h", "1")] {
            assert!(!form.add_forward(local, host, remote), "{local} {host} {remote}");
        }
        assert_eq!(form.port_forwards(), [forward(80, "localhost", 8), forward(8080, "db", 5432)]);
        form.remove_forward(0);
        assert_eq!(form.port_forwards(), [forward(8080, "db", 5432)]);
    }

    #[test]
    fn refresh_wins_and_disables_allow_outdated() {
        let mut form = ConnectionForm::default();
        assert_eq!(form.outdated(), OutdatedDaemon::Report);
        form.toggle_allow_outdated();
        assert_eq!(form.outdated(), OutdatedDaemon::Accept);
        form.toggle_refresh_daemon();
        assert_eq!(form.outdated(), OutdatedDaemon::Refresh);
        // Disabled: the click does nothing, and the box keeps its tick.
        form.toggle_allow_outdated();
        assert!(form.allow_outdated());
        form.toggle_refresh_daemon();
        assert_eq!(form.outdated(), OutdatedDaemon::Accept);
    }
}
