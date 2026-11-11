//! Reading `settings.json` the way Electron's `mergeSettings` does.
//!
//! Each key missing from the file takes Electron's default, and every legacy migration runs.
//! Electron does not validate the rest: a value of the wrong type is passed through to
//! whatever uses it. Here every value has a type, so a key holding a value that type cannot
//! hold falls back to the default for that key (logged), and the next save writes the
//! default. The exception is what `classifierSettingsSchema` checks — `provider`, `model`,
//! `titleModel` and `criteria` — which Electron refuses to load; so does [`from_value`].

use std::time::Duration;

use serde::Deserialize as _;
use serde::de::DeserializeOwned;
use serde_json::{Map, Value};

use super::{
    AiHarnessInstance, AppearanceSettings, BrowserInstance, ClassifiedState, ClassifierCriteria,
    ClassifierProvider, ConnectionLabel, CustomRunnerInstance, DEFAULT_AI_HARNESS_COMMAND,
    DaemonSettings, DebugSettings, GithubSettings, IdentityFile, Instances, Keybindings,
    LEGACY_DEFAULT_SYSTEM_PROMPTS, LlmSettings, NotificationSettings, PortForwardSpec,
    PrefixModeConfig, SandboxSettings, Settings, SshConnectionConfig, SshSettings,
    TerminalAnalyzerSettings, TerminalInstance, TerminalSettings, WebAppInstance,
};

/// What makes Electron refuse a settings file it could parse.
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("the top level is {found}, not an object")]
    NotAnObject { found: &'static str },
    /// A value `classifierSettingsSchema` rejects (Electron throws its `ZodError`).
    #[error("{key}: {source}")]
    Classifier { key: String, source: serde_json::Error },
}

/// Settings from the parsed contents of `settings.json`, merged over the defaults with
/// Electron's migrations (`mergeSettings`):
/// - Sections and keys that are missing take [`Settings::default`]'s values, except that
///   the instance lists default to empty: only a missing file gets the default AI harness.
/// - `terminal.instances` empty: the custom terminals of a legacy top-level `applications`
///   list become terminal instances.
/// - `aiHarness.instances` empty: a legacy top-level `claude` object becomes one instance.
/// - A keybinding in the legacy `{ direct, prefixMode }` form becomes its `prefixMode` key;
///   an empty one takes the default.
/// - `terminalAnalyzer.provider: "jev"` becomes [`ClassifierProvider::Classifier`] with
///   `jevModel` as its model; a missing `titleModel` is the saved `model`; a `systemPrompt`
///   that is one of [`LEGACY_DEFAULT_SYSTEM_PROMPTS`] becomes the current default; criteria
///   missing from the file take the default.
/// - An empty `globalDefaultApplicationId` takes the default.
pub fn from_value(value: Value) -> Result<Settings, ValidationError> {
    let map = match value {
        Value::Object(map) => map,
        other => return Err(ValidationError::NotAnObject { found: json_type(&other) }),
    };
    let defaults = Settings::default();
    let mut root = Fields { path: String::new(), map };
    let applications = root.raw("applications");
    let claude = root.raw("claude");
    let terminal = read_terminal(root.section("terminal"), defaults.terminal, applications);
    let sandbox = read_sandbox(root.section("sandbox"), defaults.sandbox);
    let ai_harness = read_ai_harness(root.section("aiHarness"), claude);
    let custom_runner = read_instances(root.section("customRunner"), read_custom_runner_instance);
    let web_app = read_instances(root.section("webApp"), read_web_app_instance);
    let browser = read_instances(root.section("browser"), read_browser_instance);
    let notifications = read_notifications(root.section("notifications"), defaults.notifications);
    let appearance = read_appearance(root.section("appearance"), defaults.appearance);
    let prefix_mode = read_prefix_mode(root.section("prefixMode"), defaults.prefix_mode);
    let keybindings = read_keybindings(root.section("keybindings"), defaults.keybindings);
    let daemon = read_daemon(root.section("daemon"), defaults.daemon);
    let ssh = read_ssh(root.section("ssh"));
    let llm = read_llm(root.section("llm"), defaults.llm);
    let terminal_analyzer =
        read_terminal_analyzer(root.section("terminalAnalyzer"), defaults.terminal_analyzer)?;
    let github = read_github(root.section("github"), defaults.github);
    let global_default_application_id = root.take_valid(
        "globalDefaultApplicationId",
        defaults.global_default_application_id,
        |id: &String| !id.is_empty(),
    );
    let recent_directories = root.take("recentDirectories", defaults.recent_directories);
    let debug = read_debug(root.section("debug"), defaults.debug);
    Ok(Settings {
        terminal,
        sandbox,
        ai_harness,
        custom_runner,
        web_app,
        browser,
        notifications,
        appearance,
        prefix_mode,
        keybindings,
        daemon,
        ssh,
        llm,
        terminal_analyzer,
        github,
        global_default_application_id,
        recent_directories,
        debug,
        extra: root.rest(),
    })
}

