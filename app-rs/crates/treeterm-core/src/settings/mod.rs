//! `<userData>/settings.json`, shared with the Electron app; mirrors the `Settings` interface
//! (`src/shared/types.ts`) and `src/main/settings.ts`.
//!
//! Both apps read and write the same file while they coexist, so:
//! - [`load`] merges the file over the defaults exactly as Electron's `mergeSettings` does,
//!   including its legacy migrations ([`from_value`] has the rules).
//! - Every object keeps the keys this version does not know in `extra`, and [`save`] writes
//!   them back, so a save from Rust never drops what a newer or older Electron wrote. The
//!   legacy keys the migrations consume (`applications`, `claude`,
//!   `terminalAnalyzer.jevModel`) are not written back: Electron drops them on its first
//!   save too, and keeping them would re-run a migration after its result was edited.
//! - [`save`] writes what `JSON.stringify(settings, null, 2)` writes: camelCase keys,
//!   two-space indent, optional fields omitted rather than `null`.

mod classifier;
mod read;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::ser::Error as _;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::{Map, Value};

pub use classifier::{
    ClassifiedState, ClassifierCriteria, ClassifierProvider, DEFAULT_ANALYZER_SYSTEM_PROMPT,
    DEFAULT_TITLE_SYSTEM_PROMPT, LEGACY_DEFAULT_SYSTEM_PROMPTS,
};
pub use read::{ValidationError, from_value};

/// Keys this version does not know, written back unchanged. Never holds a key the owning
/// struct has a field for.
pub type Extra = Map<String, Value>;

/// The AI harness command shipped as the default (and used by the legacy `claude` migration).
pub const DEFAULT_AI_HARNESS_COMMAND: &str = "npx @earendil-works/pi-coding-agent";

/// How many entries `recentDirectories` keeps.
pub const MAX_RECENT_DIRECTORIES: usize = 10;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub terminal: TerminalSettings,
    pub sandbox: SandboxSettings,
    pub ai_harness: Instances<AiHarnessInstance>,
    pub custom_runner: Instances<CustomRunnerInstance>,
    pub web_app: Instances<WebAppInstance>,
    pub browser: Instances<BrowserInstance>,
    pub notifications: NotificationSettings,
    pub appearance: AppearanceSettings,
    pub prefix_mode: PrefixModeConfig,
    pub keybindings: Keybindings,
    pub daemon: DaemonSettings,
    pub ssh: SshSettings,
    pub llm: LlmSettings,
    pub terminal_analyzer: TerminalAnalyzerSettings,
    pub github: GithubSettings,
    /// The application new worktrees open with unless a workspace overrides it.
    pub global_default_application_id: String,
    /// Most recent first, at most [`MAX_RECENT_DIRECTORIES`].
    pub recent_directories: Vec<String>,
    pub debug: DebugSettings,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Settings {
    /// Electron's `defaultSettings`: what a missing file loads as.
    fn default() -> Self {
        Self {
            terminal: TerminalSettings::default(),
            sandbox: SandboxSettings::default(),
            ai_harness: Instances {
                instances: vec![AiHarnessInstance {
                    id: "claude".to_owned(),
                    name: "Claude".to_owned(),
                    icon: "✦".to_owned(),
                    command: DEFAULT_AI_HARNESS_COMMAND.to_owned(),
                    is_default: false,
                    enable_sandbox: false,
                    allow_network: true,
                    background_color: "#1a1a24".to_owned(),
                    disable_scrollbar: false,
                    keep_on_exit: false,
                    extra: Extra::new(),
                }],
                extra: Extra::new(),
            },
            custom_runner: Instances::default(),
            web_app: Instances::default(),
            browser: Instances::default(),
            notifications: NotificationSettings::default(),
            appearance: AppearanceSettings::default(),
            prefix_mode: PrefixModeConfig::default(),
            keybindings: Keybindings::default(),
            daemon: DaemonSettings::default(),
            ssh: SshSettings::default(),
            llm: LlmSettings::default(),
            terminal_analyzer: TerminalAnalyzerSettings::default(),
            github: GithubSettings::default(),
            global_default_application_id: "terminal".to_owned(),
            recent_directories: Vec::new(),
            debug: DebugSettings::default(),
            extra: Extra::new(),
        }
    }
}

impl Settings {
    /// Move `dir` to the front of the recent directories (`addRecentDirectory`).
    pub fn add_recent_directory(&mut self, dir: &str) {
        self.recent_directories.retain(|existing| existing != dir);
        self.recent_directories.insert(0, dir.to_owned());
        self.recent_directories.truncate(MAX_RECENT_DIRECTORIES);
    }

