//! Run actions: project-defined tasks found in the workspace (npm scripts, Makefile targets,
//! Justfile recipes, Taskfile tasks, VS Code launch configurations and tasks), each run in
//! a new PTY (port of `renderer/lib/runActionsClient.ts` and
//! `store/createRunActionsStore.ts`).
//!
//! Unlike the TS client, a provider whose file cannot be read or parsed is reported in
//! [`DetectedActions::failures`] instead of being skipped silently; a missing file is still
//! just no actions.

mod parse;

#[cfg(test)]
mod tests;

use std::future::Future;
use std::sync::{Arc, Mutex};

use futures::FutureExt;
use futures::future::{BoxFuture, join_all};

use crate::daemon::{ApiError, DaemonConnection};
use crate::fs::FilesystemApi;
use crate::fs_ext::{TextRead, TextReadError, read_text};
use crate::pty::CreatePty;

pub use parse::{
    parse_just_recipes, parse_make_targets, parse_npm_scripts, parse_taskfile_names,
    parse_vscode_launch, parse_vscode_tasks, strip_jsonc_comments,
};

/// Where an action is defined; also its provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RunActionSource {
    Npm,
    Make,
    Just,
    Task,
    VscodeLaunch,
    VscodeTask,
}

impl RunActionSource {
    /// Every provider, in the order their actions are listed.
    pub const ALL: [RunActionSource; 6] = [
        RunActionSource::Npm,
        RunActionSource::Make,
        RunActionSource::Just,
        RunActionSource::Task,
        RunActionSource::VscodeLaunch,
        RunActionSource::VscodeTask,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RunActionSource::Npm => "npm",
            RunActionSource::Make => "make",
            RunActionSource::Just => "just",
            RunActionSource::Task => "task",
            RunActionSource::VscodeLaunch => "vscode-launch",
            RunActionSource::VscodeTask => "vscode-task",
        }
    }

    pub fn parse(source: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == source)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunAction {
    pub source: RunActionSource,
    /// What it is called where it is defined (and shown).
    pub name: String,
    pub description: String,
}

