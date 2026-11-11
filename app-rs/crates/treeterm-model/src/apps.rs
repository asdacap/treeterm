//! The applications a tab can run. Port of the registry in `src/renderer/store/app.ts`.
//!
//! Ids are shared with the Electron app (`terminal`, `terminal-<instance>`, ...), so a tab
//! created by either app opens in the other. Applications the native app does not have yet
//! are absent from the registry: their tabs are kept untouched (never closed or rewritten).

use serde_json::{Value, json};
use treeterm_core::settings::Settings;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppKind {
    /// A shell (or a startup command) in a daemon PTY.
    Terminal { startup_command: Option<String> },
    /// An AI coding agent running in a PTY, read by the LLM analyzer.
    AiHarness {
        command: String,
        background: String,
        keep_on_exit: bool,
        enable_sandbox: bool,
        allow_network: bool,
    },
    /// A command template run in a PTY (`{{workspace_path}}` expanded at start).
    CustomRunner { command_template: String },
    /// The file browser (`filesystem`).
    Files,
    /// One file being edited (`editor`); opened from the file viewer.
    Editor,
    /// What a worktree changed (`review`).
    Review,
    /// A local web page (`browser-<instance>`); only localhost URLs.
    Browser { url: String },
    /// A dev server run in a terminal with `$PORT` set, and its page (`webapp-<instance>`).
    WebApp { command: String, keep_on_exit: bool },
    /// A view over the workspace, its session or the LLM (no PTY, no page).
    Tool(ToolApp),
}

/// The built-in tool tabs (`tool_tabs` holds their runtime side).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolApp {
    /// The branch's pull request (`github`).
    GitHub,
    /// A tab's activity transitions and analyzer calls (`analyzer-history`).
    AnalyzerHistory,
    /// The workspace's review comments (`comments`).
    Comments,
    /// The daemon's PTYs started in this workspace (`tty-list`).
    TtyList,
    /// The workspace's own settings (`workspace-settings`).
    WorkspaceSettings,
    /// A streaming LLM chat (`chat`).
    Chat,
    /// Try the analyzer and title prompts on a buffer (`system-prompt-debugger`).
    PromptDebugger,
    /// Copy a local file to the ssh host (`ssh-upload`).
    SshUpload,
}

impl ToolApp {
    /// `createInitialState`.
    pub fn initial_state(self) -> Value {
        match self {
            ToolApp::Chat => json!({ "messages": [] }),
            ToolApp::GitHub
            | ToolApp::AnalyzerHistory
            | ToolApp::Comments
            | ToolApp::TtyList
            | ToolApp::WorkspaceSettings
            | ToolApp::PromptDebugger
            | ToolApp::SshUpload => json!({}),
        }
    }
}

impl AppKind {
    /// Whether tabs of this application run in a PTY.
    pub fn is_terminal(&self) -> bool {
        match self {
            AppKind::Terminal { .. }
            | AppKind::AiHarness { .. }
            | AppKind::CustomRunner { .. }
            | AppKind::WebApp { .. } => true,
            AppKind::Files | AppKind::Editor | AppKind::Review | AppKind::Browser { .. } | AppKind::Tool(_) => false,
        }
    }

