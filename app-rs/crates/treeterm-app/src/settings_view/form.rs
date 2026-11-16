//! The dialog's logic, free of GPUI: its sections, what is typed into it, how that is
//! validated, and which parts of `Settings` a save writes.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use treeterm_core::keybinding::{Os, PrefixAction, format_accelerator};
use treeterm_core::settings::{
    AiHarnessInstance, AppearanceSettings, BrowserInstance, ClassifiedState, ClassifierCriteria,
    CustomRunnerInstance, DebugSettings, GithubSettings, Instances, Keybindings, LlmSettings,
    PrefixModeConfig, SandboxSettings, Settings, TerminalAnalyzerSettings, TerminalInstance,
    TerminalSettings, WebAppInstance,
};
use treeterm_model::apps::AppRegistry;

use crate::number_field::{NumberSpec, number_value};

/// The dialog's sections, one shown at a time (Electron's `TabId`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    General,
    Terminal,
    TerminalProfiles,
    AiHarness,
    CustomRunners,
    WebApps,
    Browsers,
    Llm,
    Sandbox,
    Appearance,
    Keybindings,
    Github,
    Debug,
}

impl Section {
    /// Sidebar order.
    pub const ALL: [Section; 13] = [
        Section::General,
        Section::Terminal,
        Section::TerminalProfiles,
        Section::AiHarness,
        Section::CustomRunners,
        Section::WebApps,
        Section::Browsers,
        Section::Llm,
        Section::Sandbox,
        Section::Appearance,
        Section::Keybindings,
        Section::Github,
        Section::Debug,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Section::General => "General",
            Section::Terminal => "Terminal",
            Section::TerminalProfiles => "Terminal Profiles",
            Section::AiHarness => "AI Harness",
            Section::CustomRunners => "Custom Runners",
            Section::WebApps => "Web Apps",
            Section::Browsers => "Browsers",
            Section::Llm => "LLM",
            Section::Sandbox => "Sandbox",
            Section::Appearance => "Appearance",
            Section::Keybindings => "Keybindings",
            Section::Github => "GitHub",
            Section::Debug => "Debug",
        }
    }
}

/// Why "Save" refused, and the section holding the field to fix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormError {
    pub section: Section,
    pub message: String,
}

impl FormError {
    fn new(section: Section, message: impl Into<String>) -> Self {
        Self { section, message: message.into() }
    }
}

/// What the dialog's single-value text fields hold, as typed. Parsed on save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Texts {
    pub font_size: String,
    pub max_cols: String,
    pub font_family: String,
    pub llm_base_url: String,
    pub llm_api_key: String,
    pub llm_model: String,
    pub analyzer_model: String,
    pub analyzer_system_prompt: String,
    /// In [`ClassifiedState::ALL`] order.
    pub criteria: [String; 7],
    pub title_model: String,
    pub title_system_prompt: String,
    /// Comma-separated.
    pub safe_paths: String,
    pub buffer_lines: String,
    /// Milliseconds.
    pub idle_debounce: String,
    /// Milliseconds.
    pub idle_debounce_unread: String,
    /// An Electron accelerator such as `Control+B`.
    pub prefix_key: String,
    /// Milliseconds.
    pub prefix_timeout: String,
    /// In [`PrefixAction::ALL`] order.
    pub action_keys: [String; 6],
    pub github_pat: String,
}

impl Texts {
    /// The fields as `settings` fills them in when the dialog opens.
    pub fn from_settings(settings: &Settings) -> Texts {
        let analyzer = &settings.terminal_analyzer;
        Texts {
            font_size: settings.terminal.font_size.to_string(),
            max_cols: settings.terminal.max_cols.to_string(),
            font_family: settings.terminal.font_family.clone(),
            llm_base_url: settings.llm.base_url.clone(),
            llm_api_key: settings.llm.api_key.clone(),
            llm_model: settings.llm.model.clone(),
            analyzer_model: analyzer.model.clone(),
            analyzer_system_prompt: analyzer.system_prompt.clone(),
            criteria: ClassifiedState::ALL.map(|state| analyzer.criteria.get(state).to_owned()),
            title_model: analyzer.title_model.clone(),
            title_system_prompt: analyzer.title_system_prompt.clone(),
            safe_paths: analyzer.safe_paths.join(", "),
            buffer_lines: analyzer.buffer_lines.to_string(),
            idle_debounce: analyzer.idle_debounce.as_millis().to_string(),
            idle_debounce_unread: analyzer.idle_debounce_unread.as_millis().to_string(),
            prefix_key: settings.prefix_mode.prefix_key.clone(),
            prefix_timeout: settings.prefix_mode.timeout_ms.to_string(),
            action_keys: PrefixAction::ALL.map(|action| action.stored_key(&settings.keybindings).to_owned()),
            github_pat: settings.github.pat.clone(),
        }
    }

