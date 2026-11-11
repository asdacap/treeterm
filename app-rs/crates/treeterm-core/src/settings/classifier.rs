//! Terminal-state classifier settings; mirrors `src/shared/classifierSettings.ts`.

use serde::{Deserialize, Serialize};

use super::Extra;

/// Which kind of model classifies terminal output.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClassifierProvider {
    /// An OpenAI-compatible chat model prompted with the system prompt and criteria.
    ChatCompletions,
    /// A dedicated classifier model.
    Classifier,
}

/// The activity states a classifier can report (TS `ClassifiedState`: every `ActivityState`
/// except `error`, which means the classifier itself failed).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ClassifiedState {
    Working,
    SafePermissionRequested,
    PermissionRequest,
    Completed,
    UserInputRequired,
    ApplicationError,
    Idle,
}

impl ClassifiedState {
    /// Order in which criteria are listed in the settings page and the generated chat prompt.
    pub const ALL: [ClassifiedState; 7] = [
        ClassifiedState::Working,
        ClassifiedState::SafePermissionRequested,
        ClassifiedState::PermissionRequest,
        ClassifiedState::Completed,
        ClassifiedState::UserInputRequired,
        ClassifiedState::ApplicationError,
        ClassifiedState::Idle,
    ];

    /// The wire value, which is also the state's key in `criteria`.
    pub fn as_str(self) -> &'static str {
        match self {
            ClassifiedState::Working => "working",
            ClassifiedState::SafePermissionRequested => "safe_permission_requested",
            ClassifiedState::PermissionRequest => "permission_request",
            ClassifiedState::Completed => "completed",
            ClassifiedState::UserInputRequired => "user_input_required",
            ClassifiedState::ApplicationError => "application_error",
            ClassifiedState::Idle => "idle",
        }
    }
}

/// One definition per classified state, shared by both providers (supports `{{cwd}}` and
/// `{{safe_paths}}`). Keys are the states' wire values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClassifierCriteria {
    pub working: String,
    pub safe_permission_requested: String,
    pub permission_request: String,
    pub completed: String,
    pub user_input_required: String,
    pub application_error: String,
    pub idle: String,
    /// States this version does not know (Zod strips them; we keep them for a newer Electron).
    #[serde(flatten)]
    pub extra: Extra,
}

impl ClassifierCriteria {
    pub fn get(&self, state: ClassifiedState) -> &str {
        match state {
            ClassifiedState::Working => &self.working,
            ClassifiedState::SafePermissionRequested => &self.safe_permission_requested,
            ClassifiedState::PermissionRequest => &self.permission_request,
            ClassifiedState::Completed => &self.completed,
            ClassifiedState::UserInputRequired => &self.user_input_required,
            ClassifiedState::ApplicationError => &self.application_error,
            ClassifiedState::Idle => &self.idle,
        }
    }
}

impl Default for ClassifierCriteria {
    /// `defaultClassifierCriteria`.
    fn default() -> Self {
        Self {
            working: "Program is still running and producing output; nothing is waiting on the user.".to_owned(),
            safe_permission_requested: "Program asks for permission, but the action is safe. A safe action is one of:\n  - A git operation, unless it changes another worktree.\n  - Build, test, or installing dependencies.\n  - Anything that only mutates files within one of the safe paths ({{safe_paths}}).\n  Reading outside the safe paths is allowed, just not mutating them. A wide-ranging search via find or `grep -r` outside the safe paths is not safe.".to_owned(),
            permission_request: "Program asks for y/n or similar confirmation, but the action does not meet the safe_permission_requested criteria.".to_owned(),
            completed: "The previous user request has been satisfied.".to_owned(),
            user_input_required: "Program asks for user text input, a choice among design options, or a plan confirmation.".to_owned(),
            application_error: "The program itself failed and stopped: service overloaded, rate limited, API or network error, crash, or unhandled exception. The user must retry. Not a failing command that the program ran and is still handling.".to_owned(),
            idle: "Shell prompt visible, waiting for a command, or user input is incomplete.".to_owned(),
            extra: Extra::new(),
        }
    }
}

