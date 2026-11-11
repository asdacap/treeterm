//! Ported case by case from `src/main/settings.test.ts` and `src/renderer/store/settings.test.ts`
//! (module = `describe`, function = `it`, snake_case), plus the Rust-side interop cases.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use super::*;

/// A user-data dir holding `settings.json` with `content`, like the TS tests' mocked `readFileSync`.
fn user_data_with(content: &Value) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(settings_path(dir.path()), serde_json::to_vec(content).unwrap()).unwrap();
    dir
}

/// `loadSettings()` over a file holding `content`.
fn load_json(content: Value) -> Settings {
    let dir = user_data_with(&content);
    load(dir.path()).unwrap()
}

fn read_back(dir: &Path) -> String {
    std::fs::read_to_string(settings_path(dir)).unwrap()
}

/// What Electron's `JSON.stringify(x, null, 2)` writes for `value`.
fn stringify(value: &Value) -> String {
    serde_json::to_string_pretty(value).unwrap()
}

fn invalid_key(result: Result<Settings, SettingsError>) -> String {
    match result {
        Err(SettingsError::Invalid { source: ValidationError::Classifier { key, .. }, .. }) => key,
        other => panic!("expected a classifier validation error, got {other:?}"),
    }
}

mod get_default_settings {
    use super::*;

    #[test]
    fn returns_default_settings_structure() {
        let defaults = Settings::default();

        let analyzer = &defaults.terminal_analyzer;
        assert_eq!(analyzer.provider, ClassifierProvider::ChatCompletions);
        assert_eq!(analyzer.model, "openai/gpt-oss-safeguard-20b");
        assert_eq!(analyzer.title_model, "openai/gpt-oss-safeguard-20b");
        assert_eq!(analyzer.idle_debounce, Duration::from_millis(2000));
        assert_eq!(analyzer.idle_debounce_unread, Duration::from_millis(15000));
        assert_eq!(defaults.terminal.font_size, 14.0);
        assert_eq!(defaults.terminal.font_family, "Menlo, Monaco, Consolas, monospace");
        assert_eq!(defaults.terminal.cursor_style, CursorStyle::Block);
        assert!(defaults.terminal.cursor_blink);
        assert_eq!(defaults.appearance.theme, Theme::Dark);
        assert!(!defaults.sandbox.enabled_by_default);
        assert_eq!(defaults.ai_harness.instances[0].command, "npx @earendil-works/pi-coding-agent");
    }
}

mod load_settings {
    use super::*;

    #[test]
    fn returns_defaults_when_settings_file_does_not_exist() {
        let dir = tempfile::tempdir().unwrap();

        let settings = load(dir.path()).unwrap();

        assert_eq!(settings.terminal.font_size, 14.0);
        assert_eq!(settings, Settings::default());
        // Unlike Electron, loading never writes; the first save creates the file.
        assert!(!settings_path(dir.path()).exists());
    }

    #[test]
    fn merges_partial_settings_with_defaults() {
        let settings = load_json(json!({ "terminal": { "fontSize": 18 } }));

        assert_eq!(settings.terminal.font_size, 18.0); // Overridden
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Block); // Default preserved
        assert_eq!(settings.appearance.theme, Theme::Dark); // Default preserved
    }
}

mod classifier_settings_compatibility {
    use super::*;

    #[test]
    fn migrates_legacy_title_models_without_replacing_explicit_settings() {
        let cases = [
            (json!({}), "openai/gpt-oss-safeguard-20b"),
            (json!({ "terminalAnalyzer": { "model": "custom/chat" } }), "custom/chat"),
            (json!({ "terminalAnalyzer": { "model": "" } }), ""),
            (json!({ "terminalAnalyzer": { "model": "custom/chat", "titleModel": "" } }), ""),
            (json!({ "terminalAnalyzer": { "model": "custom/chat", "titleModel": "titles/chat" } }), "titles/chat"),
        ];
        for (loaded, title_model) in cases {
            let settings = load_json(loaded.clone());
            assert_eq!(settings.terminal_analyzer.provider, ClassifierProvider::ChatCompletions, "{loaded}");
            assert_eq!(settings.terminal_analyzer.title_model, title_model, "{loaded}");
        }
    }

