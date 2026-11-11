//! Ports of `lib/runActionsClient.test.ts` and `store/createRunActionsStore.test.ts`, case by
//! case.

use std::sync::Mutex;

use futures::channel::oneshot;

use super::*;
use crate::fakes::{FakeFs, FakeRead, block_on};

fn action(source: RunActionSource, name: &str, description: &str) -> RunAction {
    RunAction {
        source,
        name: name.to_owned(),
        description: description.to_owned(),
    }
}

fn names(actions: &[RunAction]) -> Vec<&str> {
    actions.iter().map(|action| action.name.as_str()).collect()
}

mod parse_make_targets {
    use super::*;

    #[test]
    fn parses_simple_targets() {
        let content = "\nbuild:\n\techo build\n\ntest:\n\techo test\n\nclean:\n\trm -rf dist\n";
        let actions = parse_make_targets(content);
        assert_eq!(
            actions,
            vec![
                action(RunActionSource::Make, "build", ""),
                action(RunActionSource::Make, "test", ""),
                action(RunActionSource::Make, "clean", ""),
            ]
        );
        assert_eq!(actions[0].id(), "make:build");
    }

    #[test]
    fn skips_phony_and_dotted_special_targets() {
        let actions = parse_make_targets(".PHONY: build test\n.DEFAULT:\nbuild:\n\techo build\n");
        assert_eq!(names(&actions), vec!["build"]);
    }

    #[test]
    fn handles_targets_with_dots_and_dashes() {
        assert_eq!(
            names(&parse_make_targets("my-target.all:\n\techo hi\n")),
            vec!["my-target.all"]
        );
    }

    #[test]
    fn returns_empty_for_empty_content() {
        assert_eq!(parse_make_targets(""), vec![]);
    }
}

mod parse_just_recipes {
    use super::*;

    #[test]
    fn parses_simple_recipes() {
        let content = "\nbuild:\n  echo build\n\ntest arg:\n  echo test\n\nclean:\n  rm -rf dist\n";
        let actions = parse_just_recipes(content);
        assert_eq!(names(&actions), vec!["build", "test", "clean"]);
        assert_eq!(actions[1].id(), "just:test");
    }

    #[test]
    fn returns_empty_for_empty_content() {
        assert_eq!(parse_just_recipes(""), vec![]);
    }
}

mod parse_taskfile_names {
    use super::*;

    #[test]
    fn parses_task_names_under_tasks_section() {
        let content = "version: '3'\n\ntasks:\n  build:\n    cmds:\n      - go build\n  test:\n    cmds:\n      - go test\n";
        assert_eq!(
            parse_taskfile_names(content),
            vec![
                action(RunActionSource::Task, "build", ""),
                action(RunActionSource::Task, "test", "")
            ]
        );
    }

    #[test]
    fn returns_empty_when_no_tasks_section() {
        assert_eq!(parse_taskfile_names("version: 3"), vec![]);
    }

    #[test]
    fn stops_at_the_next_top_level_key() {
        let content = "tasks:\n  build:\n    cmds: []\nvars:\n  other:\n";
        assert_eq!(names(&parse_taskfile_names(content)), vec!["build"]);
    }
}

mod strip_jsonc_comments {
    use super::*;

    #[test]
    fn strips_single_line_comments() {
        let input = "{\n  // This is a comment\n  \"name\": \"test\"\n}";
        assert!(strip_jsonc_comments(input).contains("\"name\": \"test\""));
        assert!(!strip_jsonc_comments(input).contains("// This"));
    }

    #[test]
    fn strips_multi_line_comments() {
        let input = "{\n  /* multi\n     line */\n  \"name\": \"test\"\n}";
        assert!(strip_jsonc_comments(input).contains("\"name\": \"test\""));
        assert!(!strip_jsonc_comments(input).contains("multi"));
    }

    #[test]
    fn keeps_crlf_line_ends() {
        assert_eq!(strip_jsonc_comments("{ // c\r\n}"), "{ \r\n}");
    }
}

mod parse_vscode_launch {
    use super::*;