/// One JSON object being read. Every read removes its key, so what is left at the end is
/// the keys this version does not know, kept as the struct's `extra`.
struct Fields {
    /// Dotted path of this object, for log messages; empty at the top level.
    path: String,
    map: Map<String, Value>,
}

impl Fields {
    /// A missing section is empty; one that is not an object is ignored (and logged), like
    /// spreading it in JavaScript adds nothing.
    fn new(path: String, value: Option<Value>) -> Fields {
        let map = match value {
            None => Map::new(),
            Some(Value::Object(map)) => map,
            Some(other) => {
                tracing::warn!(key = %path, found = json_type(&other), "setting is not an object; using the defaults");
                Map::new()
            }
        };
        Fields { path, map }
    }

    fn key_path(&self, key: &str) -> String {
        match self.path.as_str() {
            "" => key.to_owned(),
            path => format!("{path}.{key}"),
        }
    }

    fn section(&mut self, key: &str) -> Fields {
        let path = self.key_path(key);
        let value = self.map.remove(key);
        Fields::new(path, value)
    }

    fn raw(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key)
    }

    /// `key` as a `T`, or `default` when it is missing or not a `T`.
    fn take<T: DeserializeOwned>(&mut self, key: &str, default: T) -> T {
        self.take_valid(key, default, |_| true)
    }

    /// `key` as a `T` that passes `valid`, or `default`.
    fn take_valid<T: DeserializeOwned>(&mut self, key: &str, default: T, valid: impl FnOnce(&T) -> bool) -> T {
        let Some(value) = self.map.remove(key) else {
            return default;
        };
        match T::deserialize(&value) {
            Ok(parsed) if valid(&parsed) => parsed,
            Ok(_) => {
                tracing::warn!(key = %self.key_path(key), %value, "setting out of range; using the default");
                default
            }
            Err(error) => {
                tracing::warn!(key = %self.key_path(key), %error, "invalid setting; using the default");
                default
            }
        }
    }

    fn take_millis(&mut self, key: &str, default: Duration) -> Duration {
        match self.raw(key) {
            None => default,
            Some(value) => match u64::deserialize(&value) {
                Ok(ms) => Duration::from_millis(ms),
                Err(error) => {
                    tracing::warn!(key = %self.key_path(key), %error, "invalid setting; using the default");
                    default
                }
            },
        }
    }

    /// An array of objects, each read by `read`. A missing or non-array value is empty
    /// (Electron: `loaded.x || []`); items that are not objects are dropped (and logged).
    fn take_list<T>(&mut self, key: &str, read: impl Fn(Fields) -> T) -> Vec<T> {
        let path = self.key_path(key);
        let items = match self.map.remove(key) {
            None => return Vec::new(),
            Some(Value::Array(items)) => items,
            Some(other) => {
                tracing::warn!(key = %path, found = json_type(&other), "setting is not a list; using an empty one");
                return Vec::new();
            }
        };
        items
            .into_iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let path = format!("{path}[{index}]");
                match item {
                    Value::Object(map) => Some(read(Fields { path, map })),
                    other => {
                        tracing::warn!(key = %path, found = json_type(&other), "list item is not an object; dropping it");
                        None
                    }
                }
            })
            .collect()
    }

    /// A key `classifierSettingsSchema` validates: present means it must be a `T`.
    fn schema<T: DeserializeOwned>(&self, key: &str, value: &Value) -> Result<T, ValidationError> {
        T::deserialize(value).map_err(|source| ValidationError::Classifier { key: self.key_path(key), source })
    }

    fn rest(self) -> Map<String, Value> {
        self.map
    }
}