    #[test]
    fn migrates_old_provider_and_model_without_retaining_jev_model() {
        let cases = [
            (json!("jev"), Some("typesafe/jev-1.13"), "typesafe/jev-1.13", ClassifierProvider::Classifier),
            (json!("jev"), Some(""), "", ClassifierProvider::Classifier),
            (json!("jev"), None, "previous/chat", ClassifierProvider::Classifier),
            (json!("chat_completions"), Some("typesafe/jev-1.13"), "previous/chat", ClassifierProvider::ChatCompletions),
            (json!("classifier"), Some("obsolete/model"), "previous/chat", ClassifierProvider::Classifier),
        ];
        for (provider, jev_model, model, expected_provider) in cases {
            let mut analyzer = json!({ "provider": provider, "model": "previous/chat", "titleModel": "titles/chat" });
            if let Some(jev_model) = jev_model {
                analyzer["jevModel"] = json!(jev_model);
            }
            let dir = user_data_with(&json!({ "terminalAnalyzer": analyzer }));

            let settings = load(dir.path()).unwrap();

            let analyzer = &settings.terminal_analyzer;
            assert_eq!(analyzer.provider, expected_provider, "{provider} {jev_model:?}");
            assert_eq!(analyzer.model, model, "{provider} {jev_model:?}");
            assert_eq!(analyzer.title_model, "titles/chat");
            assert_eq!(analyzer.idle_debounce, Duration::from_millis(2000));
            assert_eq!(analyzer.idle_debounce_unread, Duration::from_millis(15000));
            assert!(!analyzer.extra.contains_key("jevModel"));
            save(dir.path(), &settings).unwrap();
            let saved: Value = serde_json::from_str(&read_back(dir.path())).unwrap();
            assert!(saved["terminalAnalyzer"].get("jevModel").is_none());
        }
    }

    #[test]
    fn round_trips_explicit_provider_classifier_and_title_models() {
        let mut settings = Settings::default();
        settings.terminal_analyzer.provider = ClassifierProvider::Classifier;
        settings.terminal_analyzer.model = "typesafe/jev-1.13".to_owned();
        settings.terminal_analyzer.title_model = "chat/titles".to_owned();
        let dir = tempfile::tempdir().unwrap();

        save(dir.path(), &settings).unwrap();

        assert_eq!(load(dir.path()).unwrap().terminal_analyzer, settings.terminal_analyzer);
    }

    /// Zod's check on save is the type system's here: an unknown provider cannot be
    /// represented. What can be represented but not written (a non-finite font size) is
    /// refused without touching the file.
    #[test]
    fn rejects_invalid_settings_before_persisting() {
        assert!(serde_json::from_value::<ClassifierProvider>(json!("unknown")).is_err());

        let mut settings = Settings::default();
        settings.terminal.font_size = f32::NAN;
        let dir = tempfile::tempdir().unwrap();

        assert!(matches!(save(dir.path(), &settings), Err(SettingsError::Encode { .. })));
        assert!(!settings_path(dir.path()).exists());
    }

    #[test]
    fn replaces_a_saved_former_default_prompt_with_the_criteria_free_default() {
        for (index, system_prompt) in LEGACY_DEFAULT_SYSTEM_PROMPTS.iter().enumerate() {
            let settings = load_json(json!({ "terminalAnalyzer": { "systemPrompt": system_prompt } }));
            assert_eq!(settings.terminal_analyzer.system_prompt, DEFAULT_ANALYZER_SYSTEM_PROMPT, "prompt {index}");
        }
    }

    #[test]
    fn keeps_a_customized_prompt_and_fills_criteria_missing_from_the_saved_file() {
        let settings = load_json(json!({ "terminalAnalyzer": {
            "systemPrompt": "My own prompt at {{cwd}}",
            "criteria": { "idle": "Custom idle" },
        } }));

        let analyzer = settings.terminal_analyzer;
        assert_eq!(analyzer.system_prompt, "My own prompt at {{cwd}}");
        assert_eq!(analyzer.criteria, ClassifierCriteria { idle: "Custom idle".to_owned(), ..ClassifierCriteria::default() });
    }

    #[test]
    fn round_trips_edited_criteria() {
        let mut settings = Settings::default();
        settings.terminal_analyzer.criteria.completed = "Done at {{cwd}}".to_owned();
        let dir = tempfile::tempdir().unwrap();

        save(dir.path(), &settings).unwrap();

        assert_eq!(load(dir.path()).unwrap().terminal_analyzer.criteria, settings.terminal_analyzer.criteria);
    }

    #[test]
    fn surfaces_invalid_saved_criteria_without_overwriting_saved_settings() {
        let content = json!({ "terminalAnalyzer": { "criteria": { "idle": 42 } } });
        let dir = user_data_with(&content);

        assert_eq!(invalid_key(load(dir.path())), "terminalAnalyzer.criteria.idle");
        assert_eq!(read_back(dir.path()), serde_json::to_string(&content).unwrap());
    }

    #[test]
    fn surfaces_invalid_providers_without_overwriting_saved_settings() {
        let content = json!({ "terminalAnalyzer": { "provider": "unknown" } });
        let dir = user_data_with(&content);

        assert_eq!(invalid_key(load(dir.path())), "terminalAnalyzer.provider");
        assert_eq!(read_back(dir.path()), serde_json::to_string(&content).unwrap());
    }