    /// Save an SSH connection, replacing the saved one for the same `user@host:port`
    /// (Electron's `ssh:saveConnection` handler).
    pub fn save_ssh_connection(&mut self, config: SshConnectionConfig) {
        let saved = &mut self.ssh.saved_connections;
        match saved
            .iter_mut()
            .find(|c| c.host == config.host && c.user == config.user && c.port == config.port)
        {
            Some(existing) => *existing = config,
            None => saved.push(config),
        }
    }

    /// Electron's `ssh:removeSavedConnection` handler.
    pub fn remove_ssh_connection(&mut self, id: &str) {
        self.ssh.saved_connections.retain(|c| c.id != id);
    }
}

// === terminal ===============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorStyle {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalSettings {
    /// Positive and finite.
    #[serde(serialize_with = "js_number_f32")]
    pub font_size: f32,
    /// CSS font-family list, e.g. `Menlo, Monaco, Consolas, monospace`.
    pub font_family: String,
    pub cursor_style: CursorStyle,
    pub cursor_blink: bool,
    pub show_raw_chars: bool,
    pub allow_osc52_clipboard: bool,
    /// Columns a tab fits to unless its width limit is switched off in the status bar. At least 2.
    pub max_cols: u16,
    pub instances: Vec<TerminalInstance>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for TerminalSettings {
    fn default() -> Self {
        Self {
            font_size: 14.0,
            font_family: "Menlo, Monaco, Consolas, monospace".to_owned(),
            cursor_style: CursorStyle::Block,
            cursor_blink: true,
            show_raw_chars: false,
            allow_osc52_clipboard: true,
            max_cols: 160,
            instances: Vec::new(),
            extra: Extra::new(),
        }
    }
}

impl TerminalSettings {
    /// The font families to try in order, without CSS quoting or generic families that
    /// mean nothing outside a browser.
    pub fn font_families(&self) -> Vec<String> {
        self.font_family
            .split(',')
            .map(|name| name.trim().trim_matches(['"', '\'']).to_owned())
            .filter(|name| !name.is_empty() && !matches!(name.as_str(), "monospace" | "ui-monospace"))
            .collect()
    }
}

/// A terminal variant with its own startup command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalInstance {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub startup_command: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

// === sandbox ================================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSettings {
    pub enabled_by_default: bool,
    pub allow_network_by_default: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for SandboxSettings {
    fn default() -> Self {
        Self { enabled_by_default: false, allow_network_by_default: true, extra: Extra::new() }
    }
}

// === application instances ==================================================================

