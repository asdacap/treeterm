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

/// What JS shows for a string, number or boolean; `None` for anything else.
fn scalar_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) => Some(number.to_string()),
        Value::Bool(flag) => Some(flag.to_string()),
        Value::Null | Value::Array(_) | Value::Object(_) => None,
    }
}

/// [`scalar_text`] of a value JS counts as true: a non-empty string, a non-zero number or
/// `true`.
fn truthy_text(value: &Value) -> Option<String> {
    let truthy = match value {
        Value::String(text) => !text.is_empty(),
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::Bool(flag) => *flag,
        Value::Null | Value::Array(_) | Value::Object(_) => false,
    };
    if truthy { scalar_text(value) } else { None }
}

/// The text of a VS Code `CommandString`: a string, parts joined with spaces, or a
/// `{ value, quoting }` object's value (unquoted, as the TS client ran its commands).
fn command_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(parts) => Some(parts.iter().map(arg_text).collect::<Vec<_>>().join(" ")),
        Value::Object(quoted) => quoted.get("value").and_then(command_text),
        Value::Null | Value::Bool(_) | Value::Number(_) => None,
    }
}

/// One argument as JS `args.join(' ')` shows it, a `CommandString` by its text: a number or
/// boolean stringified, null (or an object without a value) as nothing.
fn arg_text(value: &Value) -> String {
    command_text(value)
        .or_else(|| scalar_text(value))
        .unwrap_or_default()
}

#[derive(Deserialize)]
struct LaunchFile {
    /// Missing or null: none (TS `?? []`).
    #[serde(default)]
    configurations: Option<Vec<Value>>,
}

/// Configurations of `.vscode/launch.json`, described as `type — program`. Read one by one,
/// as leniently as the TS client's unchecked cast: an entry without a string or number
/// `name` (TS listed it as "undefined"), or that is not an object, is skipped.
pub fn parse_vscode_launch(content: &str) -> Result<Vec<RunAction>, serde_json::Error> {
    let file: LaunchFile = serde_json::from_str(&strip_jsonc_comments(content))?;
    Ok(file
        .configurations
        .unwrap_or_default()
        .iter()
        .filter_map(|config| {
            let name = match config.get("name")? {
                Value::String(name) => name.clone(),
                Value::Number(name) => name.to_string(),
                _ => return None,
            };
            let field = |key| config.get(key).and_then(truthy_text);
            let description = match (field("type"), field("program")) {
                (None, _) => String::new(),
                (Some(kind), None) => kind,
                (Some(kind), Some(program)) => format!("{kind} — {program}"),
            };
            Some(action(RunActionSource::VscodeLaunch, &name, description))
        })
        .collect())
}

#[derive(Deserialize)]
struct TasksFile {
    /// Missing or null: none (TS `?? []`).
    #[serde(default)]
    tasks: Option<Vec<Value>>,
}

/// A task of `.vscode/tasks.json`, read as leniently as the TS client's unchecked cast.
pub(super) struct VscodeTask {
    /// What the task is listed and run by; `None` (not listed) when JS counts it false.
    pub(super) label: Option<String>,
    pub(super) command: Option<String>,
    kind: Option<String>,
    /// Present when `args` is an array.
    pub(super) args: Option<Vec<String>>,
}

/// The tasks of `.vscode/tasks.json`, for listing and running; an entry that is not an
/// object is skipped.
pub(super) fn tasks_of(content: &str) -> Result<Vec<VscodeTask>, serde_json::Error> {
    let file: TasksFile = serde_json::from_str(&strip_jsonc_comments(content))?;
    Ok(file
        .tasks
        .unwrap_or_default()
        .iter()
        .filter_map(|task| {
            let task = task.as_object()?;
            Some(VscodeTask {
                label: task.get("label").and_then(truthy_text),
                command: task.get("command").and_then(command_text),
                kind: task.get("type").and_then(scalar_text),
                args: match task.get("args") {
                    Some(Value::Array(args)) => Some(args.iter().map(arg_text).collect()),
                    _ => None,
                },
            })
        })
        .collect())
}

/// Labelled tasks of `.vscode/tasks.json`, described by their command (or type).
pub fn parse_vscode_tasks(content: &str) -> Result<Vec<RunAction>, serde_json::Error> {
    Ok(tasks_of(content)?
        .into_iter()
        .filter_map(|task| {
            let label = task.label?;
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