    /// Parse the fields into `settings`; nothing is written unless all of them are valid.
    pub fn apply(&self, settings: &mut Settings) -> Result<(), FormError> {
        let font_size = match self.font_size.trim().parse::<f32>() {
            Ok(size) if size.is_finite() && size > 0. => size,
            Ok(_) | Err(_) => return Err(FormError::new(Section::Terminal, "Font size must be a positive number")),
        };
        let max_cols = whole_number::<u16>(&self.max_cols, 2)
            .ok_or_else(|| FormError::new(Section::Terminal, "Max columns must be a whole number of at least 2"))?;
        let buffer_lines = whole_number::<u32>(&self.buffer_lines, 1)
            .ok_or_else(|| FormError::new(Section::Llm, "Buffer lines must be a whole number of at least 1"))?;
        let idle_debounce = whole_number::<u64>(&self.idle_debounce, 0)
            .ok_or_else(|| FormError::new(Section::Llm, "Idle debounce must be a whole number of milliseconds"))?;
        let idle_debounce_unread = whole_number::<u64>(&self.idle_debounce_unread, 0).ok_or_else(|| {
            FormError::new(Section::Llm, "Idle debounce when unread must be a whole number of milliseconds")
        })?;

        let terminal = &mut settings.terminal;
        terminal.font_size = font_size;
        terminal.max_cols = max_cols;
        terminal.font_family = self.font_family.trim().to_owned();
        settings.llm.base_url = self.llm_base_url.clone();
        settings.llm.api_key = self.llm_api_key.clone();
        settings.llm.model = self.llm_model.clone();
        let analyzer = &mut settings.terminal_analyzer;
        analyzer.model = self.analyzer_model.clone();
        analyzer.system_prompt = self.analyzer_system_prompt.clone();
        for (state, text) in ClassifiedState::ALL.iter().zip(&self.criteria) {
            *criterion_mut(&mut analyzer.criteria, *state) = text.clone();
        }
        analyzer.title_model = self.title_model.clone();
        analyzer.title_system_prompt = self.title_system_prompt.clone();
        analyzer.safe_paths = parse_safe_paths(&self.safe_paths);
        analyzer.buffer_lines = buffer_lines;
        analyzer.idle_debounce = Duration::from_millis(idle_debounce);
        analyzer.idle_debounce_unread = Duration::from_millis(idle_debounce_unread);
        // The keys as recorded, a Space action key (` `) included: one that matches no key
        // press only disables itself (`KeybindingSettings`), as in Electron.
        settings.prefix_mode.prefix_key = self.prefix_key.clone();
        // A pasted negative timeout is stored as typed, as Electron's is (prefix mode then ends
        // at once). Whole milliseconds; one too large for an `i64` is the largest.
        settings.prefix_mode.timeout_ms = prefix_timeout_ms(&self.prefix_timeout) as i64;
        for (action, key) in PrefixAction::ALL.iter().zip(&self.action_keys) {
            *action_key_mut(&mut settings.keybindings, *action) = key.clone();
        }
        settings.github.pat = self.github_pat.clone();
        Ok(())
    }
}

/// `text` as a whole number of at least `min`.
fn whole_number<T: FromStr + PartialOrd>(text: &str, min: T) -> Option<T> {
    match text.trim().parse::<T>() {
        Ok(value) if value >= min => Some(value),
        Ok(_) | Err(_) => None,
    }
}

/// The Font Size field (`<input type="number" min=8 max=32>`).
pub const FONT_SIZE: NumberSpec = NumberSpec { min: 8., max: 32., step: 1. };
/// The Max Columns field (`<input type="number" min=40 max=1000>`).
pub const MAX_COLS: NumberSpec = NumberSpec { min: 40., max: 1000., step: 1. };
/// The Buffer Lines field (`<input type="number" min=1 max=100>`).
pub const BUFFER_LINES: NumberSpec = NumberSpec { min: 1., max: 100., step: 1. };
/// The Idle Debounce (ms) field (`<input type="number" min=0 step=100>`).
pub const IDLE_DEBOUNCE: NumberSpec = NumberSpec { min: 0., max: f64::MAX, step: 100. };
/// The Idle Debounce When Unread (ms) field (`<input type="number" min=0 step=100>`).
pub const IDLE_DEBOUNCE_UNREAD: NumberSpec = NumberSpec { min: 0., max: f64::MAX, step: 100. };
/// The prefix Timeout field (`<input type="number" min=500 max=5000 step=100>`).
pub const PREFIX_TIMEOUT: NumberSpec = NumberSpec { min: 500., max: 5000., step: 100. };

/// The timeout the Timeout field holds for `text`: Electron's `parseInt(value) || 1500`,
/// where `value` is empty unless `text` is a valid number. Whole milliseconds, never 0;
/// negative when a negative one is pasted (`-5`), as in Electron.
pub fn prefix_timeout_ms(text: &str) -> f64 {
    let parsed = match number_value(text) {
        Some(_) => parse_int(text),
        None => None,
    };
    match parsed {
        // `0 || 1500`, `-0 || 1500` and `NaN || 1500`.
        Some(ms) if ms != 0. => ms,
        Some(_) | None => 1500.,
    }
}