    /// The rest of `classifierSettingsSchema`: models must be strings when present, and a null
    /// model is missing only for the `titleModel` fallback (`model ?? default`).
    #[test]
    fn surfaces_invalid_models() {
        let cases = [
            (json!({ "model": 7 }), "terminalAnalyzer.model"),
            (json!({ "model": null, "titleModel": "t" }), "terminalAnalyzer.model"),
            (json!({ "model": "m", "titleModel": null }), "terminalAnalyzer.titleModel"),
            (json!({ "provider": "jev", "jevModel": null }), "terminalAnalyzer.model"),
            (json!({ "provider": null }), "terminalAnalyzer.provider"),
        ];
        for (analyzer, key) in cases {
            let dir = user_data_with(&json!({ "terminalAnalyzer": analyzer }));
            assert_eq!(invalid_key(load(dir.path())), key, "{analyzer}");
        }
    }

    #[test]
    fn criteria_that_are_not_an_object_are_the_defaults() {
        let settings = load_json(json!({ "terminalAnalyzer": { "criteria": "idle" } }));

        assert_eq!(settings.terminal_analyzer.criteria, ClassifierCriteria::default());
    }

    #[test]
    fn criteria_are_listed_in_the_settings_order() {
        let criteria = ClassifierCriteria::default();
        let keys: Vec<&str> = ClassifiedState::ALL.iter().map(|state| state.as_str()).collect();
        let serialized = serde_json::to_value(&criteria).unwrap();

        assert_eq!(serialized.as_object().unwrap().keys().map(String::as_str).collect::<Vec<_>>(), keys);
        for state in ClassifiedState::ALL {
            assert_eq!(serialized[state.as_str()], criteria.get(state));
        }
    }
}

mod merge_settings_migrations {
    use super::*;

    #[test]
    fn migrates_old_applications_array_to_terminal_instances() {
        let settings = load_json(json!({
            "applications": [
                { "id": "app-1", "name": "My Term", "icon": "🖥", "command": "/usr/bin/zsh", "isDefault": true, "isBuiltIn": false }
            ]
        }));

        assert_eq!(settings.terminal.instances.len(), 1);
        assert_eq!(settings.terminal.instances[0].id, "app-1");
        assert_eq!(settings.terminal.instances[0].startup_command, "/usr/bin/zsh");
        assert!(settings.terminal.instances[0].is_default);
    }

    #[test]
    fn skips_built_in_apps_during_applications_migration() {
        let settings = load_json(json!({
            "applications": [
                { "id": "builtin-1", "name": "Built-in", "icon": "🖥", "command": "/bin/sh", "isDefault": true, "isBuiltIn": true },
                { "id": "custom-1", "name": "Custom", "icon": "🖥", "command": "/usr/bin/zsh", "isDefault": false, "isBuiltIn": false }
            ]
        }));

        assert_eq!(settings.terminal.instances.len(), 1);
        assert_eq!(settings.terminal.instances[0].id, "custom-1");
    }

    /// The rest of the `applications` filter: other applications and entries without a
    /// command are skipped; `config.command` wins over `command`.
    #[test]
    fn migrates_only_terminals_with_a_command() {
        let settings = load_json(json!({
            "applications": [
                { "id": "ai", "applicationId": "aiHarness", "command": "claude", "isBuiltIn": false },
                { "id": "empty", "applicationId": "terminal", "command": "", "isBuiltIn": false },
                { "id": "config", "applicationId": "terminal", "config": { "command": "fish" }, "command": "zsh" },
                "not an application"
            ]
        }));

        assert_eq!(settings.terminal.instances.len(), 1);
        assert_eq!(settings.terminal.instances[0].id, "config");
        assert_eq!(settings.terminal.instances[0].startup_command, "fish");
    }

    #[test]
    fn does_not_migrate_applications_when_terminal_instances_already_exist() {
        let settings = load_json(json!({
            "terminal": { "instances": [{ "id": "existing", "name": "Existing", "icon": "🖥", "startupCommand": "", "isDefault": true }] },
            "applications": [
                { "id": "app-1", "name": "My Term", "icon": "🖥", "command": "/usr/bin/zsh", "isDefault": true, "isBuiltIn": false }
            ]
        }));

        assert_eq!(settings.terminal.instances.len(), 1);
        assert_eq!(settings.terminal.instances[0].id, "existing");
    }

    #[test]
    fn uses_the_pi_agent_command_when_old_claude_config_has_no_command() {
        let settings = load_json(json!({ "claude": { "startByDefault": true, "enableSandbox": false } }));

        assert_eq!(settings.ai_harness.instances[0].command, "npx @earendil-works/pi-coding-agent");
    }

