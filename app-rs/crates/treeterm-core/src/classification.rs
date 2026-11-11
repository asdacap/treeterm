//! Terminal-state classification: prompt construction, the two providers and response
//! parsing. Port of `renderer/lib/classificationProvider.ts` and `types/classification.ts`.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize, Serializer};
use serde_json::json;

use crate::activity::ActivityState;
use crate::llm::{ChatMessage, ChatSettings, HttpPost, LlmApi, LlmError, parse_llm_json};
use crate::settings::{
    ClassifiedState, ClassifierCriteria, ClassifierProvider, LlmSettings, ReasoningEffort,
    TerminalAnalyzerSettings,
};

/// A provider's verdict (`classificationSchema`: any activity state, including `error`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Classification {
    pub state: ActivityState,
    pub reason: String,
}

/// Which provider classifies, with what only it supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    /// An OpenAI-compatible chat model prompted with the generated state list.
    ChatCompletions { reasoning_effort: ReasoningEffort },
    /// The OpenRouter Decisions endpoint.
    Classifier,
}

impl Provider {
    pub fn kind(self) -> ClassifierProvider {
        match self {
            Provider::ChatCompletions { .. } => ClassifierProvider::ChatCompletions,
            Provider::Classifier => ClassifierProvider::Classifier,
        }
    }
}

/// Endpoint, credentials and model of the classifier (`ClassifierSettings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassifierSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub provider: Provider,
}

impl ClassifierSettings {
    /// The analyzer's classifier from the app settings.
    pub fn from_settings(llm: &LlmSettings, analyzer: &TerminalAnalyzerSettings) -> Self {
        Self {
            base_url: llm.base_url.clone(),
            api_key: llm.api_key.clone(),
            model: analyzer.model.clone(),
            provider: match analyzer.provider {
                ClassifierProvider::ChatCompletions => Provider::ChatCompletions {
                    reasoning_effort: analyzer.reasoning_effort,
                },
                ClassifierProvider::Classifier => Provider::Classifier,
            },
        }
    }
}

/// What to classify and the policy to classify it with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassificationInput {
    pub buffer: String,
    pub cwd: String,
    pub safe_paths: Vec<String>,
    /// Supports `{{cwd}}` and `{{safe_paths}}`.
    pub system_prompt: String,
    pub criteria: ClassifierCriteria,
}

impl ClassificationInput {
    pub fn from_settings(buffer: String, cwd: String, analyzer: &TerminalAnalyzerSettings) -> Self {
        Self {
            buffer,
            cwd,
            safe_paths: analyzer.safe_paths.clone(),
            system_prompt: analyzer.system_prompt.clone(),
            criteria: analyzer.criteria.clone(),
        }
    }
}

/// One expanded definition per classified state, in [`ClassifiedState::ALL`] order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedCriteria(IndexMap<ClassifiedState, String>);

impl PreparedCriteria {
    pub fn get(&self, state: ClassifiedState) -> &str {
        &self.0[&state]
    }
}

impl Serialize for PreparedCriteria {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_map(self.0.iter().map(|(state, text)| (state.as_str(), text)))
    }
}

/// [`ClassificationInput`] with the templates expanded and `cwd` among the safe paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedInput {
    pub buffer: String,
    pub cwd: String,
    pub safe_paths: Vec<String>,
    pub system_prompt: String,
    pub criteria: PreparedCriteria,
}

fn fill_template(text: &str, cwd: &str, safe_paths: &[String]) -> String {
    text.replace("{{cwd}}", cwd)
        .replace("{{safe_paths}}", &safe_paths.join(", "))
}

/// `prepareClassificationInput`.
pub fn prepare_classification_input(input: &ClassificationInput) -> PreparedInput {
    let mut safe_paths: Vec<String> = Vec::new();
    for path in input.safe_paths.iter().chain([&input.cwd]) {
        if !safe_paths.contains(path) {
            safe_paths.push(path.clone());
        }
    }
    let criteria = ClassifiedState::ALL
        .into_iter()
        .map(|state| {
            (
                state,
                fill_template(input.criteria.get(state), &input.cwd, &safe_paths),
            )
        })
        .collect();
    PreparedInput {
        buffer: input.buffer.clone(),
        cwd: input.cwd.clone(),
        system_prompt: fill_template(&input.system_prompt, &input.cwd, &safe_paths),
        safe_paths,
        criteria: PreparedCriteria(criteria),
    }
}

/// The configured prompt followed by the output format and the state list generated from
/// the criteria.
pub fn chat_system_prompt(prepared: &PreparedInput) -> String {
    let states: Vec<String> = ClassifiedState::ALL
        .into_iter()
        .map(|state| {
            format!(
                " - \"{}\": {}",
                state.as_str(),
                prepared.criteria.get(state)
            )
        })
        .collect();
    format!(
        "{}\n\nRespond with ONLY a JSON object: {{\"state\": \"<state>\", \"reason\": \"<reason>\"}} where state is the one of the following that best represents the terminal:\n{}\nThe reason field should give the reason for the verdict, in no more than 10 words.",
        prepared.system_prompt,
        states.join("\n")
    )
}

/// The prompt text the selected provider actually receives, for history and debugging.
pub fn effective_prompt(prepared: &PreparedInput, settings: &ClassifierSettings) -> String {
    match settings.provider {
        Provider::ChatCompletions { .. } => chat_system_prompt(prepared),
        Provider::Classifier => prepared.system_prompt.clone(),
    }
}