impl RunAction {
    /// Deterministic: `<source>:<name>`.
    pub fn id(&self) -> String {
        format!("{}:{}", self.source.as_str(), self.name)
    }
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum RunActionsError {
    #[error("No provider found for action source: {0}")]
    NoProvider(String),
    #[error("tasks.json not found")]
    TasksJsonMissing,
    #[error("Task \"{0}\" has no command")]
    TaskWithoutCommand(String),
    #[error(transparent)]
    Read(#[from] TextReadError),
    #[error("{file} is invalid: {reason}")]
    Invalid { file: &'static str, reason: String },
    /// The PTY could not be started.
    #[error(transparent)]
    Session(#[from] ApiError),
}

/// A provider that found its file but could not use it.
#[derive(Debug, Clone)]
pub struct ProviderFailure {
    pub source: RunActionSource,
    pub error: RunActionsError,
}

#[derive(Debug, Clone, Default)]
pub struct DetectedActions {
    pub actions: Vec<RunAction>,
    pub failures: Vec<ProviderFailure>,
}

/// Starts a PTY running `startup_command` in `cwd` and returns its id
/// (`terminal.createSession`).
pub trait SessionLauncher: Send + Sync {
    fn create_session(
        &self,
        cwd: &str,
        startup_command: &str,
    ) -> BoxFuture<'static, Result<String, ApiError>>;
}

impl SessionLauncher for DaemonConnection {
    fn create_session(
        &self,
        cwd: &str,
        startup_command: &str,
    ) -> BoxFuture<'static, Result<String, ApiError>> {
        self.create_pty(CreatePty {
            cwd: cwd.to_owned(),
            startup_command: Some(startup_command.to_owned()),
            ..CreatePty::default()
        })
        .map(|result| result.unwrap_or_else(|lost| Err(ApiError::from(lost))))
        .boxed()
    }
}

/// Detection and running, as a trait so the store can be driven by a fake.
pub trait RunActionsApi: Send + Sync {
    fn detect(&self, workspace_path: &str) -> BoxFuture<'static, DetectedActions>;
    /// Start the action in a new PTY; returns the PTY id.
    fn run(
        &self,
        workspace_path: &str,
        action_id: &str,
    ) -> BoxFuture<'static, Result<String, RunActionsError>>;
}

/// Reads project files relative to a workspace.
struct Files<'a> {
    fs: &'a dyn FilesystemApi,
    workspace_path: &'a str,
}

impl Files<'_> {
    /// The file's content; `None` when it does not exist.
    async fn read(&self, relative_path: &str) -> Result<Option<String>, RunActionsError> {
        let absolute = format!("{}/{relative_path}", self.workspace_path);
        match read_text(self.fs, self.workspace_path, &absolute).await? {
            TextRead::Read(content) => Ok(Some(content)),
            TextRead::Missing => Ok(None),
        }
    }

    /// The first of `candidates` that exists.
    async fn read_first(&self, candidates: &[&str]) -> Result<Option<String>, RunActionsError> {
        for candidate in candidates {
            if let Some(content) = self.read(candidate).await? {
                return Ok(Some(content));
            }
        }
        Ok(None)
    }
}

fn invalid(file: &'static str) -> impl Fn(serde_json::Error) -> RunActionsError {
    move |error| RunActionsError::Invalid {
        file,
        reason: error.to_string(),
    }
}

/// Turns a provider's file into its actions.
type Parser = fn(&str) -> Result<Vec<RunAction>, RunActionsError>;

async fn detect_source(
    source: RunActionSource,
    files: &Files<'_>,
) -> Result<Vec<RunAction>, RunActionsError> {
    let (candidates, parse): (&[&str], Parser) = match source {
        RunActionSource::Npm => (&["package.json"], |content| {
            parse_npm_scripts(content).map_err(invalid("package.json"))
        }),
        RunActionSource::Make => (&["Makefile"], |content| Ok(parse_make_targets(content))),
        RunActionSource::Just => (&["Justfile", "justfile"], |content| {
            Ok(parse_just_recipes(content))
        }),
        RunActionSource::Task => (&["Taskfile.yml", "Taskfile.yaml"], |content| {
            Ok(parse_taskfile_names(content))
        }),
        RunActionSource::VscodeLaunch => (&[".vscode/launch.json"], |content| {
            parse_vscode_launch(content).map_err(invalid(".vscode/launch.json"))
        }),
        RunActionSource::VscodeTask => (&[".vscode/tasks.json"], |content| {
            parse_vscode_tasks(content).map_err(invalid(".vscode/tasks.json"))
        }),
    };
    match files.read_first(candidates).await? {
        Some(content) if !content.is_empty() => parse(&content),
        Some(_) | None => Ok(Vec::new()),
    }
}

/// The shell command that runs the action `name` of `source`.
async fn command_for(
    source: RunActionSource,
    name: &str,
    files: &Files<'_>,
) -> Result<String, RunActionsError> {
    Ok(match source {
        RunActionSource::Npm => format!("npm run {name}"),
        RunActionSource::Make => format!("make {name}"),
        RunActionSource::Just => format!("just {name}"),
        RunActionSource::Task => format!("task {name}"),
        // A launch configuration cannot be replicated outside VS Code.
        RunActionSource::VscodeLaunch => {
            format!("echo \"Launch config: {name} (not directly runnable)\"")
        }
        RunActionSource::VscodeTask => {
            let content = files
                .read(".vscode/tasks.json")
                .await?
                .filter(|content| !content.is_empty())
                .ok_or(RunActionsError::TasksJsonMissing)?;
            let tasks = parse::parse_tasks_file(&content).map_err(invalid(".vscode/tasks.json"))?;
            let task = tasks
                .tasks
                .into_iter()
                .find(|task| task.label.as_deref() == Some(name));
            let Some((command, args)) = task.and_then(|task| {
                Some((
                    task.command.filter(|command| !command.is_empty())?,
                    task.args,
                ))
            }) else {
                return Err(RunActionsError::TaskWithoutCommand(name.to_owned()));
            };
            match args {
                Some(args) => format!("{command} {}", args.join(" ")),
                None => command,
            }
        }
    })
}

/// Finds actions through the daemon's filesystem and runs them in new PTYs. Cheap to clone.
#[derive(Clone)]
pub struct RunActionsClient {
    fs: Arc<dyn FilesystemApi>,
    terminal: Arc<dyn SessionLauncher>,
}

impl RunActionsClient {
    pub fn new(fs: Arc<dyn FilesystemApi>, terminal: Arc<dyn SessionLauncher>) -> Self {
        Self { fs, terminal }
    }