/// A settings section holding only a list of application variants (`aiHarness`,
/// `customRunner`, `webApp`, `browser`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Instances<T> {
    pub instances: Vec<T>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl<T> Default for Instances<T> {
    fn default() -> Self {
        Self { instances: Vec::new(), extra: Extra::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiHarnessInstance {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub command: String,
    pub is_default: bool,
    pub enable_sandbox: bool,
    pub allow_network: bool,
    pub background_color: String,
    pub disable_scrollbar: bool,
    /// Keep the tab open (showing the exit code) after the harness exits instead of closing it.
    pub keep_on_exit: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CustomRunnerInstance {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// e.g. `rider {{workspace_path}}`.
    pub command_template: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebAppInstance {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// e.g. `npm run dev -- -p $PORT`; `$PORT` is replaced with a free port.
    pub command: String,
    pub is_default: bool,
    pub keep_on_exit: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserInstance {
    pub id: String,
    pub name: String,
    pub icon: String,
    /// Must be http(s) on localhost or 127.0.0.1; forwarded over SSH on remote sessions.
    pub url: String,
    pub is_default: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

// === notifications, appearance ==============================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotificationSettings {
    pub sound_enabled: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self { sound_enabled: true, extra: Extra::new() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Dark,
    Light,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppearanceSettings {
    pub theme: Theme,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self { theme: Theme::Dark, extra: Extra::new() }
    }
}

// === prefix mode, keybindings ===============================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefixModeConfig {
    pub enabled: bool,
    /// e.g. `Control+B`.
    pub prefix_key: String,
    /// How long prefix mode waits for the next key (`timeout`, in ms on disk).
    #[serde(serialize_with = "millis")]
    pub timeout: Duration,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for PrefixModeConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            prefix_key: "Control+B".to_owned(),
            timeout: Duration::from_millis(1500),
            extra: Extra::new(),
        }
    }
}

/// The key pressed after the prefix for each action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Keybindings {
    pub new_tab: String,
    pub close_tab: String,
    pub next_tab: String,
    pub prev_tab: String,
    pub open_settings: String,
    pub workspace_focus: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for Keybindings {
    fn default() -> Self {
        Self {
            new_tab: "c".to_owned(),
            close_tab: "x".to_owned(),
            next_tab: "n".to_owned(),
            prev_tab: "p".to_owned(),
            open_settings: ",".to_owned(),
            workspace_focus: "w".to_owned(),
            extra: Extra::new(),
        }
    }
}

// === daemon =================================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DaemonSettings {
    /// Bytes.
    pub merge_threshold: u64,
    /// Bytes.
    pub compacted_limit: u64,
    pub scrollback_lines: u32,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for DaemonSettings {
    fn default() -> Self {
        Self {
            merge_threshold: 50 * 1024,
            compacted_limit: 1024 * 1024,
            scrollback_lines: 10000,
            extra: Extra::new(),
        }
    }
}

// === ssh ====================================================================================

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshSettings {
    pub saved_connections: Vec<SshConnectionConfig>,
    #[serde(flatten)]
    pub extra: Extra,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SshConnectionConfig {
    pub id: String,
    pub host: String,
    pub user: String,
    pub port: u16,
    #[serde(skip_serializing_if = "IdentityFile::is_ssh_default")]
    pub identity_file: IdentityFile,
    #[serde(skip_serializing_if = "ConnectionLabel::is_user_at_host")]
    pub label: ConnectionLabel,
    pub port_forwards: Vec<PortForwardSpec>,
    #[serde(flatten)]
    pub extra: Extra,
}

impl SshConnectionConfig {
    /// The label, or `user@host` without one.
    pub fn display_name(&self) -> String {
        match &self.label {
            ConnectionLabel::Custom(label) => label.clone(),
            ConnectionLabel::UserAtHost => format!("{}@{}", self.user, self.host),
        }
    }
}

/// `identityFile`, optional on disk; Electron treats an empty one as absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IdentityFile {
    /// No `-i`: ssh picks its own keys.
    SshDefault,
    Path(String),
}

impl IdentityFile {
    fn is_ssh_default(&self) -> bool {
        matches!(self, IdentityFile::SshDefault)
    }
}

impl Serialize for IdentityFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Skipped by `SshConnectionConfig`; empty is what Electron reads as absent.
            IdentityFile::SshDefault => serializer.serialize_str(""),
            IdentityFile::Path(path) => serializer.serialize_str(path),
        }
    }
}

/// `label`, optional on disk; Electron treats an empty one as absent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionLabel {
    /// Shown as `user@host`.
    UserAtHost,
    Custom(String),
}

impl ConnectionLabel {
    fn is_user_at_host(&self) -> bool {
        matches!(self, ConnectionLabel::UserAtHost)
    }
}