fn json_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) => "an array",
        Value::Object(_) => "an object",
    }
}

/// JavaScript truthiness, for the legacy migrations' `||` defaults.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// A non-empty string, which is what `x || fallback` keeps of a string-typed `x`.
fn non_empty_string(value: Option<&Value>) -> Option<&str> {
    match value {
        Some(Value::String(s)) if !s.is_empty() => Some(s),
        Some(_) | None => None,
    }
}

// === terminal ===============================================================================

fn read_terminal(mut f: Fields, d: TerminalSettings, applications: Option<Value>) -> TerminalSettings {
    let font_size = f.take_valid("fontSize", d.font_size, |size: &f32| size.is_finite() && *size > 0.0);
    let font_family = f.take("fontFamily", d.font_family);
    let cursor_style = f.take("cursorStyle", d.cursor_style);
    let cursor_blink = f.take("cursorBlink", d.cursor_blink);
    let show_raw_chars = f.take("showRawChars", d.show_raw_chars);
    let allow_osc52_clipboard = f.take("allowOsc52Clipboard", d.allow_osc52_clipboard);
    let max_cols = f.take_valid("maxCols", d.max_cols, |cols: &u16| *cols >= 2);
    let mut instances = f.take_list("instances", read_terminal_instance);
    if instances.is_empty()
        && let Some(applications) = applications
    {
        instances = migrate_applications(applications);
    }
    TerminalSettings {
        font_size,
        font_family,
        cursor_style,
        cursor_blink,
        show_raw_chars,
        allow_osc52_clipboard,
        max_cols,
        instances,
        extra: f.rest(),
    }
}

fn read_terminal_instance(mut f: Fields) -> TerminalInstance {
    TerminalInstance {
        id: f.take("id", String::new()),
        name: f.take("name", String::new()),
        icon: f.take("icon", String::new()),
        startup_command: f.take("startupCommand", String::new()),
        is_default: f.take("isDefault", false),
        extra: f.rest(),
    }
}

/// The legacy `applications` list: its non-built-in terminals that have a command.
fn migrate_applications(applications: Value) -> Vec<TerminalInstance> {
    let Value::Array(applications) = applications else {
        tracing::warn!(found = json_type(&applications), "legacy `applications` is not a list; ignoring it");
        return Vec::new();
    };
    applications
        .into_iter()
        .filter_map(|application| {
            let Value::Object(app) = application else {
                return None;
            };
            let is_terminal = match app.get("applicationId") {
                Some(Value::String(id)) => id == "terminal" || id.is_empty(),
                Some(other) => !truthy(other),
                None => true,
            };
            let built_in = app.get("isBuiltIn").is_some_and(truthy);
            let command = non_empty_string(app.get("config").and_then(|config| config.get("command")))
                .or_else(|| non_empty_string(app.get("command")))?
                .to_owned();
            if built_in || !is_terminal {
                return None;
            }
            let mut f = Fields { path: "applications[]".to_owned(), map: app };
            Some(TerminalInstance {
                id: f.take("id", String::new()),
                name: f.take("name", String::new()),
                icon: f.take("icon", String::new()),
                startup_command: command,
                is_default: f.take("isDefault", false),
                extra: Map::new(),
            })
        })
        .collect()
}