/// Context and policy only; the state list and output format are generated from the criteria.
pub const DEFAULT_ANALYZER_SYSTEM_PROMPT: &str = "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, classify the terminal's activity state.";

pub const DEFAULT_TITLE_SYSTEM_PROMPT: &str = "Given the terminal output below, suggest a short title (max 5 words), a brief description (max 15 words), and a git branch name (lowercase kebab-case, max 4 words). Respond with ONLY a JSON object: {\"title\": \"<title>\", \"description\": \"<description>\", \"branchName\": \"<branch-name>\"}";

/// Every analyzer prompt previously shipped as a default, which embedded the state list. A
/// saved copy of one is replaced on load.
pub const LEGACY_DEFAULT_SYSTEM_PROMPTS: [&str; 6] = [
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"} where state is one of the following that best represent the state: \n - \"working\" program is still running and producing output, nothing is waiting on the user.\n - \"safe_permission_requested\" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - \"permission_request\" program asking for y/n or similar confirmation but it is not considered safe as \"safe_permission_requested\".\n - \"completed\" when previous user request satisfied.\n - \"user_input_required\" program asking for user text input or confirmation among design choice or a plan confirmation.\n - \"application_error\" the program itself failed and stopped: service overloaded, rate limited, API or network error, crash, or unhandled exception. The user must retry. Not a failing command that the program ran and is still handling.\n - \"idle\" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ",
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"} where state is one of the following that best represent the state: \n - \"working\" program is still running and producing output, nothing is waiting on the user.\n - \"safe_permission_requested\" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - \"permission_request\" program asking for y/n or similar confirmation but it is not considered safe as \"safe_permission_requested\".\n - \"completed\" when previous user request satisfied.\n - \"user_input_required\" program asking for user text input or confirmation among design choice or a plan confirmation.\n - \"idle\" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ",
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"} where state is one of the following that best represent the state: \n - \"safe_permission_requested\" program asking for permission but the action is safe. A safe action are action that either:\n   - Git operation unless it changes other worktree.\n   - Build or test or install dependencies.\n   - Anything that only mutates files within one of the safe paths.\n   - It IS allowed to read outside the safe path, just not mutate them. \n   - But it is not allowed to do a wide ranging search via find or `grep -r` on file outside the safe path.\n - \"permission_request\" program asking for y/n or similar confirmation but it is not considered safe as \"safe_permission_requested\".\n - \"completed\" when previous user request satisfied.\n - \"user_input_required\" program asking for user text input or confirmation among design choice or a plan confirmation.\n - \"idle\" for shell prompt visible, waiting for command or user input is incomplete.\nThe reason field should show the reason for the verdict, no more than 10 word. ",
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"} where state is one of: \"idle\" (shell prompt visible, waiting for command), \"working\" (process actively running/producing output), \"user_input_required\" (program asking for user text input), \"permission_request\" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), \"safe_permission_requested\" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), \"completed\" (task finished, showing final result). \"working\" override other state and reason is the reason for the verdict, no more than 10 word. A git operation is considered safe as long as it does not mutate other branch. A plan approval is considered \"user_input_required\" even if it only mutate safe path.",
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"} where reason is a short explanation of why you chose this state, and state is one of: \"idle\" (shell prompt visible, waiting for command), \"working\" (process actively running/producing output), \"user_input_required\" (program asking for user text input), \"permission_request\" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), \"safe_permission_requested\" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), \"completed\" (task finished, showing final result). No other text.",
    "You are a terminal state analyzer. The current working directory is: {{cwd}}. The safe paths are: {{safe_paths}}. Given the last lines of terminal output, respond with ONLY a JSON object: {\"state\": \"<state>\"} where state is one of: \"idle\" (shell prompt visible, waiting for command), \"working\" (process actively running/producing output), \"user_input_required\" (program asking for user text input), \"permission_request\" (program asking for y/n or similar confirmation and the action may mutate files OUTSIDE the safe paths), \"safe_permission_requested\" (program asking for permission but the action only mutates files within one of the safe paths — safe to approve), \"completed\" (task finished, showing final result). No other text.",
];