    /// Whether the new-tab menu offers it on a local or remote session
    /// (`isAppAvailableForConnection`): upload is for ssh sessions only.
    pub fn available_on(&self, remote: bool) -> bool {
        match self {
            AppKind::Tool(ToolApp::SshUpload) => remote,
            AppKind::Tool(
                ToolApp::GitHub
                | ToolApp::AnalyzerHistory
                | ToolApp::Comments
                | ToolApp::TtyList
                | ToolApp::WorkspaceSettings
                | ToolApp::Chat
                | ToolApp::PromptDebugger,
            )
            | AppKind::Terminal { .. }
            | AppKind::AiHarness { .. }
            | AppKind::CustomRunner { .. }
            | AppKind::Files
            | AppKind::Editor
            | AppKind::Review
            | AppKind::Browser { .. }
            | AppKind::WebApp { .. } => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDef {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub kind: AppKind,
    pub can_close: bool,
    pub show_in_new_tab_menu: bool,
    /// Opened automatically in new workspaces.
    pub is_default: bool,
}

impl AppDef {
    /// `createInitialState` of the application.
    pub fn initial_state(&self) -> Value {
        let handle = uuid::Uuid::new_v4().to_string();
        match &self.kind {
            AppKind::Terminal { .. } | AppKind::CustomRunner { .. } => json!({
                "ptyId": null,
                "ptyHandle": handle,
                "keepOnExit": false,
                "idleDetectorDisabled": false,
                "widthLimitDisabled": false,
            }),
            AppKind::WebApp { keep_on_exit, .. } => json!({
                "ptyId": null,
                "ptyHandle": handle,
                "keepOnExit": keep_on_exit,
                "idleDetectorDisabled": false,
                "widthLimitDisabled": false,
                "port": { "status": "unassigned" },
                "localPort": { "status": "unassigned" },
                "path": "",
            }),
            AppKind::Files => json!({ "selectedPath": null, "expandedDirs": [] }),
            AppKind::Review => json!({ "viewMode": "committed" }),
            AppKind::Browser { .. } => json!({ "localPort": { "status": "unassigned" }, "path": "" }),
            AppKind::Editor => json!({
                "status": "ready", "filePath": "", "originalContent": "", "currentContent": "",
                "language": "plaintext", "isDirty": false, "viewMode": "editor"
            }),
            AppKind::AiHarness {
                keep_on_exit,
                enable_sandbox,
                allow_network,
                ..
            } => json!({
                "ptyId": null,
                "ptyHandle": handle,
                "keepOnExit": keep_on_exit,
                "sandbox": { "enabled": enable_sandbox, "allowNetwork": allow_network, "allowedPaths": [] },
                "autoApprove": false,
                "idleDetectorDisabled": false,
                "widthLimitDisabled": false,
            }),
            AppKind::Tool(tool) => tool.initial_state(),
        }
    }

    /// What a PTY-backed tab runs on start, for a workspace at `workspace_path`.
    pub fn startup_command(&self, workspace_path: &str) -> Option<String> {
        match &self.kind {
            AppKind::Terminal { startup_command } => startup_command.clone(),
            AppKind::AiHarness { command, .. } => Some(command.clone()),
            AppKind::CustomRunner { command_template } => {
                Some(command_template.replace("{{workspace_path}}", workspace_path))
            }
            // Needs its port first: see `web_app::resolve_command`.
            AppKind::WebApp { .. } => None,
            AppKind::Files | AppKind::Editor | AppKind::Review | AppKind::Browser { .. } | AppKind::Tool(_) => None,
        }
    }

    /// The terminal background a tab of this application uses, as `#rrggbb`, if not the default.
    pub fn background(&self) -> Option<&str> {
        match &self.kind {
            AppKind::AiHarness { background, .. } => Some(background),
            AppKind::Terminal { .. }
            | AppKind::CustomRunner { .. }
            | AppKind::Files
            | AppKind::Editor
            | AppKind::Review
            | AppKind::Browser { .. }
            | AppKind::WebApp { .. }
            | AppKind::Tool(_) => None,
        }
    }
}

/// Whether a page may load: http(s) on localhost or 127.0.0.1 only (`isAllowedWebviewUrl`).
pub fn is_allowed_webview_url(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once("://") else {
        return false;
    };
    if scheme != "http" && scheme != "https" {
        return false;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    let host_port = authority.rsplit('@').next().unwrap_or_default();
    let host = match host_port.strip_prefix('[') {
        Some(_) => return false, // IPv6 literals are not on the list
        None => host_port.split(':').next().unwrap_or_default(),
    };
    matches!(host.to_ascii_lowercase().as_str(), "localhost" | "127.0.0.1")
}

pub struct AppRegistry {
    apps: Vec<AppDef>,
}

impl AppRegistry {
    /// Built-ins first, then the variants from settings, in Electron's registration order.
    pub fn from_settings(settings: &Settings) -> Self {
        let terminal = |id: &str, show: bool| AppDef {
            id: id.to_owned(),
            name: "Terminal".to_owned(),
            icon: ">".to_owned(),
            kind: AppKind::Terminal {
                startup_command: None,
            },
            can_close: true,
            show_in_new_tab_menu: show,
            is_default: show,
        };
        let mut apps = vec![
            terminal("terminal", true),
            // The Electron app's alternate terminal frontend; natively it is the same terminal.
            terminal("ghostty-terminal", false),
            AppDef {
                id: "filesystem".to_owned(),
                name: "Files".to_owned(),
                icon: "\u{1F4C2}".to_owned(),
                kind: AppKind::Files,
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: true,
            },
            AppDef {
                id: "review".to_owned(),
                name: "Review".to_owned(),
                icon: "\u{1F4CB}".to_owned(),
                kind: AppKind::Review,
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: false,
            },
            AppDef {
                id: "editor".to_owned(),
                name: "Editor".to_owned(),
                icon: "\u{270F}".to_owned(),
                kind: AppKind::Editor,
                can_close: true,
                show_in_new_tab_menu: false,
                is_default: false,
            },
        ];
        // The tool tabs, in Electron's registration order.
        let tool = |id: &str, name: &str, icon: &str, tool: ToolApp, show: bool| AppDef {
            id: id.to_owned(),
            name: name.to_owned(),
            icon: icon.to_owned(),
            kind: AppKind::Tool(tool),
            can_close: true,
            show_in_new_tab_menu: show,
            is_default: false,
        };
        apps.extend([
            tool("comments", "Comments", "\u{1F4AC}", ToolApp::Comments, true),
            tool("chat", "Chat", "M", ToolApp::Chat, true),
            tool("system-prompt-debugger", "System Prompt Debugger", "\u{1F52C}", ToolApp::PromptDebugger, true),
            tool("analyzer-history", "Activity History", "\u{1F4CA}", ToolApp::AnalyzerHistory, false),
            tool("workspace-settings", "Settings", "\u{2699}", ToolApp::WorkspaceSettings, false),
            tool("github", "GitHub", "\u{1F419}", ToolApp::GitHub, true),
            tool("tty-list", "TTYs", "\u{2630}", ToolApp::TtyList, true),
            tool("ssh-upload", "Upload", "\u{2B06}", ToolApp::SshUpload, true),
        ]);
        for instance in &settings.terminal.instances {
            apps.push(AppDef {
                id: format!("terminal-{}", instance.id),
                name: instance.name.clone(),
                icon: instance.icon.clone(),
                kind: AppKind::Terminal {
                    startup_command: (!instance.startup_command.is_empty())
                        .then(|| instance.startup_command.clone()),
                },
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: instance.is_default,
            });
        }
        for instance in &settings.ai_harness.instances {
            apps.push(AppDef {
                id: format!("aiharness-{}", instance.id),
                name: instance.name.clone(),
                icon: instance.icon.clone(),
                kind: AppKind::AiHarness {
                    command: instance.command.clone(),
                    background: instance.background_color.clone(),
                    keep_on_exit: instance.keep_on_exit,
                    enable_sandbox: instance.enable_sandbox,
                    allow_network: instance.allow_network,
                },
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: instance.is_default,
            });
        }
        for instance in &settings.custom_runner.instances {
            apps.push(AppDef {
                id: format!("customrunner-{}", instance.id),
                name: instance.name.clone(),
                icon: instance.icon.clone(),
                kind: AppKind::CustomRunner {
                    command_template: instance.command_template.clone(),
                },
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: instance.is_default,
            });
        }
        for instance in &settings.web_app.instances {
            apps.push(AppDef {
                id: format!("webapp-{}", instance.id),
                name: instance.name.clone(),
                icon: instance.icon.clone(),
                kind: AppKind::WebApp {
                    command: instance.command.clone(),
                    keep_on_exit: instance.keep_on_exit,
                },
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: instance.is_default,
            });
        }
        for instance in &settings.browser.instances {
            apps.push(AppDef {
                id: format!("browser-{}", instance.id),
                name: instance.name.clone(),
                icon: instance.icon.clone(),
                kind: AppKind::Browser {
                    url: instance.url.clone(),
                },
                can_close: true,
                show_in_new_tab_menu: true,
                is_default: instance.is_default,
            });
        }
        Self { apps }
    }

    pub fn get(&self, id: &str) -> Option<&AppDef> {
        self.apps.iter().find(|app| app.id == id)
    }

    /// `getDefaultApplication()`: the first registered application (the terminal).
    pub fn default_app(&self) -> Option<&AppDef> {
        self.apps.first()
    }

    /// The application a new workspace opens with (`getDefaultAppForWorktree`): the
    /// workspace's own setting, then its ancestors' (nearest first), then the global
    /// default, then the first application. Ids that no longer exist are skipped.
    pub fn default_for_workspace<'a>(
        &self,
        candidate_ids: impl IntoIterator<Item = &'a str>,
        global_default: &'a str,
    ) -> Option<&AppDef> {
        candidate_ids
            .into_iter()
            .chain((!global_default.is_empty()).then_some(global_default))
            .find_map(|id| self.get(id))
            .or_else(|| self.default_app())
    }

    pub fn new_tab_menu(&self) -> impl Iterator<Item = &AppDef> {
        self.apps.iter().filter(|app| app.show_in_new_tab_menu)
    }

    /// The new-tab menu of a local or remote session.
    pub fn new_tab_menu_for(&self, remote: bool) -> impl Iterator<Item = &AppDef> {
        self.new_tab_menu().filter(move |app| app.kind.available_on(remote))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_tabs_start_without_a_pty_and_with_a_fresh_handle() {
        let registry = AppRegistry::from_settings(&Settings::default());
        let app = registry.default_app().unwrap();
        assert_eq!(app.id, "terminal");
        let a = app.initial_state();
        let b = app.initial_state();
        assert!(a["ptyId"].is_null());
        assert_ne!(a["ptyHandle"], b["ptyHandle"]);
        assert_eq!(a["keepOnExit"], false);
    }

    #[test]
    fn variants_come_from_settings_and_the_first_app_is_the_default() {
        let settings = treeterm_core::settings::from_value(json!({
            "aiHarness": { "instances": [{
                "id": "claude", "name": "Claude", "icon": "✦", "command": "claude",
                "isDefault": true, "enableSandbox": false, "allowNetwork": true,
                "backgroundColor": "#1a1a24", "disableScrollbar": false, "keepOnExit": true
            }]},
            "customRunner": { "instances": [{
                "id": "build", "name": "Build", "icon": "B",
                "commandTemplate": "make -C {{workspace_path}}", "isDefault": false
            }]}
        }))
        .unwrap();
        let registry = AppRegistry::from_settings(&settings);
        assert_eq!(registry.default_app().unwrap().id, "terminal");
        let claude = registry.get("aiharness-claude").unwrap();
        assert_eq!(claude.startup_command("/w").as_deref(), Some("claude"));
        assert_eq!(claude.background(), Some("#1a1a24"));
        assert_eq!(claude.initial_state()["keepOnExit"], true);
        assert_eq!(claude.initial_state()["sandbox"]["allowNetwork"], true);
        let build = registry.get("customrunner-build").unwrap();
        assert_eq!(build.startup_command("/w").as_deref(), Some("make -C /w"));
        assert_eq!(
            registry.default_for_workspace(["gone", "customrunner-build"], "").unwrap().id,
            "customrunner-build"
        );
        assert_eq!(registry.default_for_workspace([], "aiharness-claude").unwrap().id, "aiharness-claude");
    }

    #[test]
    fn only_local_http_pages_are_allowed() {
        assert!(is_allowed_webview_url("http://localhost:3000/app"));
        assert!(is_allowed_webview_url("https://127.0.0.1/"));
        assert!(!is_allowed_webview_url("https://example.com"));
        assert!(!is_allowed_webview_url("http://localhost.evil.com"));
        assert!(!is_allowed_webview_url("http://user@evil.com"));
        assert!(!is_allowed_webview_url("file:///etc/passwd"));
        assert!(!is_allowed_webview_url("javascript:alert(1)"));
    }

    #[test]
    fn the_ghostty_alias_is_hidden_from_the_menu() {
        let registry = AppRegistry::from_settings(&Settings::default());
        assert!(registry.get("ghostty-terminal").is_some());
        let menu: Vec<_> = registry.new_tab_menu().map(|app| app.id.as_str()).collect();
        // Electron's default settings carry one AI harness ("Claude").
        let expected = [
            "terminal", "filesystem", "review", "comments", "chat", "system-prompt-debugger", "github", "tty-list",
            "ssh-upload", "aiharness-claude",
        ];
        assert_eq!(menu, expected);
    }

    #[test]
    fn upload_is_offered_on_ssh_sessions_only() {
        let registry = AppRegistry::from_settings(&Settings::default());
        let menu = |remote| registry.new_tab_menu_for(remote).map(|app| app.id.clone()).collect::<Vec<_>>();
        assert!(!menu(false).contains(&"ssh-upload".to_owned()));
        assert!(menu(true).contains(&"ssh-upload".to_owned()));
        assert_eq!(menu(false).len() + 1, menu(true).len());
    }

    #[test]
    fn tool_tabs_keep_electrons_ids_and_initial_state() {
        let registry = AppRegistry::from_settings(&Settings::default());
        let ids = [
            "comments", "chat", "system-prompt-debugger", "analyzer-history", "workspace-settings", "github",
            "tty-list", "ssh-upload",
        ];
        for id in ids {
            let app = registry.get(id).unwrap_or_else(|| panic!("{id} is registered"));
            assert!(matches!(app.kind, AppKind::Tool(_)));
            assert!(!app.kind.is_terminal());
            assert!(app.startup_command("/w").is_none());
            assert!(app.background().is_none());
            assert!(app.can_close && !app.is_default);
        }
        assert_eq!(registry.get("chat").unwrap().initial_state(), json!({ "messages": [] }));
        assert_eq!(registry.get("github").unwrap().initial_state(), json!({}));
        assert!(!registry.get("analyzer-history").unwrap().show_in_new_tab_menu);
        assert!(!registry.get("workspace-settings").unwrap().show_in_new_tab_menu);
    }
}