fn read_sandbox(mut f: Fields, d: SandboxSettings) -> SandboxSettings {
    SandboxSettings {
        enabled_by_default: f.take("enabledByDefault", d.enabled_by_default),
        allow_network_by_default: f.take("allowNetworkByDefault", d.allow_network_by_default),
        extra: f.rest(),
    }
}

// === application instances ==================================================================

fn read_instances<T>(mut f: Fields, read: impl Fn(Fields) -> T) -> Instances<T> {
    let instances = f.take_list("instances", read);
    Instances { instances, extra: f.rest() }
}

fn read_ai_harness(mut f: Fields, claude: Option<Value>) -> Instances<AiHarnessInstance> {
    let mut instances = f.take_list("instances", read_ai_harness_instance);
    if instances.is_empty()
        && let Some(claude) = claude.filter(truthy)
    {
        instances = vec![migrate_claude(Fields::new("claude".to_owned(), Some(claude)))];
    }
    Instances { instances, extra: f.rest() }
}

/// Fields added after an instance was first saved (`keepOnExit`, `disableScrollbar`) are
/// `false` when missing, as Electron reads them.
fn read_ai_harness_instance(mut f: Fields) -> AiHarnessInstance {
    AiHarnessInstance {
        id: f.take("id", String::new()),
        name: f.take("name", String::new()),
        icon: f.take("icon", String::new()),
        command: f.take("command", String::new()),
        is_default: f.take("isDefault", false),
        enable_sandbox: f.take("enableSandbox", false),
        allow_network: f.take("allowNetwork", false),
        background_color: f.take("backgroundColor", String::new()),
        disable_scrollbar: f.take("disableScrollbar", false),
        keep_on_exit: f.take("keepOnExit", false),
        extra: f.rest(),
    }
}

/// The legacy `claude: { command, startByDefault, enableSandbox }` object.
fn migrate_claude(mut f: Fields) -> AiHarnessInstance {
    let flag = |f: &mut Fields, key: &str| f.raw(key).is_some_and(|value| truthy(&value));
    let command = non_empty_string(f.raw("command").as_ref())
        .unwrap_or(DEFAULT_AI_HARNESS_COMMAND)
        .to_owned();
    AiHarnessInstance {
        id: "claude".to_owned(),
        name: "Claude".to_owned(),
        icon: "✦".to_owned(),
        command,
        is_default: flag(&mut f, "startByDefault"),
        enable_sandbox: flag(&mut f, "enableSandbox"),
        allow_network: true,
        background_color: "#1a1a24".to_owned(),
        disable_scrollbar: false,
        keep_on_exit: false,
        extra: Map::new(),
    }
}

fn read_custom_runner_instance(mut f: Fields) -> CustomRunnerInstance {
    CustomRunnerInstance {
        id: f.take("id", String::new()),
        name: f.take("name", String::new()),
        icon: f.take("icon", String::new()),
        command_template: f.take("commandTemplate", String::new()),
        is_default: f.take("isDefault", false),
        extra: f.rest(),
    }
}

fn read_web_app_instance(mut f: Fields) -> WebAppInstance {
    WebAppInstance {
        id: f.take("id", String::new()),
        name: f.take("name", String::new()),
        icon: f.take("icon", String::new()),
        command: f.take("command", String::new()),
        is_default: f.take("isDefault", false),
        keep_on_exit: f.take("keepOnExit", false),
        extra: f.rest(),
    }
}

fn read_browser_instance(mut f: Fields) -> BrowserInstance {
    BrowserInstance {
        id: f.take("id", String::new()),
        name: f.take("name", String::new()),
        icon: f.take("icon", String::new()),
        url: f.take("url", String::new()),
        is_default: f.take("isDefault", false),
        extra: f.rest(),
    }
}

// === small sections =========================================================================

fn read_notifications(mut f: Fields, d: NotificationSettings) -> NotificationSettings {
    NotificationSettings { sound_enabled: f.take("soundEnabled", d.sound_enabled), extra: f.rest() }
}