/// The Decisions endpoint on the configured origin (`new URL('/api/alpha/decisions', base)`).
pub fn decisions_url(base_url: &str) -> Result<String, ClassifyError> {
    let invalid =
        |error: url::ParseError| ClassifyError::InvalidUrl(format!("{base_url} ({error})"));
    let base = url::Url::parse(base_url).map_err(invalid)?;
    Ok(base.join("/api/alpha/decisions").map_err(invalid)?.into())
}

/// Request semantics without credentials: equal identities may share a cached answer.
pub fn classification_identity(
    input: &ClassificationInput,
    settings: &ClassifierSettings,
) -> Result<String, ClassifyError> {
    let prepared = prepare_classification_input(input);
    let mut identity = json!({
        "buffer": prepared.buffer,
        "cwd": prepared.cwd,
        "safePaths": prepared.safe_paths,
        "systemPrompt": prepared.system_prompt,
        "criteria": prepared.criteria,
        "provider": settings.provider.kind(),
        "endpoint": match settings.provider {
            Provider::Classifier => decisions_url(&settings.base_url)?,
            Provider::ChatCompletions { .. } => settings.base_url.clone(),
        },
        "model": settings.model,
    });
    if let Provider::ChatCompletions { reasoning_effort } = settings.provider {
        identity["reasoning"] = json!(reasoning_effort);
    }
    Ok(identity.to_string())
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum ClassifyError {
    #[error(transparent)]
    Llm(#[from] LlmError),
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    /// The chat model's answer is not a classification.
    #[error("Invalid classification: {0}")]
    InvalidClassification(String),
    /// Non-JSON HTTP errors still surface their status; arbitrary bodies are not exposed.
    #[error("OpenRouter Decisions HTTP {status}: {message}")]
    DecisionsHttp { status: u16, message: String },
    #[error("Invalid Decisions response: {0}")]
    InvalidDecision(String),
}

const DECISIONS_OUTPUT: &str = "Choose the activity state using these criteria. Return the typed choice, not generated JSON or an explanation.";

/// Classify `input` with the configured provider.
pub async fn classify(
    api: &dyn LlmApi,
    settings: &ClassifierSettings,
    input: &ClassificationInput,
) -> Result<Classification, ClassifyError> {
    let prepared = prepare_classification_input(input);
    match settings.provider {
        Provider::ChatCompletions { reasoning_effort } => {
            let messages = vec![
                ChatMessage::system(chat_system_prompt(&prepared)),
                ChatMessage::user(prepared.buffer.clone()),
            ];
            let chat = ChatSettings {
                base_url: settings.base_url.clone(),
                api_key: settings.api_key.clone(),
                model: settings.model.clone(),
                reasoning: reasoning_effort,
            };
            let raw = api.complete_chat(messages, chat).await?;
            parse_chat_classification(&raw)
        }
        Provider::Classifier => {
            let body = json!({
                "model": settings.model,
                "state": { "terminal": prepared.buffer, "cwd": prepared.cwd, "safe_paths": prepared.safe_paths },
                "questions": { "activity_state": {
                    "type": "choice",
                    "instructions": { "guidance": prepared.system_prompt, "output": DECISIONS_OUTPUT },
                    "criteria": prepared.criteria,
                } },
            });
            let request = HttpPost {
                url: decisions_url(&settings.base_url)?,
                headers: vec![
                    (
                        "Authorization".to_owned(),
                        format!("Bearer {}", settings.api_key),
                    ),
                    ("Content-Type".to_owned(), "application/json".to_owned()),
                ],
                body: body.to_string(),
            };
            let response = api.fetch(request).await?;
            if !response.ok() {
                return Err(ClassifyError::DecisionsHttp {
                    status: response.status,
                    message: decisions_error_message(&response.body)
                        .unwrap_or(response.status_text),
                });
            }
            let choice = parse_decision(&response.body)?;
            Ok(Classification {
                state: choice.into(),
                reason: format!("Classifier decision: {}", choice.as_str()),
            })
        }
    }
}

/// The chat model's JSON as a [`Classification`].
pub fn parse_chat_classification(raw: &str) -> Result<Classification, ClassifyError> {
    let invalid =
        |error: serde_json::Error| ClassifyError::InvalidClassification(error.to_string());
    serde_json::from_value(parse_llm_json(raw).map_err(invalid)?).map_err(invalid)
}

/// `{"error": {"message": "…"}}` from an error response.
fn decisions_error_message(body: &str) -> Option<String> {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: ErrorMessage,
    }
    #[derive(Deserialize)]
    struct ErrorMessage {
        message: String,
    }
    serde_json::from_str::<ErrorBody>(body)
        .ok()
        .map(|body| body.error.message)
}

/// `answers.activity_state` of a Decisions response: a typed choice among the classified
/// states (extra fields such as confidence are allowed).
fn parse_decision(body: &str) -> Result<ClassifiedState, ClassifyError> {
    #[derive(Deserialize)]
    struct Response {
        answers: Answers,
    }
    #[derive(Deserialize)]
    struct Answers {
        activity_state: Answer,
    }
    #[derive(Deserialize)]
    struct Answer {
        #[serde(rename = "type")]
        kind: AnswerKind,
        choice: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum AnswerKind {
        Choice,
    }
    let response: Response = serde_json::from_str(body)
        .map_err(|error| ClassifyError::InvalidDecision(error.to_string()))?;
    let Answer {
        kind: AnswerKind::Choice,
        choice,
    } = response.answers.activity_state;
    ClassifiedState::ALL
        .into_iter()
        .find(|state| state.as_str() == choice)
        .ok_or_else(|| ClassifyError::InvalidDecision(format!("unknown choice {choice:?}")))
}

#[cfg(test)]
mod tests;