/// What React writes back into the Timeout field once `text` is typed into it: the timeout
/// it holds ([`prefix_timeout_ms`]), unless the field's value is that number already
/// (`node.value != value`, the empty value counting as 0). So an emptied field shows
/// `1500`, and `1500.7` turns into `1500`.
pub fn prefix_timeout_field(text: &str) -> Option<String> {
    let ms = prefix_timeout_ms(text);
    if number_value(text).unwrap_or(0.) == ms { None } else { Some(ms.to_string()) }
}

/// JavaScript's `parseInt` of a valid number: its sign and leading digits (`1e3` is 1,
/// `12.9` is 12), or `None` (NaN) when it starts with neither (`.5`).
fn parse_int(text: &str) -> Option<f64> {
    let (sign, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (-1., rest),
        None => (1., text),
    };
    let end = unsigned.find(|c: char| !c.is_ascii_digit()).unwrap_or(unsigned.len());
    let digits = &unsigned[..end];
    if digits.is_empty() {
        return None;
    }
    // As a float, as JavaScript holds it: any run of digits is one.
    Some(sign * digits.parse::<f64>().expect("ASCII digits parse as a float"))
}

/// Electron's safe-paths field: comma-separated, trimmed, empty entries dropped.
pub fn parse_safe_paths(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

fn action_key_mut(keybindings: &mut Keybindings, action: PrefixAction) -> &mut String {
    match action {
        PrefixAction::NewTab => &mut keybindings.new_tab,
        PrefixAction::CloseTab => &mut keybindings.close_tab,
        PrefixAction::NextTab => &mut keybindings.next_tab,
        PrefixAction::PrevTab => &mut keybindings.prev_tab,
        PrefixAction::OpenSettings => &mut keybindings.open_settings,
        PrefixAction::WorkspaceFocus => &mut keybindings.workspace_focus,
    }
}

fn criterion_mut(criteria: &mut ClassifierCriteria, state: ClassifiedState) -> &mut String {
    match state {
        ClassifiedState::Working => &mut criteria.working,
        ClassifiedState::SafePermissionRequested => &mut criteria.safe_permission_requested,
        ClassifiedState::PermissionRequest => &mut criteria.permission_request,
        ClassifiedState::Completed => &mut criteria.completed,
        ClassifiedState::UserInputRequired => &mut criteria.user_input_required,
        ClassifiedState::ApplicationError => &mut criteria.application_error,
        ClassifiedState::Idle => &mut criteria.idle,
    }
}

/// The instance lists as edited, in display order.
pub struct Lists {
    pub profiles: Vec<TerminalInstance>,
    pub harnesses: Vec<AiHarnessInstance>,
    pub runners: Vec<CustomRunnerInstance>,
    pub web_apps: Vec<WebAppInstance>,
    pub browsers: Vec<BrowserInstance>,
}

impl Lists {
    /// Into `settings`, replacing its lists.
    pub fn store_in(self, settings: &mut Settings) {
        settings.terminal.instances = self.profiles;
        settings.ai_harness.instances = self.harnesses;
        settings.custom_runner.instances = self.runners;
        settings.web_app.instances = self.web_apps;
        settings.browser.instances = self.browsers;
    }
}

/// The settings "Save" writes: `draft` (the toggles and choices) with the typed text and
/// the instance lists, or why the text is invalid. An instance's problem is only a warning
/// under it: the instance is saved as typed.
pub fn build(draft: &Settings, texts: &Texts, lists: Lists) -> Result<Settings, FormError> {
    let mut edited = draft.clone();
    texts.apply(&mut edited)?;
    lists.store_in(&mut edited);
    Ok(edited)
}

/// Write what the dialog edits from `edited` into `current`, keeping the rest of `current`
/// (recent directories, SSH connections, ... may have changed while the dialog was open).
/// Destructured in full so a new settings field has to be placed on one side or the other.
pub fn apply_owned(edited: Settings, current: &mut Settings) {
    let Settings {
        terminal,
        sandbox,
        ai_harness,
        custom_runner,
        web_app,
        browser,
        notifications: _,
        appearance,
        prefix_mode,
        keybindings,
        daemon: _,
        ssh: _,
        llm,
        terminal_analyzer,
        github,
        global_default_application_id,
        recent_directories: _,
        debug,
        extra: _,
    } = edited;

    let TerminalSettings {
        font_size,
        font_family,
        cursor_style,
        cursor_blink,
        show_raw_chars,
        allow_osc52_clipboard,
        max_cols,
        instances,
        extra: _,
    } = terminal;
    let t = &mut current.terminal;
    (t.font_size, t.font_family, t.cursor_style, t.cursor_blink) = (font_size, font_family, cursor_style, cursor_blink);
    (t.show_raw_chars, t.allow_osc52_clipboard, t.max_cols, t.instances) =
        (show_raw_chars, allow_osc52_clipboard, max_cols, instances);

    let SandboxSettings { enabled_by_default, allow_network_by_default, extra: _ } = sandbox;
    current.sandbox.enabled_by_default = enabled_by_default;
    current.sandbox.allow_network_by_default = allow_network_by_default;

    current.ai_harness.instances = instances_of(ai_harness);
    current.custom_runner.instances = instances_of(custom_runner);
    current.web_app.instances = instances_of(web_app);
    current.browser.instances = instances_of(browser);

    let AppearanceSettings { theme, extra: _ } = appearance;
    current.appearance.theme = theme;

    let PrefixModeConfig { enabled: _, prefix_key, timeout_ms, extra: _ } = prefix_mode;
    current.prefix_mode.prefix_key = prefix_key;
    current.prefix_mode.timeout_ms = timeout_ms;

    let Keybindings { new_tab, close_tab, next_tab, prev_tab, open_settings, workspace_focus, extra: _ } = keybindings;
    let k = &mut current.keybindings;
    (k.new_tab, k.close_tab, k.next_tab) = (new_tab, close_tab, next_tab);
    (k.prev_tab, k.open_settings, k.workspace_focus) = (prev_tab, open_settings, workspace_focus);

    let LlmSettings { base_url, api_key, model, extra: _ } = llm;
    (current.llm.base_url, current.llm.api_key, current.llm.model) = (base_url, api_key, model);

    let TerminalAnalyzerSettings {
        provider,
        title_model,
        model,
        system_prompt,
        criteria,
        title_system_prompt,
        reasoning_effort,
        safe_paths,
        buffer_lines,
        idle_debounce,
        idle_debounce_unread,
        extra: _,
    } = terminal_analyzer;
    let a = &mut current.terminal_analyzer;
    (a.provider, a.title_model, a.model, a.system_prompt) = (provider, title_model, model, system_prompt);
    (a.title_system_prompt, a.reasoning_effort, a.safe_paths) = (title_system_prompt, reasoning_effort, safe_paths);
    (a.buffer_lines, a.idle_debounce, a.idle_debounce_unread) = (buffer_lines, idle_debounce, idle_debounce_unread);
    let ClassifierCriteria {
        working,
        safe_permission_requested,
        permission_request,
        completed,
        user_input_required,
        application_error,
        idle,
        extra: _,
    } = criteria;
    let c = &mut a.criteria;
    (c.working, c.safe_permission_requested, c.permission_request) = (working, safe_permission_requested, permission_request);
    (c.completed, c.user_input_required, c.application_error, c.idle) =
        (completed, user_input_required, application_error, idle);

    let GithubSettings { pat, autodetect_via_gh, extra: _ } = github;
    current.github.pat = pat;
    current.github.autodetect_via_gh = autodetect_via_gh;

    current.global_default_application_id = global_default_application_id;

    let DebugSettings { show_badge, extra: _ } = debug;
    current.debug.show_badge = show_badge;
}

/// The list of an instances section, without the keys it does not know.
fn instances_of<T>(Instances { instances, extra: _ }: Instances<T>) -> Vec<T> {
    instances
}

/// One entry of the "Default Application for New Worktrees" choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppChoice {
    pub id: String,
    pub label: String,
}

