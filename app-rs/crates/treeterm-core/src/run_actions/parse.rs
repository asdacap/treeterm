//! Task definitions in project files: Makefile targets, Justfile recipes, Taskfile tasks,
//! VS Code launch configurations and tasks.

use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::Value;

use super::{RunAction, RunActionSource};

static MAKE_TARGET: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^([a-zA-Z_][a-zA-Z0-9_.-]*)\s*:").expect("valid regex"));
static JUST_RECIPE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?m)^([a-zA-Z_][a-zA-Z0-9_-]*)\s*(?:[^:=]*):").expect("valid regex")
});
static TASKS_SECTION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?m)^tasks:\s*$").expect("valid regex"));
static TASK_NAME: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^  ([a-zA-Z_][a-zA-Z0-9_-]*):\s*$").expect("valid regex"));
static LINE_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?mR)//.*$").expect("valid regex"));
static BLOCK_COMMENT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"/\*[\s\S]*?\*/").expect("valid regex"));

fn action(source: RunActionSource, name: &str, description: impl Into<String>) -> RunAction {
    RunAction {
        source,
        name: name.to_owned(),
        description: description.into(),
    }
}

/// Targets of a Makefile; special targets (`.PHONY`, `.DEFAULT`, ...) are skipped.
pub fn parse_make_targets(content: &str) -> Vec<RunAction> {
    MAKE_TARGET
        .captures_iter(content)
        .map(|captures| action(RunActionSource::Make, &captures[1], ""))
        .collect()
}

/// Recipes of a Justfile: a name, optional parameters, then `:`.
pub fn parse_just_recipes(content: &str) -> Vec<RunAction> {
    JUST_RECIPE
        .captures_iter(content)
        .map(|captures| action(RunActionSource::Just, &captures[1], ""))
        .collect()
}

/// Task names of a Taskfile: the 2-space-indented keys under the top-level `tasks:`.
pub fn parse_taskfile_names(content: &str) -> Vec<RunAction> {
    let Some(section) = TASKS_SECTION.find(content) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    for line in content[section.end()..].split('\n') {
        // The next top-level key ends the section.
        if line.starts_with(|c: char| c.is_ascii_alphabetic()) {
            break;
        }
        if let Some(captures) = TASK_NAME.captures(line) {
            actions.push(action(RunActionSource::Task, &captures[1], ""));
        }
    }
    actions
}

/// Remove `// ...` and `/* ... */` comments, enough for typical `.vscode` JSON files (a `//`
/// inside a string goes too).
pub fn strip_jsonc_comments(content: &str) -> String {
    let without_lines = LINE_COMMENT.replace_all(content, "");
    BLOCK_COMMENT.replace_all(&without_lines, "").into_owned()
}

#[derive(Deserialize)]
struct LaunchFile {
    #[serde(default)]
    configurations: Vec<LaunchConfiguration>,
}

#[derive(Deserialize)]
struct LaunchConfiguration {
    name: String,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    program: Option<String>,
}

/// Configurations of `.vscode/launch.json`, described as `type — program`.
pub fn parse_vscode_launch(content: &str) -> Result<Vec<RunAction>, serde_json::Error> {
    let file: LaunchFile = serde_json::from_str(&strip_jsonc_comments(content))?;
    Ok(file
        .configurations
        .into_iter()
        .map(|config| {
            let description = match (config.kind.as_deref(), config.program.as_deref()) {
                (None | Some(""), _) => String::new(),
                (Some(kind), None | Some("")) => kind.to_owned(),
                (Some(kind), Some(program)) => format!("{kind} — {program}"),
            };
            action(RunActionSource::VscodeLaunch, &config.name, description)
        })
        .collect())
}

#[derive(Deserialize)]
pub(super) struct TasksFile {
    #[serde(default)]
    pub(super) tasks: Vec<VscodeTask>,
}

#[derive(Deserialize)]
pub(super) struct VscodeTask {
    #[serde(default)]
    pub(super) label: Option<String>,
    #[serde(default)]
    pub(super) command: Option<String>,
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(default)]
    pub(super) args: Option<Vec<String>>,
}

pub(super) fn parse_tasks_file(content: &str) -> Result<TasksFile, serde_json::Error> {
    serde_json::from_str(&strip_jsonc_comments(content))
}

/// Labelled tasks of `.vscode/tasks.json`, described by their command (or type).
pub fn parse_vscode_tasks(content: &str) -> Result<Vec<RunAction>, serde_json::Error> {
    Ok(parse_tasks_file(content)?
        .tasks
        .into_iter()
        .filter_map(|task| {
            let label = task.label.filter(|label| !label.is_empty())?;
            let description = task.command.or(task.kind).unwrap_or_default();
            Some(action(RunActionSource::VscodeTask, &label, description))
        })
        .collect())
}

/// Scripts of `package.json`, described by their command; none without a `scripts` object.
pub fn parse_npm_scripts(content: &str) -> Result<Vec<RunAction>, serde_json::Error> {
    let package: Value = serde_json::from_str(content)?;
    let Some(Value::Object(scripts)) = package.get("scripts") else {
        return Ok(Vec::new());
    };
    Ok(scripts
        .iter()
        .map(|(name, command)| {
            let description = match command {
                Value::String(command) => command.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            action(RunActionSource::Npm, name, description)
        })
        .collect())
}