fn read_appearance(mut f: Fields, d: AppearanceSettings) -> AppearanceSettings {
    AppearanceSettings { theme: f.take("theme", d.theme), extra: f.rest() }
}

fn read_prefix_mode(mut f: Fields, d: PrefixModeConfig) -> PrefixModeConfig {
    PrefixModeConfig {
        enabled: f.take("enabled", d.enabled),
        prefix_key: f.take("prefixKey", d.prefix_key),
        timeout: f.take_millis("timeout", d.timeout),
        extra: f.rest(),
    }
}

fn read_keybindings(mut f: Fields, d: Keybindings) -> Keybindings {
    Keybindings {
        new_tab: keybinding(&mut f, "newTab", d.new_tab),
        close_tab: keybinding(&mut f, "closeTab", d.close_tab),
        next_tab: keybinding(&mut f, "nextTab", d.next_tab),
        prev_tab: keybinding(&mut f, "prevTab", d.prev_tab),
        open_settings: keybinding(&mut f, "openSettings", d.open_settings),
        workspace_focus: keybinding(&mut f, "workspaceFocus", d.workspace_focus),
        extra: f.rest(),
    }
}

/// A key string, or the legacy `{ direct, prefixMode }` object's `prefixMode` key
/// (`migrateKeybindings`). Empty means the default.
fn keybinding(f: &mut Fields, key: &str, default: String) -> String {
    match f.raw(key) {
        None => default,
        Some(Value::String(binding)) if binding.is_empty() => default,
        Some(Value::String(binding)) => binding,
        Some(Value::Object(legacy)) => non_empty_string(legacy.get("prefixMode")).map_or(default, str::to_owned),
        Some(other) => {
            tracing::warn!(key = %f.key_path(key), found = json_type(&other), "invalid keybinding; using the default");
            default
        }
    }
}

fn read_daemon(mut f: Fields, d: DaemonSettings) -> DaemonSettings {
    DaemonSettings {
        merge_threshold: f.take("mergeThreshold", d.merge_threshold),
        compacted_limit: f.take("compactedLimit", d.compacted_limit),
        scrollback_lines: f.take("scrollbackLines", d.scrollback_lines),
        extra: f.rest(),
    }
}

fn read_llm(mut f: Fields, d: LlmSettings) -> LlmSettings {
    LlmSettings {
        base_url: f.take("baseUrl", d.base_url),
        api_key: f.take("apiKey", d.api_key),
        model: f.take("model", d.model),
        extra: f.rest(),
    }
}

fn read_github(mut f: Fields, d: GithubSettings) -> GithubSettings {
    GithubSettings {
        pat: f.take("pat", d.pat),
        autodetect_via_gh: f.take("autodetectViaGh", d.autodetect_via_gh),
        extra: f.rest(),
    }
}

fn read_debug(mut f: Fields, d: DebugSettings) -> DebugSettings {
    DebugSettings { show_badge: f.take("showBadge", d.show_badge), extra: f.rest() }
}

// === ssh ====================================================================================

fn read_ssh(mut f: Fields) -> SshSettings {
    SshSettings { saved_connections: f.take_list("savedConnections", read_connection), extra: f.rest() }
}

fn read_connection(mut f: Fields) -> SshConnectionConfig {
    let id = f.take("id", String::new());
    let host = f.take("host", String::new());
    let user = f.take("user", String::new());
    let port = f.take("port", 22);
    let identity_file = match f.take("identityFile", String::new()) {
        path if path.is_empty() => IdentityFile::SshDefault,
        path => IdentityFile::Path(path),
    };
    let label = match f.take("label", String::new()) {
        label if label.is_empty() => ConnectionLabel::UserAtHost,
        label => ConnectionLabel::Custom(label),
    };
    SshConnectionConfig {
        id,
        host,
        user,
        port,
        identity_file,
        label,
        port_forwards: f.take_list("portForwards", read_port_forward),
        extra: f.rest(),
    }
}