/// The applications new worktrees can open with: every registered one, given the edited
/// instance lists, by name (Electron lists `applications.values()`).
pub fn default_app_choices(settings: &Settings) -> Vec<AppChoice> {
    AppRegistry::from_settings(settings)
        .all()
        .map(|app| AppChoice { id: app.id.clone(), label: app.name.clone() })
        .collect()
}

/// What the choice shows for `id`. An id the native app has no application for (a web
/// app, or a removed variant) is kept and shown as such.
pub fn default_app_label(choices: &[AppChoice], id: &str) -> String {
    match choices.iter().find(|choice| choice.id == id) {
        Some(choice) => choice.label.clone(),
        None => format!("{id} (not available)"),
    }
}

/// Whether sandboxed terminals can run here (Electron's `sandbox:isAvailable`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxSupport {
    Available,
    /// Linux without bubblewrap on `PATH`.
    MissingBwrap,
}

impl SandboxSupport {
    /// macOS always has `sandbox-exec`; Linux needs `bwrap` on `path` (the `PATH` value).
    pub fn detect(os: Os, path: Option<&OsStr>) -> SandboxSupport {
        match os {
            Os::MacOs => SandboxSupport::Available,
            Os::Linux => {
                let found = path.is_some_and(|path| std::env::split_paths(path).any(|dir| is_executable(&dir.join("bwrap"))));
                if found { SandboxSupport::Available } else { SandboxSupport::MissingBwrap }
            }
        }
    }