    /// Every provider's actions, in provider order; the providers read concurrently.
    pub async fn detect(&self, workspace_path: &str) -> DetectedActions {
        let files = Files {
            fs: self.fs.as_ref(),
            workspace_path,
        };
        let results = join_all(RunActionSource::ALL.map(|source| {
            let files = &files;
            async move { (source, detect_source(source, files).await) }
        }))
        .await;
        let mut detected = DetectedActions::default();
        for (source, result) in results {
            match result {
                Ok(actions) => detected.actions.extend(actions),
                Err(error) => detected.failures.push(ProviderFailure { source, error }),
            }
        }
        detected
    }

    /// Run the action with id `action_id` (`<source>:<name>`) in a new PTY; returns its id.
    pub async fn run(
        &self,
        workspace_path: &str,
        action_id: &str,
    ) -> Result<String, RunActionsError> {
        let (source, name) = action_id.split_once(':').unwrap_or((action_id, ""));
        let source = RunActionSource::parse(source)
            .ok_or_else(|| RunActionsError::NoProvider(source.to_owned()))?;
        let files = Files {
            fs: self.fs.as_ref(),
            workspace_path,
        };
        let command = command_for(source, name, &files).await?;
        Ok(self
            .terminal
            .create_session(workspace_path, &command)
            .await?)
    }
}

impl RunActionsApi for RunActionsClient {
    fn detect(&self, workspace_path: &str) -> BoxFuture<'static, DetectedActions> {
        let (client, workspace_path) = (self.clone(), workspace_path.to_owned());
        async move { client.detect(&workspace_path).await }.boxed()
    }

    fn run(
        &self,
        workspace_path: &str,
        action_id: &str,
    ) -> BoxFuture<'static, Result<String, RunActionsError>> {
        let (client, workspace_path, action_id) = (
            self.clone(),
            workspace_path.to_owned(),
            action_id.to_owned(),
        );
        async move { client.run(&workspace_path, &action_id).await }.boxed()
    }
}

// === store ==================================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetectionPhase {
    Detecting,
    Idle,
}

#[derive(Debug, Clone)]
pub struct RunActionsState {
    pub actions: Vec<RunAction>,
    pub failures: Vec<ProviderFailure>,
    pub phase: DetectionPhase,
}

/// Puts the phase back to idle however detection ends, finished or dropped.
struct Detecting(Arc<Mutex<RunActionsState>>);

impl Drop for Detecting {
    fn drop(&mut self) {
        self.0.lock().expect("run actions state lock").phase = DetectionPhase::Idle;
    }
}

/// `createRunActionsStore`: one workspace's actions and whether they are being detected.
#[derive(Clone)]
pub struct RunActionsStore {
    workspace_path: String,
    api: Arc<dyn RunActionsApi>,
    state: Arc<Mutex<RunActionsState>>,
}

impl RunActionsStore {
    /// The store and its first detection, which the owner drives (TS detected on creation).
    pub fn new(
        workspace_path: &str,
        api: Arc<dyn RunActionsApi>,
    ) -> (Self, impl Future<Output = ()> + Send + 'static + use<>) {
        let store = Self {
            workspace_path: workspace_path.to_owned(),
            api,
            state: Arc::new(Mutex::new(RunActionsState {
                actions: Vec::new(),
                failures: Vec::new(),
                phase: DetectionPhase::Idle,
            })),
        };
        let initial = store.detect();
        (store, initial)
    }

    pub fn state(&self) -> RunActionsState {
        self.state.lock().expect("run actions state lock").clone()
    }

    /// Detect again. The phase is [`DetectionPhase::Detecting`] from this call until the
    /// returned future finishes or is dropped.
    pub fn detect(&self) -> impl Future<Output = ()> + Send + 'static + use<> {
        self.state.lock().expect("run actions state lock").phase = DetectionPhase::Detecting;
        let detecting = Detecting(Arc::clone(&self.state));
        let detection = self.api.detect(&self.workspace_path);
        async move {
            let detected = detection.await;
            let mut state = detecting.0.lock().expect("run actions state lock");
            state.actions = detected.actions;
            state.failures = detected.failures;
            drop(state);
            drop(detecting);
        }
    }

    pub fn run(&self, action_id: &str) -> BoxFuture<'static, Result<String, RunActionsError>> {
        self.api.run(&self.workspace_path, action_id)
    }
}