    #[test]
    fn migrates_old_claude_config_to_ai_harness_instances() {
        let settings = load_json(json!({
            "claude": { "command": "my-claude", "startByDefault": true, "enableSandbox": false }
        }));

        assert_eq!(settings.ai_harness.instances.len(), 1);
        assert_eq!(settings.ai_harness.instances[0].command, "my-claude");
        assert!(settings.ai_harness.instances[0].is_default);
        assert!(!settings.ai_harness.instances[0].enable_sandbox);
    }

    #[test]
    fn does_not_migrate_claude_config_when_ai_harness_instances_already_exist() {
        let settings = load_json(json!({
            "aiHarness": { "instances": [{ "id": "existing-ai", "name": "AI", "icon": "✦", "command": "ai", "isDefault": false, "enableSandbox": false, "allowNetwork": true, "backgroundColor": "#000" }] },
            "claude": { "command": "my-claude" }
        }));

        assert_eq!(settings.ai_harness.instances.len(), 1);
        assert_eq!(settings.ai_harness.instances[0].id, "existing-ai");
    }

    #[test]
    fn defaults_keep_on_exit_to_false_for_instances_saved_before_the_field_existed() {
        let settings = load_json(json!({
            "aiHarness": { "instances": [{ "id": "existing-ai", "name": "AI", "icon": "✦", "command": "ai", "isDefault": false, "enableSandbox": false, "allowNetwork": true, "backgroundColor": "#000", "disableScrollbar": false }] }
        }));

        assert!(!settings.ai_harness.instances[0].keep_on_exit);
    }

    /// Only a missing file gets the default harness; a file without one has none (Electron's
    /// `loaded.aiHarness?.instances || []`).
    #[test]
    fn a_file_without_ai_harness_instances_has_none() {
        assert_eq!(load_json(json!({})).ai_harness.instances, Vec::new());
    }

    /// The migrations consume the legacy keys: Electron's first save drops them, and so does
    /// ours, or deleting every migrated instance would bring them back on the next load.
    #[test]
    fn legacy_keys_are_not_written_back() {
        let dir = user_data_with(&json!({
            "applications": [{ "id": "app-1", "name": "T", "icon": "", "command": "zsh", "isDefault": false, "isBuiltIn": false }],
            "claude": { "command": "my-claude" },
            "terminalAnalyzer": { "provider": "jev", "jevModel": "typesafe/jev-1.13" }
        }));
        let mut settings = load(dir.path()).unwrap();
        settings.terminal.instances.clear();
        settings.ai_harness.instances.clear();

        save(dir.path(), &settings).unwrap();

        let reloaded = load(dir.path()).unwrap();
        assert_eq!(reloaded.terminal.instances, Vec::new());
        assert_eq!(reloaded.ai_harness.instances, Vec::new());
        assert_eq!(reloaded.terminal_analyzer.provider, ClassifierProvider::Classifier);
        assert_eq!(reloaded.terminal_analyzer.model, "typesafe/jev-1.13");
        assert!(reloaded.extra.is_empty());
    }

    #[test]
    fn preserves_custom_runner_instances_from_loaded_settings() {
        let settings = load_json(json!({
            "customRunner": {
                "instances": [{ "id": "rider", "name": "Rider", "icon": "▶", "commandTemplate": "rider {{workspace_path}}", "isDefault": false }]
            }
        }));

        assert_eq!(settings.custom_runner.instances.len(), 1);
        assert_eq!(settings.custom_runner.instances[0].id, "rider");
        assert_eq!(settings.custom_runner.instances[0].command_template, "rider {{workspace_path}}");
    }

    #[test]
    fn defaults_custom_runner_instances_to_empty_array_when_not_in_stored_settings() {
        assert_eq!(load_json(json!({})).custom_runner.instances, Vec::new());
    }

    #[test]
    fn preserves_web_app_instances_and_defaults_them_to_empty() {
        let next = WebAppInstance {
            id: "next".to_owned(),
            name: "Next".to_owned(),
            icon: "🌐".to_owned(),
            command: "npm run dev -- -p $PORT".to_owned(),
            is_default: false,
            keep_on_exit: true,
            extra: Extra::new(),
        };
        let settings = load_json(json!({ "webApp": { "instances": [next] } }));
        assert_eq!(settings.web_app.instances, vec![next]);

        assert_eq!(load_json(json!({})).web_app.instances, Vec::new());
    }

    #[test]
    fn preserves_browser_instances_and_defaults_them_to_empty() {
        let graf = BrowserInstance {
            id: "graf".to_owned(),
            name: "Grafana".to_owned(),
            icon: "📈".to_owned(),
            url: "http://localhost:3000/".to_owned(),
            is_default: false,
            extra: Extra::new(),
        };
        let settings = load_json(json!({ "browser": { "instances": [graf] } }));
        assert_eq!(settings.browser.instances, vec![graf]);

        assert_eq!(load_json(json!({})).browser.instances, Vec::new());
    }