    pub fn is_available(self) -> bool {
        match self {
            SandboxSupport::Available => true,
            SandboxSupport::MissingBwrap => false,
        }
    }

    /// The warning Electron shows above the sandbox options.
    pub fn warning(self) -> Option<&'static str> {
        match self {
            SandboxSupport::Available => None,
            SandboxSupport::MissingBwrap => {
                Some("Sandbox not available on this system. Install bubblewrap (bwrap) to enable.")
            }
        }
    }
}

/// A probe like `which`: anything unreadable counts as absent.
fn is_executable(path: &Path) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => metadata.is_file() && metadata.permissions().mode() & 0o111 != 0,
        Err(_) => false,
    }
}

/// An id for a new instance: Electron's `<prefix>-<Date.now()>`, moved past ids in use.
/// A pressed key as the DOM's `KeyboardEvent.key` names it, from GPUI's key name and the
/// character it types (Electron's keybinding recorder stores DOM names).
pub fn dom_key(key: &str, key_char: Option<&str>) -> String {
    let named = match key {
        "tab" => "Tab",
        "enter" => "Enter",
        "escape" => "Escape",
        "space" => " ",
        "backspace" => "Backspace",
        "delete" => "Delete",
        "insert" => "Insert",
        "up" => "ArrowUp",
        "down" => "ArrowDown",
        "left" => "ArrowLeft",
        "right" => "ArrowRight",
        "home" => "Home",
        "end" => "End",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_owned();
    }
    if let Some(n) = key.strip_prefix('f').filter(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit())) {
        return format!("F{n}");
    }
    match key_char {
        Some(typed) if typed.chars().count() == 1 => typed.to_owned(),
        _ => key.to_owned(),
    }
}

/// `handleKeyDown` recording an action key: one key, lower-cased if it is a character.
pub fn recorded_action_key(dom_key: &str) -> String {
    if dom_key.chars().count() == 1 { dom_key.to_lowercase() } else { dom_key.to_owned() }
}

/// `handleKeyDown` recording the prefix key: `CommandOrControl` (Cmd or Ctrl), `Alt`,
/// `Shift`, then the key, upper-cased if it is a character.
pub fn recorded_prefix_key(dom_key: &str, command_or_control: bool, alt: bool, shift: bool) -> String {
    let mut parts: Vec<String> = Vec::new();
    if command_or_control {
        parts.push("CommandOrControl".to_owned());
    }
    if alt {
        parts.push("Alt".to_owned());
    }
    if shift {
        parts.push("Shift".to_owned());
    }
    parts.push(if dom_key.chars().count() == 1 { dom_key.to_uppercase() } else { dom_key.to_owned() });
    parts.join("+")
}

/// `formatKeybinding`: an accelerator as the prefix key button shows it.
pub fn format_prefix_key(binding: &str, os: Os) -> String {
    if binding.is_empty() {
        return "(none)".to_owned();
    }
    format_accelerator(binding, os)
}

pub fn new_id(prefix: &str, now_ms: u128, taken: impl Fn(&str) -> bool) -> String {
    (now_ms..)
        .map(|ms| format!("{prefix}-{ms}"))
        .find(|id| !taken(id))
        .expect("an unbounded range has a free id")
}

#[cfg(test)]
mod tests {
    use treeterm_core::settings::{ClassifierProvider, CursorStyle, ReasoningEffort, Theme};

    use crate::settings_view::instance::InstanceForm;
    use super::*;

    #[test]
    fn recorded_keys_follow_electrons_recorder() {
        assert_eq!(dom_key("tab", None), "Tab");
        assert_eq!(dom_key("f2", None), "F2");
        assert_eq!(dom_key("up", None), "ArrowUp");
        assert_eq!(dom_key("a", Some("A")), "A");
        assert_eq!(dom_key(",", Some(",")), ",");
        assert_eq!(recorded_action_key("C"), "c");
        assert_eq!(recorded_action_key("Tab"), "Tab");
        assert_eq!(recorded_prefix_key("b", true, false, true), "CommandOrControl+Shift+B");
        assert_eq!(recorded_prefix_key("F2", false, true, false), "Alt+F2");
        assert_eq!(format_prefix_key("Control+B", Os::MacOs), "Ctrl + B");
        assert_eq!(format_prefix_key("CommandOrControl+K", Os::MacOs), "Cmd + K");
        assert_eq!(format_prefix_key("CommandOrControl+K", Os::Linux), "Ctrl + K");
        assert_eq!(format_prefix_key("", Os::MacOs), "(none)");
    }

    fn texts() -> Texts {
        Texts::from_settings(&Settings::default())
    }

    #[test]
    fn untouched_texts_build_the_settings_they_came_from() {
        let mut settings = Settings::default();
        settings.terminal.font_size = 13.5;
        settings.terminal_analyzer.safe_paths = vec!["/tmp".into(), "/var/tmp".into()];
        let texts = Texts::from_settings(&settings);
        assert_eq!(texts.font_size, "13.5");
        assert_eq!(texts.safe_paths, "/tmp, /var/tmp");
        let mut applied = settings.clone();
        texts.apply(&mut applied).unwrap();
        assert_eq!(applied, settings);
    }