impl Serialize for ConnectionLabel {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            // Skipped by `SshConnectionConfig`; empty is what Electron reads as absent.
            ConnectionLabel::UserAtHost => serializer.serialize_str(""),
            ConnectionLabel::Custom(label) => serializer.serialize_str(label),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PortForwardSpec {
    pub local_port: u16,
    pub remote_host: String,
    pub remote_port: u16,
    #[serde(flatten)]
    pub extra: Extra,
}

// === llm, terminal analyzer =================================================================

/// The OpenAI-compatible endpoint used for titles and chat-completion classification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LlmSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: String::new(),
            model: "gpt-4o".to_owned(),
            extra: Extra::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    Off,
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalAnalyzerSettings {
    pub provider: ClassifierProvider,
    /// Chat-completion model for titles, descriptions and branch names.
    pub title_model: String,
    /// Model that classifies terminal output.
    pub model: String,
    /// Context and policy only (supports `{{cwd}}` and `{{safe_paths}}`); the state list and
    /// output format are generated from `criteria`.
    pub system_prompt: String,
    pub criteria: ClassifierCriteria,
    /// System prompt for generating workspace titles from terminal output.
    pub title_system_prompt: String,
    pub reasoning_effort: ReasoningEffort,
    /// Paths considered safe for `safe_permission_requested`.
    pub safe_paths: Vec<String>,
    /// Lines read from the terminal buffer.
    pub buffer_lines: u32,
    /// Quiet output before a tab is idle / classified.
    #[serde(rename = "idleDebounceMs", serialize_with = "millis")]
    pub idle_debounce: Duration,
    /// Idle debounce while the workspace already carries an unread marker.
    #[serde(rename = "idleDebounceUnreadMs", serialize_with = "millis")]
    pub idle_debounce_unread: Duration,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for TerminalAnalyzerSettings {
    fn default() -> Self {
        Self {
            provider: ClassifierProvider::ChatCompletions,
            title_model: "openai/gpt-oss-safeguard-20b".to_owned(),
            model: "openai/gpt-oss-safeguard-20b".to_owned(),
            system_prompt: DEFAULT_ANALYZER_SYSTEM_PROMPT.to_owned(),
            criteria: ClassifierCriteria::default(),
            title_system_prompt: DEFAULT_TITLE_SYSTEM_PROMPT.to_owned(),
            reasoning_effort: ReasoningEffort::Low,
            safe_paths: vec!["/tmp".to_owned()],
            buffer_lines: 30,
            idle_debounce: Duration::from_millis(2000),
            idle_debounce_unread: Duration::from_millis(15000),
            extra: Extra::new(),
        }
    }
}

// === github, debug ==========================================================================

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GithubSettings {
    pub pat: String,
    pub autodetect_via_gh: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

impl Default for GithubSettings {
    fn default() -> Self {
        Self { pat: String::new(), autodetect_via_gh: true, extra: Extra::new() }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DebugSettings {
    pub show_badge: bool,
    #[serde(flatten)]
    pub extra: Extra,
}

// === serialization helpers ==================================================================

/// A duration as whole milliseconds, the unit every `*Ms`/`timeout` key uses.
fn millis<S: Serializer>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error> {
    serializer.serialize_u64(u64::try_from(duration.as_millis()).map_err(S::Error::custom)?)
}

/// An `f32` written as JavaScript would write the number it came from: the shortest decimal
/// that reads back as the same `f32` (13.3, not 13.300000190734863), and without a fraction
/// when integral (14, not 14.0).
fn js_number_f32<S: Serializer>(value: &f32, serializer: S) -> Result<S::Ok, S::Error> {
    if !value.is_finite() {
        return Err(S::Error::custom(format!("{value} is not a JSON number")));
    }
    let decimal: f64 = value.to_string().parse().map_err(S::Error::custom)?;
    // Every finite f32 with no fraction is far inside i64.
    if decimal.fract() == 0.0 {
        serializer.serialize_i64(decimal as i64)
    } else {
        serializer.serialize_f64(decimal)
    }
}

// === file I/O ===============================================================================

pub fn settings_path(user_data_dir: &Path) -> PathBuf {
    user_data_dir.join("settings.json")
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("cannot read {path}: {source}")]
    Read { path: PathBuf, source: std::io::Error },
    #[error("{path} is not valid JSON: {source}")]
    Parse { path: PathBuf, source: serde_json::Error },
    #[error("{path} holds invalid settings: {source}")]
    Invalid { path: PathBuf, source: ValidationError },
    #[error("cannot encode settings for {path}: {source}")]
    Encode { path: PathBuf, source: serde_json::Error },
    #[error("cannot write {path}: {source}")]
    Write { path: PathBuf, source: std::io::Error },
}

/// Load settings. A missing file is [`Settings::default`]. An unreadable or corrupt file, or
/// classifier settings Electron's Zod schema rejects, is an error the caller must show:
/// Electron throws on the latter too, but falls back to the defaults on the former (and
/// overwrites the file with them), which hides the problem and loses the user's settings.
pub fn load(user_data_dir: &Path) -> Result<Settings, SettingsError> {
    let path = settings_path(user_data_dir);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Settings::default()),
        Err(source) => return Err(SettingsError::Read { path, source }),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(source) => return Err(SettingsError::Parse { path, source }),
    };
    from_value(value).map_err(|source| SettingsError::Invalid { path, source })
}

/// Write `settings` as Electron's `saveSettings` does (`JSON.stringify(settings, null, 2)`),
/// creating the directory if needed.
pub fn save(user_data_dir: &Path, settings: &Settings) -> Result<(), SettingsError> {
    let path = settings_path(user_data_dir);
    let json = match serde_json::to_vec_pretty(settings) {
        Ok(json) => json,
        Err(source) => return Err(SettingsError::Encode { path, source }),
    };
    if let Err(source) = std::fs::create_dir_all(user_data_dir) {
        return Err(SettingsError::Write { path: user_data_dir.to_owned(), source });
    }
    // In place, like Electron's writeFileSync: keeps the file's mode, inode and symlink.
    std::fs::write(&path, json).map_err(|source| SettingsError::Write { path, source })
}