    #[test]
    fn parses_launch_configurations() {
        let content = r#"{
          // launch config
          "version": "0.2.0",
          "configurations": [
            { "name": "Launch Program", "type": "node", "program": "index.js" },
            { "name": "Debug Tests", "type": "node" }
          ]
        }"#;
        let actions = parse_vscode_launch(content).unwrap();
        assert_eq!(
            actions,
            vec![
                action(
                    RunActionSource::VscodeLaunch,
                    "Launch Program",
                    "node — index.js"
                ),
                action(RunActionSource::VscodeLaunch, "Debug Tests", "node"),
            ]
        );
        assert_eq!(actions[0].id(), "vscode-launch:Launch Program");
    }

    #[test]
    fn returns_empty_description_when_config_has_no_type() {
        let actions = parse_vscode_launch(r#"{"configurations": [{"name": "Run"}]}"#).unwrap();
        assert_eq!(actions[0].description, "");
    }

    #[test]
    fn returns_empty_for_no_configurations() {
        assert_eq!(
            parse_vscode_launch(r#"{"version": "0.2.0"}"#).unwrap(),
            vec![]
        );
    }
}

mod parse_vscode_tasks {
    use super::*;

    #[test]
    fn parses_shell_tasks() {
        let content = r#"{
          "version": "2.0.0",
          "tasks": [
            { "label": "Build", "type": "shell", "command": "npm run build" },
            { "label": "Test", "command": "npm test" }
          ]
        }"#;
        let actions = parse_vscode_tasks(content).unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(
            actions[0],
            action(RunActionSource::VscodeTask, "Build", "npm run build")
        );
        assert_eq!(actions[0].id(), "vscode-task:Build");
    }

    #[test]
    fn returns_empty_for_no_tasks() {
        assert_eq!(
            parse_vscode_tasks(r#"{"version": "2.0.0"}"#).unwrap(),
            vec![]
        );
    }

    #[test]
    fn uses_type_as_description_when_no_command() {
        let actions =
            parse_vscode_tasks(r#"{"tasks": [{"label": "Watch", "type": "typescript"}]}"#).unwrap();
        assert_eq!(actions[0].description, "typescript");
    }

    #[test]
    fn filters_tasks_without_labels() {
        let content = r#"{ "tasks": [ { "command": "echo hi" }, { "label": "Build", "command": "npm run build" } ] }"#;
        assert_eq!(names(&parse_vscode_tasks(content).unwrap()), vec!["Build"]);
    }
}

/// The TS `createMockTerminal`: every session gets `pty_id`, or fails with `error`.
struct FakeTerminal {
    result: Result<String, String>,
    sessions: Mutex<Vec<(String, String)>>,
}

impl FakeTerminal {
    fn new(pty_id: &str) -> Arc<Self> {
        Self::with(Ok(pty_id.to_owned()))
    }

    fn with(result: Result<String, String>) -> Arc<Self> {
        Arc::new(Self {
            result,
            sessions: Mutex::new(Vec::new()),
        })
    }

    fn sessions(&self) -> Vec<(String, String)> {
        self.sessions.lock().unwrap().clone()
    }
}

impl SessionLauncher for FakeTerminal {
    fn create_session(
        &self,
        cwd: &str,
        startup_command: &str,
    ) -> BoxFuture<'static, Result<String, ApiError>> {
        self.sessions
            .lock()
            .unwrap()
            .push((cwd.to_owned(), startup_command.to_owned()));
        let result = self.result.clone().map_err(ApiError::Operation);
        async move { result }.boxed()
    }
}

/// The TS `createMockFilesystem`, keyed by path under `/workspace`.
fn filesystem(files: &[(&str, &str)]) -> Arc<FakeFs> {
    let fs = FakeFs::new();
    for (relative, content) in files {
        fs.set(
            &format!("/workspace/{relative}"),
            FakeRead::Content((*content).to_owned()),
        );
    }
    fs
}

fn client(fs: &Arc<FakeFs>, terminal: &Arc<FakeTerminal>) -> RunActionsClient {
    RunActionsClient::new(fs.clone(), terminal.clone())
}

fn of_source(detected: &DetectedActions, source: RunActionSource) -> Vec<RunAction> {
    detected
        .actions
        .iter()
        .filter(|action| action.source == source)
        .cloned()
        .collect()
}

fn session(cwd: &str, command: &str) -> (String, String) {
    (cwd.to_owned(), command.to_owned())
}

mod create_run_actions_api {
    use super::*;

    #[test]
    fn detect_aggregates_actions_from_all_providers() {
        let fs = filesystem(&[
            ("package.json", r#"{"scripts":{"build":"echo build"}}"#),
            ("Makefile", "test:\n\techo test\n"),
        ]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(
            of_source(&detected, RunActionSource::Npm),
            vec![action(RunActionSource::Npm, "build", "echo build")]
        );
        assert_eq!(
            of_source(&detected, RunActionSource::Make),
            vec![action(RunActionSource::Make, "test", "")]
        );
        assert!(detected.failures.is_empty());
        assert!(
            fs.reads()
                .iter()
                .all(|(workspace, _, _)| workspace == "/workspace")
        );
        assert!(
            fs.reads()
                .iter()
                .any(|(_, file, _)| file == "/workspace/package.json")
        );
    }

    #[test]
    fn detect_returns_empty_when_no_files_found() {
        let detected =
            block_on(client(&filesystem(&[]), &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(detected.actions, vec![]);
        assert!(detected.failures.is_empty());
    }

    /// TS skipped providers silently when reads failed; their failures are now reported.
    #[test]
    fn detect_handles_read_file_errors_gracefully() {
        let fs = FakeFs::new();
        fs.set_all(FakeRead::Unreachable("read error".to_owned()));
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(detected.actions, vec![]);
        let sources: Vec<_> = detected
            .failures
            .iter()
            .map(|failure| failure.source)
            .collect();
        assert_eq!(sources, RunActionSource::ALL.to_vec());
    }

    #[test]
    fn detect_returns_empty_when_package_json_has_no_scripts() {
        let fs = filesystem(&[("package.json", r#"{"name": "test"}"#)]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(of_source(&detected, RunActionSource::Npm), vec![]);
    }

    #[test]
    fn detect_returns_empty_when_package_json_scripts_is_not_an_object() {
        let fs = filesystem(&[("package.json", r#"{"scripts": "invalid"}"#)]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(of_source(&detected, RunActionSource::Npm), vec![]);
        assert!(detected.failures.is_empty());
    }

    #[test]
    fn detect_reports_a_provider_whose_file_is_invalid_and_keeps_the_others() {
        let fs = filesystem(&[("package.json", "{oops"), ("Makefile", "build:\n")]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(names(&detected.actions), vec!["build"]);
        assert_eq!(detected.failures.len(), 1);
        assert_eq!(detected.failures[0].source, RunActionSource::Npm);
        assert!(
            detected.failures[0]
                .error
                .to_string()
                .starts_with("package.json is invalid")
        );
    }

    #[test]
    fn detect_finds_taskfile_yaml_when_taskfile_yml_missing() {
        let fs = filesystem(&[(
            "Taskfile.yaml",
            "version: '3'\n\ntasks:\n  build:\n    cmds:\n      - echo build\n",
        )]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(
            names(&of_source(&detected, RunActionSource::Task)),
            vec!["build"]
        );
    }

    #[test]
    fn detect_finds_justfile_with_lowercase_fallback() {
        let fs = filesystem(&[("justfile", "build:\n  echo build\n")]);
        let detected = block_on(client(&fs, &FakeTerminal::new("pty-1")).detect("/workspace"));
        assert_eq!(
            names(&of_source(&detected, RunActionSource::Just)),
            vec!["build"]
        );
    }

    #[test]
    fn run_npm_action_creates_session_with_correct_command() {
        let terminal = FakeTerminal::new("pty-npm");
        let result = block_on(client(&filesystem(&[]), &terminal).run("/workspace", "npm:build"));
        assert_eq!(result.unwrap(), "pty-npm");
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "npm run build")]
        );
    }

    #[test]
    fn run_keeps_colons_in_the_action_name() {
        let terminal = FakeTerminal::new("pty-npm");
        block_on(client(&filesystem(&[]), &terminal).run("/workspace", "npm:build:prod")).unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "npm run build:prod")]
        );
    }

    #[test]
    fn run_make_action_creates_session_with_correct_command() {
        let terminal = FakeTerminal::new("pty-make");
        block_on(client(&filesystem(&[]), &terminal).run("/workspace", "make:test")).unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "make test")]
        );
    }

    #[test]
    fn run_just_action_creates_session_with_correct_command() {
        let terminal = FakeTerminal::new("pty-just");
        block_on(client(&filesystem(&[]), &terminal).run("/workspace", "just:deploy")).unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "just deploy")]
        );
    }

    #[test]
    fn run_task_action_creates_session_with_correct_command() {
        let terminal = FakeTerminal::new("pty-task");
        block_on(client(&filesystem(&[]), &terminal).run("/workspace", "task:lint")).unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "task lint")]
        );
    }

    #[test]
    fn run_vscode_launch_action_creates_session_with_echo_message() {
        let terminal = FakeTerminal::new("pty-launch");
        block_on(client(&filesystem(&[]), &terminal).run("/workspace", "vscode-launch:Debug App"))
            .unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session(
                "/workspace",
                "echo \"Launch config: Debug App (not directly runnable)\""
            )]
        );
    }

    #[test]
    fn run_vscode_task_re_reads_tasks_json_and_runs_command() {
        let tasks = r#"{"version":"2.0.0","tasks":[{"label":"Build","command":"npm","args":["run","build"]}]}"#;
        let terminal = FakeTerminal::new("pty-vscode");
        block_on(
            client(&filesystem(&[(".vscode/tasks.json", tasks)]), &terminal)
                .run("/workspace", "vscode-task:Build"),
        )
        .unwrap();
        assert_eq!(
            terminal.sessions(),
            vec![session("/workspace", "npm run build")]
        );
    }

    #[test]
    fn run_vscode_task_returns_error_when_tasks_json_not_found() {
        let result = block_on(
            client(&filesystem(&[]), &FakeTerminal::new("pty-1"))
                .run("/workspace", "vscode-task:Build"),
        );
        assert_eq!(result.unwrap_err().to_string(), "tasks.json not found");
    }

    #[test]
    fn run_vscode_task_returns_error_when_task_has_no_command() {
        let fs = filesystem(&[(".vscode/tasks.json", r#"{"tasks":[{"label":"NoCmd"}]}"#)]);
        let result = block_on(
            client(&fs, &FakeTerminal::new("pty-1")).run("/workspace", "vscode-task:NoCmd"),
        );
        assert_eq!(
            result.unwrap_err().to_string(),
            "Task \"NoCmd\" has no command"
        );
    }

    #[test]
    fn run_vscode_task_runs_command_without_args_when_no_args_specified() {
        let fs = filesystem(&[(
            ".vscode/tasks.json",
            r#"{"tasks":[{"label":"Lint","command":"eslint ."}]}"#,
        )]);
        let terminal = FakeTerminal::new("pty-lint");
        block_on(client(&fs, &terminal).run("/workspace", "vscode-task:Lint")).unwrap();
        assert_eq!(terminal.sessions(), vec![session("/workspace", "eslint .")]);
    }

    #[test]
    fn run_returns_error_for_unknown_provider() {
        let result = block_on(
            client(&filesystem(&[]), &FakeTerminal::new("pty-1"))
                .run("/workspace", "unknown:action"),
        );
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("No provider found")
        );
    }

    #[test]
    fn run_returns_error_when_create_session_fails() {
        let terminal = FakeTerminal::with(Err("daemon down".to_owned()));
        let result = block_on(client(&filesystem(&[]), &terminal).run("/workspace", "npm:build"));
        assert_eq!(result.unwrap_err().to_string(), "daemon down");
    }
}

/// The TS store tests' deps: `detect` answers through a channel the test controls.
struct FakeApi {
    detections: Mutex<Vec<oneshot::Receiver<DetectedActions>>>,
    detect_calls: Mutex<Vec<String>>,
    run_calls: Mutex<Vec<(String, String)>>,
}

impl FakeApi {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            detections: Mutex::new(Vec::new()),
            detect_calls: Mutex::new(Vec::new()),
            run_calls: Mutex::new(Vec::new()),
        })
    }

    /// The next detection resolves when the sender sends.
    fn next_detection(&self) -> oneshot::Sender<DetectedActions> {
        let (tx, rx) = oneshot::channel();
        self.detections.lock().unwrap().push(rx);
        tx
    }
}