    #[test]
    fn typed_text_is_parsed_into_settings() {
        let mut texts = texts();
        texts.font_size = " 16 ".into();
        texts.max_cols = "120".into();
        texts.safe_paths = "/tmp, ,/work ,".into();
        texts.idle_debounce = "0".into();
        texts.prefix_timeout = "900".into();
        texts.criteria[0] = "busy".into();
        texts.action_keys[5] = "W".into();
        texts.llm_api_key = "sk-1".into();
        let mut settings = Settings::default();
        texts.apply(&mut settings).unwrap();
        assert_eq!(settings.terminal.font_size, 16.);
        assert_eq!(settings.terminal.max_cols, 120);
        assert_eq!(settings.terminal_analyzer.safe_paths, ["/tmp", "/work"]);
        assert_eq!(settings.terminal_analyzer.idle_debounce, Duration::ZERO);
        assert_eq!(settings.terminal_analyzer.criteria.working, "busy");
        assert_eq!(settings.prefix_mode.timeout_ms, 900);
        assert_eq!(settings.keybindings.workspace_focus, "W");
        assert_eq!(settings.llm.api_key, "sk-1");
    }

    /// `handleSave` saves the recorded keys untouched: Space stays ` `, and a key the app
    /// cannot match (it only disables itself) is saved too.
    #[test]
    fn keys_are_saved_as_recorded() {
        let mut texts = texts();
        texts.prefix_key = "Hyper+B".into();
        texts.action_keys[0] = " ".into();
        texts.action_keys[1] = "CapsLock".into();
        let mut settings = Settings::default();
        texts.apply(&mut settings).unwrap();
        assert_eq!(settings.prefix_mode.prefix_key, "Hyper+B");
        assert_eq!(settings.keybindings.new_tab, " ");
        assert_eq!(settings.keybindings.close_tab, "CapsLock");

        texts.prefix_key = String::new();
        texts.apply(&mut settings).unwrap();
        assert_eq!(settings.prefix_mode.prefix_key, "");
    }

    /// The number fields step within their `min`/`max` and by their `step` from `min`, as
    /// Electron's `<input type="number">`s do; an empty field steps into range.
    #[test]
    fn the_number_fields_step_as_electrons_inputs() {
        use gpui_kit::base::StepAction::{self, Decrement, Increment};

        use crate::number_field::spin;

        let cases: [(NumberSpec, &str, StepAction, Option<&str>); 19] = [
            (FONT_SIZE, "14", Increment, Some("15")),
            (FONT_SIZE, "14", Decrement, Some("13")),
            (FONT_SIZE, "32", Increment, None),
            (FONT_SIZE, "8", Decrement, None),
            (FONT_SIZE, "", Increment, Some("8")),
            (FONT_SIZE, "5", Increment, Some("8")),
            (FONT_SIZE, "33", Decrement, Some("32")),
            (MAX_COLS, "160", Increment, Some("161")),
            (MAX_COLS, "40", Decrement, None),
            (MAX_COLS, "2000", Decrement, Some("1000")),
            (MAX_COLS, "2000", Increment, None),
            (BUFFER_LINES, "100", Increment, None),
            (BUFFER_LINES, "1", Decrement, None),
            (IDLE_DEBOUNCE, "2050", Increment, Some("2100")),
            (IDLE_DEBOUNCE, "2050", Decrement, Some("2000")),
            (IDLE_DEBOUNCE, "0", Decrement, None),
            (IDLE_DEBOUNCE, "", Increment, Some("100")),
            (IDLE_DEBOUNCE_UNREAD, "15000", Increment, Some("15100")),
            (PREFIX_TIMEOUT, "450", Increment, Some("500")),
        ];
        for (spec, text, action, stepped) in cases {
            assert_eq!(spin(text, action, spec).as_deref(), stepped, "{spec:?} {text:?} {action:?}");
        }
    }

    /// `parseInt(e.target.value) || 1500` over what `<input type="number">` reports.
    #[test]
    fn the_prefix_timeout_is_read_as_electron_reads_it() {
        for (text, ms) in [
            ("800", 800.),
            ("", 1500.),
            ("0", 1500.),
            ("-0", 1500.),
            ("1500.7", 1500.),
            ("1e3", 1.),
            ("-5", -5.),
            (".5", 1500.),
            ("abc", 1500.),
            ("5.", 1500.),
            ("+5", 1500.),
            ("99999999999999999999", 1e20),
        ] {
            assert_eq!(prefix_timeout_ms(text), ms, "{text:?}");
        }
    }