    #[test]
    fn preserves_string_keybindings_as_is() {
        let settings = load_json(json!({ "keybindings": { "newTab": "t" } }));

        assert_eq!(settings.keybindings.new_tab, "t");
    }

    #[test]
    fn migrates_old_object_keybinding_format_using_prefix_mode_field() {
        let settings = load_json(json!({
            "keybindings": { "newTab": { "direct": "Ctrl+T", "prefixMode": "n" }, "closeTab": { "direct": "Ctrl+W" }, "nextTab": "" }
        }));

        assert_eq!(settings.keybindings.new_tab, "n");
        assert_eq!(settings.keybindings.close_tab, "x");
        assert_eq!(settings.keybindings.next_tab, "n");
    }

    #[test]
    fn uses_defaults_when_keybindings_is_undefined() {
        let settings = load_json(json!({ "terminal": { "fontSize": 16 } }));

        assert_eq!(settings.keybindings.new_tab, "c");
        assert_eq!(settings.keybindings.close_tab, "x");
    }

    /// Electron's `migrateKeybindings` forgets `workspaceFocus` (it predates it), so Electron
    /// resets it to `w` on every load. We keep what the file says.
    #[test]
    fn keeps_the_workspace_focus_keybinding() {
        assert_eq!(load_json(json!({ "keybindings": { "workspaceFocus": "f" } })).keybindings.workspace_focus, "f");
    }

    #[test]
    fn an_empty_global_default_application_takes_the_default() {
        assert_eq!(load_json(json!({ "globalDefaultApplicationId": "" })).global_default_application_id, "terminal");
        assert_eq!(load_json(json!({ "globalDefaultApplicationId": "browser" })).global_default_application_id, "browser");
    }
}

mod save_settings {
    use super::*;

    #[test]
    fn creates_directory_if_it_does_not_exist() {
        let root = tempfile::tempdir().unwrap();
        let dir = root.path().join("nested").join("userData");

        save(&dir, &Settings::default()).unwrap();

        assert!(settings_path(&dir).exists());
    }

    /// Byte for byte what Electron writes for its defaults (checked against
    /// `JSON.stringify(getDefaultSettings(), null, 2)`).
    #[test]
    fn writes_json_stringify_formatting() {
        let dir = tempfile::tempdir().unwrap();

        save(dir.path(), &Settings::default()).unwrap();

        let written = read_back(dir.path());
        assert!(written.starts_with("{\n  \"terminal\": {\n    \"fontSize\": 14,\n    \"fontFamily\": \"Menlo, Monaco, Consolas, monospace\",\n"));
        assert!(written.contains("\n    \"instances\": []\n  },\n  \"sandbox\": {\n"));
        assert!(written.contains("\n    \"timeout\": 1500\n"));
        assert!(written.contains("\n    \"mergeThreshold\": 51200,\n"));
        assert!(written.ends_with("\n  \"debug\": {\n    \"showBadge\": false\n  }\n}"));
        assert!(!written.contains("null"));
        assert_eq!(written, stringify(&serde_json::from_str(&written).unwrap()));
    }
}

mod notification_sound_settings {
    use super::*;

    #[test]
    fn defaults_legacy_settings_and_preserves_explicit_false() {
        let cases = [
            (json!({}), true),
            (json!({ "notifications": {} }), true),
            (json!({ "notifications": { "soundEnabled": false } }), false),
        ];
        for (loaded, enabled) in cases {
            assert_eq!(load_json(loaded.clone()).notifications.sound_enabled, enabled, "{loaded}");
        }
    }

    #[test]
    fn defaults_to_enabled() {
        assert!(Settings::default().notifications.sound_enabled);
    }
}

/// `src/renderer/store/settings.test.ts`: the store's non-UI behaviour is load and save.
mod settings_store {
    use super::*;

    #[test]
    fn uses_backward_compatible_classifier_defaults() {
        let analyzer = Settings::default().terminal_analyzer;
        assert_eq!(analyzer.provider, ClassifierProvider::ChatCompletions);
        assert_eq!(analyzer.model, "openai/gpt-oss-safeguard-20b");
        assert_eq!(analyzer.title_model, "openai/gpt-oss-safeguard-20b");
    }

    #[test]
    fn has_default_settings() {
        let settings = Settings::default();
        assert_eq!(settings.terminal.font_size, 14.0);
        assert_eq!(settings.appearance.theme, Theme::Dark);
        assert!(settings.prefix_mode.enabled);
        assert_eq!(settings.ai_harness.instances[0].command, "npx @earendil-works/pi-coding-agent");
    }

