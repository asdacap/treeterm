//! The dialog's logic, free of GPUI: its sections, what is typed into it, how that is
//! validated, and which parts of `Settings` a save writes.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use treeterm_core::keybinding::{Key, Os, PrefixAction, Shortcut};
use treeterm_core::settings::{
    AiHarnessInstance, AppearanceSettings, BrowserInstance, ClassifiedState, ClassifierCriteria,
    CustomRunnerInstance, DebugSettings, GithubSettings, Instances, Keybindings, LlmSettings,
    PrefixModeConfig, SandboxSettings, Settings, TerminalAnalyzerSettings, TerminalInstance,
    TerminalSettings, WebAppInstance,
};
use treeterm_model::apps::AppRegistry;

use super::instance::InstanceForm;

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
            prefix_timeout: settings.prefix_mode.timeout.as_millis().to_string(),
            action_keys: PrefixAction::ALL.map(|action| action_key(&settings.keybindings, action).to_owned()),
            github_pat: settings.github.pat.clone(),
        }
    }

    /// Parse the fields into `settings`; nothing is written unless all of them are valid.
    pub fn apply(&self, settings: &mut Settings, os: Os) -> Result<(), FormError> {
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
        let prefix_key = self.prefix_key.trim();
        if let Err(error) = Shortcut::parse(prefix_key, os) {
            return Err(FormError::new(Section::Keybindings, format!("Prefix key “{prefix_key}”: {error}")));
        }
        let timeout = whole_number::<u64>(&self.prefix_timeout, 1).ok_or_else(|| {
            FormError::new(Section::Keybindings, "Prefix timeout must be a positive whole number of milliseconds")
        })?;
        for (action, key) in PrefixAction::ALL.iter().zip(&self.action_keys) {
            let key = key.trim();
            if let Err(error) = Key::parse(key) {
                return Err(FormError::new(Section::Keybindings, format!("{} key “{key}”: {error}", action.label())));
            }
        }

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
        settings.prefix_mode.prefix_key = prefix_key.to_owned();
        settings.prefix_mode.timeout = Duration::from_millis(timeout);
        for (action, key) in PrefixAction::ALL.iter().zip(&self.action_keys) {
            *action_key_mut(&mut settings.keybindings, *action) = key.trim().to_owned();
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

/// Electron's safe-paths field: comma-separated, trimmed, empty entries dropped.
pub fn parse_safe_paths(text: &str) -> Vec<String> {
    text.split(',').map(str::trim).filter(|path| !path.is_empty()).map(str::to_owned).collect()
}

pub fn action_key(keybindings: &Keybindings, action: PrefixAction) -> &str {
    match action {
        PrefixAction::NewTab => &keybindings.new_tab,
        PrefixAction::CloseTab => &keybindings.close_tab,
        PrefixAction::NextTab => &keybindings.next_tab,
        PrefixAction::PrevTab => &keybindings.prev_tab,
        PrefixAction::OpenSettings => &keybindings.open_settings,
        PrefixAction::WorkspaceFocus => &keybindings.workspace_focus,
    }
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
/// the instance lists, or why they are invalid.
pub fn build(draft: &Settings, texts: &Texts, lists: Lists, os: Os) -> Result<Settings, FormError> {
    let mut edited = draft.clone();
    texts.apply(&mut edited, os)?;
    check(&lists.profiles)?;
    check(&lists.harnesses)?;
    check(&lists.runners)?;
    check(&lists.web_apps)?;
    check(&lists.browsers)?;
    lists.store_in(&mut edited);
    Ok(edited)
}

fn check<T: InstanceForm>(instances: &[T]) -> Result<(), FormError> {
    match instances.iter().find_map(|instance| instance.problem().map(|problem| (instance, problem))) {
        Some((instance, problem)) => Err(FormError::new(T::SECTION, format!("{}: {problem}", instance.name()))),
        None => Ok(()),
    }
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

    let PrefixModeConfig { enabled: _, prefix_key, timeout, extra: _ } = prefix_mode;
    current.prefix_mode.prefix_key = prefix_key;
    current.prefix_mode.timeout = timeout;

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

/// The applications new worktrees can open with: those of the new-tab menu, given the
/// edited instance lists.
pub fn default_app_choices(settings: &Settings) -> Vec<AppChoice> {
    AppRegistry::from_settings(settings)
        .new_tab_menu()
        .map(|app| AppChoice { id: app.id.clone(), label: format!("{}  {}", app.icon, app.name) })
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
pub fn new_id(prefix: &str, now_ms: u128, taken: impl Fn(&str) -> bool) -> String {
    (now_ms..)
        .map(|ms| format!("{prefix}-{ms}"))
        .find(|id| !taken(id))
        .expect("an unbounded range has a free id")
}

#[cfg(test)]
mod tests {
    use treeterm_core::settings::{ClassifierProvider, CursorStyle, ReasoningEffort, Theme};

    use super::*;

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
        texts.apply(&mut applied, Os::MacOs).unwrap();
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
        texts.action_keys[5] = " W ".into();
        texts.llm_api_key = "sk-1".into();
        let mut settings = Settings::default();
        texts.apply(&mut settings, Os::Linux).unwrap();
        assert_eq!(settings.terminal.font_size, 16.);
        assert_eq!(settings.terminal.max_cols, 120);
        assert_eq!(settings.terminal_analyzer.safe_paths, ["/tmp", "/work"]);
        assert_eq!(settings.terminal_analyzer.idle_debounce, Duration::ZERO);
        assert_eq!(settings.terminal_analyzer.criteria.working, "busy");
        assert_eq!(settings.prefix_mode.timeout, Duration::from_millis(900));
        assert_eq!(settings.keybindings.workspace_focus, "W");
        assert_eq!(settings.llm.api_key, "sk-1");
    }

    /// An edit to the texts, the section it belongs to, and how the error starts.
    type InvalidCase = (fn(&mut Texts), Section, &'static str);

    #[test]
    fn invalid_text_names_the_field_and_its_section_and_writes_nothing() {
        let cases: [InvalidCase; 9] = [
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
            (
                |t| t.prefix_timeout = "0".into(),
                Section::Keybindings,
                "Prefix timeout must be a positive whole number of milliseconds",
            ),
            (|t| t.prefix_key = "Hyper+B".into(), Section::Keybindings, "Prefix key “Hyper+B”"),
            (|t| t.action_keys[1] = String::new(), Section::Keybindings, "Close Tab key “”"),
        ];
        for (edit, section, message) in cases {
            let mut texts = texts();
            texts.github_pat = "ghp_x".into();
            edit(&mut texts);
            let mut settings = Settings::default();
            let error = texts.apply(&mut settings, Os::MacOs).unwrap_err();
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
        let edited = build(&draft, &texts, lists, Os::MacOs).unwrap();
        assert_eq!(edited.terminal.cursor_style, CursorStyle::Bar);
        assert_eq!(edited.terminal.font_family, "Iosevka");
        assert_eq!(edited.browser.instances.len(), 1);
        assert_eq!(edited.ai_harness.instances, draft.ai_harness.instances);
    }

    #[test]
    fn build_refuses_an_instance_with_a_problem() {
        let draft = Settings::default();
        let mut lists = lists(&draft);
        let mut grafana = BrowserInstance::added("browser-1".into());
        grafana.name = "Grafana".into();
        grafana.url = "https://grafana.example.com/".into();
        lists.browsers.push(grafana);
        let error = build(&draft, &texts(), lists, Os::MacOs).unwrap_err();
        assert_eq!(error.section, Section::Browsers);
        assert_eq!(
            error.message,
            "Grafana: URL must be http(s)://localhost or 127.0.0.1: https://grafana.example.com/"
        );
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
        assert_eq!(ids, ["terminal", "filesystem", "review", "terminal-term-1", "aiharness-claude"]);
        assert_eq!(default_app_label(&choices, "terminal-term-1"), ">  Zsh");
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