impl RunActionsApi for FakeApi {
    fn detect(&self, workspace_path: &str) -> BoxFuture<'static, DetectedActions> {
        self.detect_calls
            .lock()
            .unwrap()
            .push(workspace_path.to_owned());
        let rx = self.detections.lock().unwrap().remove(0);
        async move { rx.await.expect("the test answers every detection") }.boxed()
    }

    fn run(
        &self,
        workspace_path: &str,
        action_id: &str,
    ) -> BoxFuture<'static, Result<String, RunActionsError>> {
        self.run_calls
            .lock()
            .unwrap()
            .push((workspace_path.to_owned(), action_id.to_owned()));
        async { Ok("pty-1".to_owned()) }.boxed()
    }
}

mod create_run_actions_store {
    use super::*;

    fn detected(actions: Vec<RunAction>) -> DetectedActions {
        DetectedActions {
            actions,
            failures: Vec::new(),
        }
    }

    #[test]
    fn auto_detects_on_creation() {
        let api = FakeApi::new();
        let answer = api.next_detection();
        let actions = vec![action(RunActionSource::Npm, "Test", "Run tests")];
        answer.send(detected(actions.clone())).unwrap();

        let (store, initial) = RunActionsStore::new("/workspace", api.clone());
        block_on(initial);
        assert_eq!(store.state().actions, actions);
        assert_eq!(*api.detect_calls.lock().unwrap(), vec!["/workspace"]);
    }