    #[test]
    fn loads_settings_from_electron() {
        let settings = load_json(json!({
            "terminal": { "fontSize": 18, "fontFamily": "Menlo", "cursorStyle": "block", "cursorBlink": true, "showRawChars": false, "allowOsc52Clipboard": false, "maxCols": 160, "instances": [] },
            "appearance": { "theme": "light" },
            "aiHarness": { "instances": [] },
            "customRunner": { "instances": [] },
            "webApp": { "instances": [] },
            "browser": { "instances": [] },
            "sandbox": { "enabledByDefault": false, "allowNetworkByDefault": true },
            "prefixMode": { "enabled": true, "prefixKey": "Control+B", "timeout": 1500 },
            "keybindings": { "newTab": "c", "closeTab": "x", "nextTab": "n", "prevTab": "p", "openSettings": ",", "workspaceFocus": "w" },
            "daemon": { "mergeThreshold": 51200, "compactedLimit": 1048576, "scrollbackLines": 10000 },
            "globalDefaultApplicationId": "terminal",
            "recentDirectories": []
        }));

        assert_eq!(settings.terminal.font_size, 18.0);
        assert_eq!(settings.terminal.font_family, "Menlo");
        assert_eq!(settings.appearance.theme, Theme::Light);
        assert_eq!(settings.prefix_mode.timeout, Duration::from_millis(1500));
    }

    /// The store falls back to its defaults when loading throws. Load errors reach the
    /// caller here; it decides (the app logs and uses [`Settings::default`]).
    #[test]
    fn handles_load_error_gracefully() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(settings_path(dir.path()), b"{not json").unwrap();

        assert!(matches!(load(dir.path()), Err(SettingsError::Parse { .. })));
        assert_eq!(read_back(dir.path()), "{not json");
    }

    #[test]
    fn saves_settings_via_electron() {
        let mut settings = Settings::default();
        settings.appearance.theme = Theme::Light;
        let dir = tempfile::tempdir().unwrap();

        save(dir.path(), &settings).unwrap();

        assert_eq!(load(dir.path()).unwrap(), settings);
    }

    #[test]
    fn throws_error_when_save_fails() {
        let file = tempfile::NamedTempFile::new().unwrap();

        // The user-data "directory" is a file, so neither it nor settings.json can be created.
        assert!(matches!(save(file.path(), &Settings::default()), Err(SettingsError::Write { .. })));
    }

    #[test]
    fn updates_nested_setting_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let mut settings = Settings::default();
        settings.terminal.font_size = 20.0;

        save(dir.path(), &settings).unwrap();

        assert_eq!(load(dir.path()).unwrap().terminal.font_size, 20.0);
    }

    #[test]
    fn preserves_other_values_when_updating_a_category() {
        let dir = user_data_with(&json!({ "terminal": { "fontFamily": "Menlo, Monaco, Consolas, monospace", "fontSize": 12, "cursorStyle": "bar" } }));
        let mut settings = load(dir.path()).unwrap();
        settings.terminal.font_size = 20.0;

        save(dir.path(), &settings).unwrap();

        let saved = load(dir.path()).unwrap();
        assert_eq!(saved.terminal.font_family, "Menlo, Monaco, Consolas, monospace");
        assert_eq!(saved.terminal.cursor_style, CursorStyle::Bar);
        assert_eq!(saved.terminal.font_size, 20.0);
    }
}

/// Running side by side with Electron on the same file.
mod interop {
    use super::*;

    /// A file in Electron's shape, every section customized and unknown keys at every level.
    /// `JSON.stringify(JSON.parse(it), null, 2)` reproduces it, and Electron's `loadSettings`
    /// returns it unchanged except for what Electron itself drops (the unknown top-level,
    /// keybinding and criteria keys) and `keybindings.workspaceFocus` (see
    /// `keeps_the_workspace_focus_keybinding`).
    const ELECTRON_SETTINGS: &str = include_str!("electron_settings.json");

    #[test]
    fn an_electron_file_round_trips_unchanged_with_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(settings_path(dir.path()), ELECTRON_SETTINGS).unwrap();

        let settings = load(dir.path()).unwrap();
        save(dir.path(), &settings).unwrap();