    /// React rewrites the controlled field whenever its value is not the stored number.
    #[test]
    fn the_prefix_timeout_field_shows_what_is_stored() {
        for (text, shown) in [
            ("800", None),
            ("1500", None),
            ("-5", None),
            ("0800", None),
            ("", Some("1500")),
            ("0", Some("1500")),
            ("-", Some("1500")),
            ("1500.", Some("1500")),
            ("1500.7", Some("1500")),
            ("1e3", Some("1")),
        ] {
            assert_eq!(prefix_timeout_field(text).as_deref(), shown, "{text:?}");
        }
    }

    /// Save never refuses the timeout, and stores a pasted negative one as typed (prefix mode
    /// then ends at once), which Settings shows again when it reopens.
    #[test]
    fn the_prefix_timeout_is_saved_as_electron_stores_it() {
        let mut settings = Settings::default();
        let cases = [("", 1500), ("0", 1500), ("1500.7", 1500), ("5", 5), ("-5", -5), ("9000", 9000)];
        for (text, ms) in cases {
            let mut texts = texts();
            texts.prefix_timeout = text.into();
            texts.apply(&mut settings).unwrap();
            assert_eq!(settings.prefix_mode.timeout_ms, ms, "{text:?}");
            assert_eq!(Texts::from_settings(&settings).prefix_timeout, ms.to_string(), "{text:?}");
        }
        let mut texts = texts();
        texts.prefix_timeout = "99999999999999999999".into();
        texts.apply(&mut settings).unwrap();
        assert_eq!(settings.prefix_mode.timeout_ms, i64::MAX);
    }