    #[test]
    fn detect_sets_detecting_true_then_false() {
        let api = FakeApi::new();
        let answer = api.next_detection();
        let (store, initial) = RunActionsStore::new("/workspace", api);
        assert_eq!(store.state().phase, DetectionPhase::Detecting);

        answer.send(detected(Vec::new())).unwrap();
        block_on(initial);
        assert_eq!(store.state().phase, DetectionPhase::Idle);
    }

    /// TS: "detect resets detecting on error". Detection cannot fail here (provider failures
    /// are part of the result); the analogue is a detection dropped part way.
    #[test]
    fn detect_resets_detecting_when_dropped() {
        let api = FakeApi::new();
        let first = api.next_detection();
        first.send(detected(Vec::new())).unwrap();
        let (store, initial) = RunActionsStore::new("/workspace", api.clone());
        block_on(initial);

        let _pending = api.next_detection();
        let mut detection = Box::pin(store.detect());
        assert!(detection.as_mut().now_or_never().is_none());
        assert_eq!(store.state().phase, DetectionPhase::Detecting);
        drop(detection);
        assert_eq!(store.state().phase, DetectionPhase::Idle);
    }

    #[test]
    fn detect_keeps_provider_failures() {
        let api = FakeApi::new();
        let answer = api.next_detection();
        answer
            .send(DetectedActions {
                actions: Vec::new(),
                failures: vec![ProviderFailure {
                    source: RunActionSource::Npm,
                    error: RunActionsError::TasksJsonMissing,
                }],
            })
            .unwrap();
        let (store, initial) = RunActionsStore::new("/workspace", api);
        block_on(initial);
        assert_eq!(store.state().failures.len(), 1);
    }

    #[test]
    fn run_delegates_to_deps_run() {
        let api = FakeApi::new();
        api.next_detection().send(detected(Vec::new())).unwrap();
        let (store, initial) = RunActionsStore::new("/workspace", api.clone());
        block_on(initial);

        assert_eq!(block_on(store.run("build")).unwrap(), "pty-1");
        assert_eq!(
            *api.run_calls.lock().unwrap(),
            vec![("/workspace".to_owned(), "build".to_owned())]
        );
    }
}