        assert_eq!(read_back(dir.path()), ELECTRON_SETTINGS);
        assert_eq!(load(dir.path()).unwrap(), settings);
    }

    #[test]
    fn reads_every_section() {
        let settings: Settings = from_value(serde_json::from_str(ELECTRON_SETTINGS).unwrap()).unwrap();

        assert_eq!(settings.terminal.font_size, 13.5);
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Bar);
        assert!(settings.terminal.show_raw_chars);
        assert_eq!(settings.terminal.instances[0].extra["futureInstanceKey"], "kept");
        assert_eq!(settings.terminal.extra["lineHeight"], 1.2);
        assert!(settings.sandbox.enabled_by_default);
        assert!(settings.ai_harness.instances[0].keep_on_exit);
        assert_eq!(settings.custom_runner.instances[0].command_template, "rider {{workspace_path}}");
        assert!(settings.web_app.instances[0].is_default);
        assert_eq!(settings.browser.instances[0].url, "http://localhost:3000/");
        assert!(!settings.notifications.sound_enabled);
        assert_eq!(settings.appearance.theme, Theme::System);
        assert_eq!(settings.prefix_mode.timeout, Duration::from_millis(2500));
        assert_eq!(settings.keybindings.workspace_focus, "f");
        assert_eq!(settings.keybindings.extra["splitPane"], "s");
        assert_eq!(settings.daemon.scrollback_lines, 500);
        let [boxed, other] = &settings.ssh.saved_connections[..] else { panic!("two connections") };
        assert_eq!(boxed.identity_file, IdentityFile::Path("~/.ssh/id_box".to_owned()));
        assert_eq!(boxed.display_name(), "Box");
        assert_eq!(boxed.port_forwards[0].local_port, 8080);
        assert_eq!(other.identity_file, IdentityFile::SshDefault);
        assert_eq!(other.display_name(), "you@other.lan");
        assert_eq!(settings.llm.api_key, "sk-test");
        let analyzer = &settings.terminal_analyzer;
        assert_eq!(analyzer.provider, ClassifierProvider::Classifier);
        assert_eq!(analyzer.criteria.get(ClassifiedState::Working), "Working at {{cwd}}");
        assert_eq!(analyzer.criteria.extra["future_state"], "Kept");
        assert_eq!(analyzer.reasoning_effort, ReasoningEffort::High);
        assert_eq!(analyzer.safe_paths, ["/tmp", "/work"]);
        assert_eq!(analyzer.buffer_lines, 50);
        assert_eq!(analyzer.idle_debounce, Duration::from_millis(750));
        assert_eq!(analyzer.extra["temperature"], 0);
        assert!(!settings.github.autodetect_via_gh);
        assert_eq!(settings.global_default_application_id, "aiHarness-claude");
        assert_eq!(settings.recent_directories, ["/work/a", "/work/b"]);
        assert!(settings.debug.show_badge);
        assert_eq!(settings.extra["futureTopLevel"], json!({ "nested": true }));
    }

    /// Electron reads an empty `identityFile` or `label` as absent; we write it absent, never
    /// as `null` or `""`.
    #[test]
    fn empty_optional_connection_fields_are_omitted() {
        let settings = load_json(json!({ "ssh": { "savedConnections": [
            { "id": "c", "host": "h", "user": "u", "port": 22, "identityFile": "", "label": "", "portForwards": [] }
        ] } }));
        let connection = &settings.ssh.saved_connections[0];
        assert_eq!(connection.identity_file, IdentityFile::SshDefault);
        assert_eq!(connection.label, ConnectionLabel::UserAtHost);

        let written = serde_json::to_value(connection).unwrap();
        assert_eq!(written, json!({ "id": "c", "host": "h", "user": "u", "port": 22, "portForwards": [] }));
    }

    #[test]
    fn a_top_level_that_is_not_an_object_is_an_error() {
        let dir = user_data_with(&json!([1, 2]));

        assert!(matches!(
            load(dir.path()),
            Err(SettingsError::Invalid { source: ValidationError::NotAnObject { found: "an array" }, .. })
        ));
    }

    #[test]
    fn writes_font_sizes_as_javascript_numbers() {
        let mut terminal = TerminalSettings { font_size: 13.3, ..TerminalSettings::default() };
        assert_eq!(serde_json::to_string(&serde_json::to_value(&terminal).unwrap()["fontSize"]).unwrap(), "13.3");
        terminal.font_size = 14.0;
        assert_eq!(serde_json::to_string(&serde_json::to_value(&terminal).unwrap()["fontSize"]).unwrap(), "14");
    }
}

/// Values of the wrong type or out of range fall back to the default per key.
mod lenient_reads {
    use super::*;

    #[test]
    fn missing_keys_take_electron_defaults() {
        let defaults = Settings::default();
        let settings = load_json(json!({}));

        assert_eq!(settings, Settings { ai_harness: Instances::default(), ..defaults });
    }