fn read_port_forward(mut f: Fields) -> PortForwardSpec {
    PortForwardSpec {
        local_port: f.take("localPort", 0),
        remote_host: f.take("remoteHost", String::new()),
        remote_port: f.take("remotePort", 0),
        extra: f.rest(),
    }
}

// === terminal analyzer ======================================================================

fn read_terminal_analyzer(
    mut f: Fields,
    d: TerminalAnalyzerSettings,
) -> Result<TerminalAnalyzerSettings, ValidationError> {
    // The classifier model of the former per-provider settings; consumed, never written back.
    let jev_model = f.raw("jevModel");
    let provider = f.raw("provider");
    let model = f.raw("model");
    let title_model = f.raw("titleModel");
    let legacy_classifier = matches!(&provider, Some(Value::String(p)) if p == "jev");

    let provider = match provider {
        _ if legacy_classifier => ClassifierProvider::Classifier,
        None => d.provider,
        Some(value) => f.schema("provider", &value)?,
    };
    let saved_model = match (legacy_classifier, jev_model, &model) {
        (true, Some(value), _) => f.schema("model", &value)?,
        (_, _, None) => d.model,
        (_, _, Some(value)) => f.schema("model", value)?,
    };
    let title_model = match (title_model, &model) {
        (Some(value), _) => f.schema("titleModel", &value)?,
        // `loaded.model ?? default`: a null model counts as missing here.
        (None, None | Some(Value::Null)) => d.title_model,
        (None, Some(value)) => f.schema("titleModel", value)?,
    };
    let system_prompt = match f.raw("systemPrompt") {
        None => d.system_prompt,
        Some(Value::String(prompt)) if LEGACY_DEFAULT_SYSTEM_PROMPTS.contains(&prompt.as_str()) => d.system_prompt,
        Some(Value::String(prompt)) => prompt,
        Some(other) => {
            tracing::warn!(key = %f.key_path("systemPrompt"), found = json_type(&other), "invalid setting; using the default");
            d.system_prompt
        }
    };
    let criteria = read_criteria(f.section("criteria"), d.criteria)?;
    Ok(TerminalAnalyzerSettings {
        provider,
        title_model,
        model: saved_model,
        system_prompt,
        criteria,
        title_system_prompt: f.take("titleSystemPrompt", d.title_system_prompt),
        reasoning_effort: f.take("reasoningEffort", d.reasoning_effort),
        safe_paths: f.take("safePaths", d.safe_paths),
        buffer_lines: f.take("bufferLines", d.buffer_lines),
        idle_debounce: f.take_millis("idleDebounceMs", d.idle_debounce),
        idle_debounce_unread: f.take_millis("idleDebounceUnreadMs", d.idle_debounce_unread),
        extra: f.rest(),
    })
}

/// Saved criteria over the defaults; each saved one must be a string.
fn read_criteria(mut f: Fields, d: ClassifierCriteria) -> Result<ClassifierCriteria, ValidationError> {
    let mut criterion = |state: ClassifiedState, default: String| match f.raw(state.as_str()) {
        None => Ok(default),
        Some(value) => f.schema::<String>(state.as_str(), &value),
    };
    let working = criterion(ClassifiedState::Working, d.working)?;
    let safe_permission_requested = criterion(ClassifiedState::SafePermissionRequested, d.safe_permission_requested)?;
    let permission_request = criterion(ClassifiedState::PermissionRequest, d.permission_request)?;
    let completed = criterion(ClassifiedState::Completed, d.completed)?;
    let user_input_required = criterion(ClassifiedState::UserInputRequired, d.user_input_required)?;
    let application_error = criterion(ClassifiedState::ApplicationError, d.application_error)?;
    let idle = criterion(ClassifiedState::Idle, d.idle)?;
    Ok(ClassifierCriteria {
        working,
        safe_permission_requested,
        permission_request,
        completed,
        user_input_required,
        application_error,
        idle,
        extra: f.rest(),
    })
}