    /// An edit to the texts, the section it belongs to, and how the error starts.
    type InvalidCase = (fn(&mut Texts), Section, &'static str);

    #[test]
    fn invalid_text_names_the_field_and_its_section_and_writes_nothing() {
        let cases: [InvalidCase; 6] = [
            (|t| t.font_size = "big".into(), Section::Terminal, "Font size must be a positive number"),
            (|t| t.font_size = "0".into(), Section::Terminal, "Font size must be a positive number"),
            (|t| t.max_cols = "1".into(), Section::Terminal, "Max columns must be a whole number of at least 2"),
            (|t| t.buffer_lines = "0".into(), Section::Llm, "Buffer lines must be a whole number of at least 1"),
            (|t| t.idle_debounce = "-5".into(), Section::Llm, "Idle debounce must be a whole number of milliseconds"),
            (
                |t| t.idle_debounce_unread = "1.5".into(),
                Section::Llm,
                "Idle debounce when unread must be a whole number of milliseconds",
            ),
        ];
        for (edit, section, message) in cases {
            let mut texts = texts();
            texts.github_pat = "ghp_x".into();
            edit(&mut texts);
            let mut settings = Settings::default();
            let error = texts.apply(&mut settings).unwrap_err();
            assert_eq!(error.section, section, "{message}");
            assert!(error.message.starts_with(message), "{} does not start with {message}", error.message);
            assert_eq!(settings, Settings::default(), "{message}");
        }
    }

    fn lists(settings: &Settings) -> Lists {
        Lists {
            profiles: settings.terminal.instances.clone(),
            harnesses: settings.ai_harness.instances.clone(),
            runners: settings.custom_runner.instances.clone(),
            web_apps: settings.web_app.instances.clone(),
            browsers: settings.browser.instances.clone(),
        }
    }

    #[test]
    fn build_combines_choices_text_and_lists() {
        let mut draft = Settings::default();
        draft.terminal.cursor_style = CursorStyle::Bar;
        let mut lists = lists(&draft);
        lists.browsers.push(BrowserInstance::added("browser-1".into()));
        let mut texts = texts();
        texts.font_family = "  Iosevka  ".into();
        let edited = build(&draft, &texts, lists).unwrap();
        assert_eq!(edited.terminal.cursor_style, CursorStyle::Bar);
        assert_eq!(edited.terminal.font_family, "Iosevka");
        assert_eq!(edited.browser.instances.len(), 1);
        assert_eq!(edited.ai_harness.instances, draft.ai_harness.instances);
    }

    #[test]
    fn build_keeps_an_instance_with_a_url_problem() {
        let draft = Settings::default();
        let mut lists = lists(&draft);
        let mut grafana = BrowserInstance::added("browser-1".into());
        grafana.name = "Grafana".into();
        grafana.url = "https://grafana.example.com/".into();
        assert!(grafana.problem().is_some());
        lists.browsers.push(grafana.clone());
        // Electron only shows the problem under the instance; Save still writes it.
        let edited = build(&draft, &texts(), lists).unwrap();
        assert_eq!(edited.browser.instances, vec![grafana]);
    }

    #[test]
    fn apply_owned_writes_the_dialog_fields_and_keeps_the_others() {
        let mut edited = Settings::default();
        edited.terminal.show_raw_chars = true;
        edited.terminal.instances.push(TerminalInstance::added("term-1".into()));
        edited.sandbox.enabled_by_default = true;
        edited.custom_runner.instances.push(CustomRunnerInstance::added("runner-1".into()));
        edited.web_app.instances.push(WebAppInstance::added("webapp-1".into()));
        edited.appearance.theme = Theme::Light;
        edited.prefix_mode.prefix_key = "Alt+A".into();
        edited.keybindings.prev_tab = "b".into();
        edited.llm.model = "m".into();
        edited.terminal_analyzer.provider = ClassifierProvider::Classifier;
        edited.terminal_analyzer.reasoning_effort = ReasoningEffort::High;
        edited.terminal_analyzer.criteria.idle = "quiet".into();
        edited.github.autodetect_via_gh = false;
        edited.global_default_application_id = "filesystem".into();
        edited.debug.show_badge = true;
        // Not the dialog's: must not reach `current`.
        edited.recent_directories = vec!["/edited".into()];
        edited.prefix_mode.enabled = false;
        edited.notifications.sound_enabled = false;
        edited.extra.insert("fromDraft".into(), true.into());

        let mut current = Settings { recent_directories: vec!["/elsewhere".into()], ..Settings::default() };
        current.daemon.scrollback_lines = 5;
        current.terminal.extra.insert("newer".into(), 1.into());
        apply_owned(edited.clone(), &mut current);

        assert_eq!(current.recent_directories, ["/elsewhere"]);
        assert_eq!(current.daemon.scrollback_lines, 5);
        assert!(current.prefix_mode.enabled);
        assert!(current.notifications.sound_enabled);
        assert!(!current.extra.contains_key("fromDraft"));
        assert_eq!(current.terminal.extra.get("newer"), Some(&1.into()));

        // Everything else now matches `edited`.
        let mut expected = edited;
        expected.recent_directories = current.recent_directories.clone();
        expected.daemon = current.daemon.clone();
        expected.prefix_mode.enabled = true;
        expected.notifications.sound_enabled = true;
        expected.extra = current.extra.clone();
        expected.terminal.extra = current.terminal.extra.clone();
        assert_eq!(current, expected);
    }

    #[test]
    fn default_app_choices_follow_the_edited_lists() {
        let mut settings = Settings::default();
        let mut profile = TerminalInstance::added("term-1".into());
        profile.name = "Zsh".into();
        settings.terminal.instances.push(profile);
        let choices = default_app_choices(&settings);
        let ids: Vec<&str> = choices.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "terminal",
                "ghostty-terminal",
                "filesystem",
                "review",
                "editor",
                "comments",
                "chat",
                "system-prompt-debugger",
                "analyzer-history",
                "workspace-settings",
                "github",
                "tty-list",
                "ssh-upload",
                "terminal-term-1",
                "aiharness-claude"
            ]
        );
        assert_eq!(default_app_label(&choices, "terminal"), "Terminal");
        assert_eq!(default_app_label(&choices, "ghostty-terminal"), "Terminal (Ghostty)");
        assert_eq!(default_app_label(&choices, "terminal-term-1"), "Zsh");
        assert_eq!(default_app_label(&choices, "webapp-next"), "webapp-next (not available)");
    }

    #[test]
    fn sandbox_needs_bwrap_on_linux_only() {
        assert_eq!(SandboxSupport::detect(Os::MacOs, None), SandboxSupport::Available);
        assert_eq!(SandboxSupport::detect(Os::Linux, None), SandboxSupport::MissingBwrap);

        let dir = std::env::temp_dir().join(format!("treeterm-settings-bwrap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = std::env::join_paths([Path::new("/nonexistent"), &dir]).unwrap();
        assert_eq!(SandboxSupport::detect(Os::Linux, Some(&path)), SandboxSupport::MissingBwrap);
        let bwrap = dir.join("bwrap");
        std::fs::write(&bwrap, "").unwrap();
        assert_eq!(SandboxSupport::detect(Os::Linux, Some(&path)), SandboxSupport::MissingBwrap, "not executable");
        std::fs::set_permissions(&bwrap, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(SandboxSupport::detect(Os::Linux, Some(&path)), SandboxSupport::Available);
        std::fs::remove_dir_all(&dir).unwrap();

        assert!(SandboxSupport::Available.is_available());
        assert_eq!(SandboxSupport::Available.warning(), None);
        assert!(!SandboxSupport::MissingBwrap.is_available());
        assert!(SandboxSupport::MissingBwrap.warning().unwrap().contains("bwrap"));
    }

    #[test]
    fn new_ids_skip_ids_in_use() {
        assert_eq!(new_id("term", 7, |_| false), "term-7");
        assert_eq!(new_id("ai", 7, |id| id == "ai-7" || id == "ai-8"), "ai-9");
    }

    #[test]
    fn sections_are_listed_once_each() {
        for section in Section::ALL {
            assert_eq!(Section::ALL.iter().filter(|s| **s == section).count(), 1, "{}", section.label());
        }
    }
}