    #[test]
    fn reads_the_terminal_section() {
        let settings = load_json(json!({
            "terminal": {
                "fontSize": 12,
                "fontFamily": "'JetBrains Mono', Menlo, monospace",
                "cursorStyle": "bar",
                "cursorBlink": false,
                "allowOsc52Clipboard": false,
                "maxCols": 200
            },
            "terminalAnalyzer": { "idleDebounceMs": 750 }
        }));
        assert_eq!(settings.terminal.font_size, 12.0);
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Bar);
        assert!(!settings.terminal.cursor_blink);
        assert!(!settings.terminal.allow_osc52_clipboard);
        assert_eq!(settings.terminal.max_cols, 200);
        assert_eq!(settings.terminal.font_families(), vec!["JetBrains Mono", "Menlo"]);
        assert_eq!(settings.terminal_analyzer.idle_debounce, Duration::from_millis(750));
        assert_eq!(settings.terminal_analyzer.idle_debounce_unread, Duration::from_millis(15000));
    }

    #[test]
    fn wrong_types_fall_back_per_key() {
        let settings = load_json(json!({
            "terminal": { "fontSize": "big", "cursorStyle": 3, "maxCols": -1, "fontFamily": "Fira Code", "instances": "none" },
            "sandbox": 5,
            "appearance": { "theme": "blue" },
            "prefixMode": { "timeout": 1.5 },
            "keybindings": { "newTab": 7 },
            "aiHarness": { "instances": [{ "id": "a", "isDefault": "yes" }, 3] },
            "ssh": { "savedConnections": [{ "id": "c", "port": 70000, "identityFile": 1 }] },
            "terminalAnalyzer": { "systemPrompt": 1, "reasoningEffort": "max", "safePaths": "/tmp" },
            "recentDirectories": null
        }));
        let defaults = Settings::default();
        assert_eq!(settings.terminal.font_size, 14.0);
        assert_eq!(settings.terminal.cursor_style, CursorStyle::Block);
        assert_eq!(settings.terminal.max_cols, 160);
        assert_eq!(settings.terminal.font_family, "Fira Code");
        assert_eq!(settings.terminal.instances, Vec::new());
        assert_eq!(settings.sandbox, defaults.sandbox);
        assert_eq!(settings.appearance.theme, Theme::Dark);
        assert_eq!(settings.prefix_mode.timeout, Duration::from_millis(1500));
        assert_eq!(settings.keybindings.new_tab, "c");
        let [harness] = &settings.ai_harness.instances[..] else { panic!("one harness") };
        assert_eq!(harness.id, "a");
        assert!(!harness.is_default);
        let connection = &settings.ssh.saved_connections[0];
        assert_eq!(connection.port, 22);
        assert_eq!(connection.identity_file, IdentityFile::SshDefault);
        assert_eq!(settings.terminal_analyzer.system_prompt, DEFAULT_ANALYZER_SYSTEM_PROMPT);
        assert_eq!(settings.terminal_analyzer.reasoning_effort, ReasoningEffort::Low);
        assert_eq!(settings.terminal_analyzer.safe_paths, ["/tmp"]);
        assert_eq!(settings.recent_directories, Vec::<String>::new());
    }

    #[test]
    fn out_of_range_values_fall_back_per_key() {
        let settings = load_json(json!({ "terminal": { "fontSize": 0, "maxCols": 1 } }));

        assert_eq!(settings.terminal.font_size, 14.0);
        assert_eq!(settings.terminal.max_cols, 160);
    }

    #[test]
    fn a_missing_file_is_the_defaults_and_a_corrupt_one_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(dir.path()).unwrap(), Settings::default());
        std::fs::write(settings_path(dir.path()), b"{not json").unwrap();
        assert!(matches!(load(dir.path()), Err(SettingsError::Parse { .. })));
    }

    #[test]
    fn an_unreadable_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(settings_path(dir.path())).unwrap();

        assert!(matches!(load(dir.path()), Err(SettingsError::Read { .. })));
    }
}

/// Settings logic of Electron's main process (`addRecentDirectory`, the `ssh:*` handlers).
mod helpers {
    use super::*;

    #[test]
    fn add_recent_directory_moves_it_to_the_front_and_keeps_ten() {
        let mut settings = Settings::default();
        for index in 0..12 {
            settings.add_recent_directory(&format!("/d{index}"));
        }
        settings.add_recent_directory("/d5");

        let expected: Vec<String> =
            ["/d5", "/d11", "/d10", "/d9", "/d8", "/d7", "/d6", "/d4", "/d3", "/d2"].map(str::to_owned).into();
        assert_eq!(settings.recent_directories, expected);
    }

    fn connection(id: &str, host: &str, port: u16) -> SshConnectionConfig {
        SshConnectionConfig {
            id: id.to_owned(),
            host: host.to_owned(),
            user: "me".to_owned(),
            port,
            identity_file: IdentityFile::SshDefault,
            label: ConnectionLabel::UserAtHost,
            port_forwards: Vec::new(),
            extra: Extra::new(),
        }
    }

    #[test]
    fn save_ssh_connection_replaces_the_same_destination() {
        let mut settings = Settings::default();
        settings.save_ssh_connection(connection("a", "box", 22));
        settings.save_ssh_connection(connection("b", "box", 2222));
        settings.save_ssh_connection(connection("c", "box", 22));

        let ids: Vec<&str> = settings.ssh.saved_connections.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c", "b"]);

        settings.remove_ssh_connection("c");
        let ids: Vec<&str> = settings.ssh.saved_connections.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["b"]);
    }
}
