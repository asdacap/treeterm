//! LLM access: OpenAI-compatible chat completions and the classifier endpoint. Port of
//! `renderer/lib/llmClient.ts`.
//!
//! [`LlmApi`] is the transport (the TS `ClassifierTransports` plus the streaming `send`):
//! [`HttpLlm`] talks HTTP with `reqwest`, [`FakeLlm`] scripts answers for tests. [`Llm`] is
//! the facade the app uses (the TS `LlmApi`): terminal classification behind a shared
//! cache, title suggestions, and streaming chat. Futures and streams are `'static` and must
//! run on tokio (hand them to [`crate::rt::Rt`]); dropping one cancels its request.

mod fake;
mod http;
mod sse;

#[cfg(test)]
mod test_server;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::sync::{Arc, LazyLock, Mutex};

use futures::FutureExt;
use futures::future::BoxFuture;
use futures::stream::BoxStream;
use regex::Regex;
use serde::Serialize;
use serde_json::{Map, Value, json};

pub use fake::{FakeCall, FakeLlm};
pub use http::{HttpLlm, RetryPolicy};

use crate::classification::{
    self, Classification, ClassificationInput, ClassifierSettings, classification_identity,
    effective_prompt, prepare_classification_input,
};
use crate::settings::ReasoningEffort;

// === errors =================================================================================

/// Why an LLM call failed. `Display` is what the TS `formatLlmError` showed the user.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum LlmError {
    /// The API answered with an error status (after retries), or put an error in the stream
    /// (the OpenAI SDK's `APIError`). `error` is the body's `error` field.
    #[error("{}", format_api_error(*.status, .error.as_ref(), .message.as_deref()))]
    Api {
        status: Option<u16>,
        error: Option<Value>,
        /// The body when it was not JSON.
        message: Option<String>,
    },
    /// The request could not be sent or the connection broke (with the cause chain).
    #[error("Connection error: {0}")]
    Connection(String),
    #[error("Request timed out.")]
    Timeout,
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
    #[error("Invalid header value for {0}")]
    InvalidHeader(String),
    /// A success response whose body is not what the API documents.
    #[error("Invalid response: {0}")]
    InvalidResponse(String),
    /// A plain HTTP request (the classifier endpoint's `fetch`) failed before a response.
    #[error("{0}")]
    Fetch(String),
}

/// JS truthiness of a JSON value.
fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|n| n != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(_) | Value::Object(_) => true,
    }
}

/// `APIError.makeMessage` followed by `formatLlmError`'s error body.
fn format_api_error(status: Option<u16>, error: Option<&Value>, message: Option<&str>) -> String {
    let error = error.filter(|error| truthy(error));
    let msg = match error {
        Some(error) => match error.get("message").filter(|message| truthy(message)) {
            Some(Value::String(message)) => message.clone(),
            Some(message) => message.to_string(),
            None => error.to_string(),
        },
        None => message.unwrap_or_default().to_owned(),
    };
    let head = match (status, msg.is_empty()) {
        (Some(status), false) => format!("{status} {msg}"),
        (Some(status), true) => format!("{status} status code (no body)"),
        (None, false) => msg,
        (None, true) => "(no status code or body)".to_owned(),
    };
    match error {
        Some(error) => format!("{head}\n{error}"),
        None => head,
    }
}

/// `err` and its sources, joined with `: `.
pub(crate) fn error_chain(err: &dyn std::error::Error) -> String {
    let mut text = err.to_string();
    let mut source = err.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    text
}

// === transport ==============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: Role::System,
            content: content.into(),
        }
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: content.into(),
        }
    }
}

/// An OpenAI-compatible endpoint and model (`ChatCompletionSettings`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// `Off` leaves `reasoning_effort` out of the request.
    pub reasoning: ReasoningEffort,
}

/// A JSON `POST` (what the TS classifier passed to `fetch`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpPost {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Any status is a response; only a failure to get one is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub status_text: String,
    pub body: String,
}

impl HttpResponse {
    /// `Response.ok`.
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }
}

/// How LLM requests leave the process.
pub trait LlmApi: Send + Sync {
    /// One chat completion (`stream: false`): the first choice's content, empty when absent.
    fn complete_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxFuture<'static, Result<String, LlmError>>;

    /// A streamed chat completion: the content deltas, ending after `[DONE]` or with the
    /// first error. Dropping it aborts the request.
    fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxStream<'static, Result<String, LlmError>>;

    /// A plain JSON `POST`, no retries.
    fn fetch(&self, request: HttpPost) -> BoxFuture<'static, Result<HttpResponse, LlmError>>;
}

// === JSON from model output =================================================================

static JSON_FENCE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new("```json\n?").expect("valid regex"));
static FENCE: LazyLock<Regex> = LazyLock::new(|| Regex::new("```\n?").expect("valid regex"));

fn strip_fences(text: &str) -> String {
    FENCE
        .replace_all(&JSON_FENCE.replace_all(text, ""), "")
        .trim()
        .to_owned()
}

/// Strip markdown fences and parse JSON (`parseLlmJson`). If a field value is itself a
/// (fenced) JSON object string, its non-empty fields are merged in.
pub fn parse_llm_json(raw: &str) -> Result<Value, serde_json::Error> {
    let mut parsed: Value = serde_json::from_str(&strip_fences(raw))?;
    let Value::Object(result) = &mut parsed else {
        return Ok(parsed);
    };
    let nested: Vec<Map<String, Value>> = result
        .values()
        .filter_map(Value::as_str)
        .map(strip_fences)
        .filter(|inner| inner.starts_with('{'))
        .filter_map(|inner| match serde_json::from_str(&inner) {
            Ok(Value::Object(inner)) => Some(inner),
            // Not JSON after all: leave the field as it is.
            Ok(_) | Err(_) => None,
        })
        .collect();
    for inner in nested {
        for (key, value) in inner {
            let empty = matches!(&value, Value::Null) || value.as_str() == Some("");
            if !empty {
                result.insert(key, value);
            }
        }
    }
    Ok(parsed)
}

// === facade =================================================================================

/// Analyzer results shared by every [`Llm`] built with it (TS: module-global), so a
/// reconnect or a second window does not re-query buffers already classified with the same
/// configuration. Oldest entries are evicted first. Cheap to clone; clones share.
#[derive(Clone, Default)]
pub struct AnalyzerCache {
    entries: Arc<Mutex<VecDeque<(String, Classification)>>>,
}

pub const ANALYZER_CACHE_SIZE: usize = 1024;

impl AnalyzerCache {
    fn lock(&self) -> std::sync::MutexGuard<'_, VecDeque<(String, Classification)>> {
        // Only plain pushes and reads happen under the lock; a panic cannot leave it torn.
        self.entries
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn get(&self, identity: &str) -> Option<Classification> {
        self.lock()
            .iter()
            .find(|(entry, _)| entry == identity)
            .map(|(_, result)| result.clone())
    }

    fn insert(&self, identity: String, result: Classification) {
        let mut entries = self.lock();
        entries.push_back((identity, result));
        while entries.len() > ANALYZER_CACHE_SIZE {
            entries.pop_front();
        }
    }

    pub fn clear(&self) {
        self.lock().clear();
    }

    pub fn len(&self) -> usize {
        self.lock().len()
    }

    pub fn is_empty(&self) -> bool {
        self.lock().is_empty()
    }
}

/// A classification as the facade reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Analyzed {
    pub classification: Classification,
    /// Answered from the [`AnalyzerCache`] without a request.
    pub cached: bool,
}

/// `analyzeTerminal`'s result: never an error, the failure is part of the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeOutcome {
    pub result: Result<Analyzed, String>,
    /// The prompt the provider receives, for history and debugging.
    pub system_prompt: String,
}

impl AnalyzeOutcome {
    /// `JSON.stringify` of the TS result object, as recorded in the analyzer history.
    pub fn to_json(&self) -> String {
        let value = match &self.result {
            Ok(Analyzed {
                classification,
                cached: true,
            }) => json!({
                "state": classification.state,
                "reason": classification.reason,
                "cached": true,
                "systemPrompt": self.system_prompt,
            }),
            Ok(Analyzed {
                classification,
                cached: false,
            }) => json!({
                "state": classification.state,
                "reason": classification.reason,
                "systemPrompt": self.system_prompt,
            }),
            Err(error) => json!({ "error": error, "systemPrompt": self.system_prompt }),
        };
        value.to_string()
    }
}

/// The title model's endpoint and prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleSettings {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub title_system_prompt: String,
    pub reasoning_effort: ReasoningEffort,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleSuggestion {
    pub title: String,
    pub description: String,
    pub branch_name: String,
}

/// `generateTitle`'s result: never an error, the failure is part of the outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TitleOutcome {
    pub result: Result<TitleSuggestion, String>,
    pub system_prompt: String,
}

impl TitleOutcome {
    /// `JSON.stringify` of the TS result object, as recorded in the analyzer history.
    pub fn to_json(&self) -> String {
        let value = match &self.result {
            Ok(suggestion) => json!({
                "title": suggestion.title,
                "description": suggestion.description,
                "branchName": suggestion.branch_name,
                "systemPrompt": self.system_prompt,
            }),
            Err(error) => json!({ "error": error, "systemPrompt": self.system_prompt }),
        };
        value.to_string()
    }
}

/// The app's LLM entry point (`createLlmClient`). Cheap to clone.
#[derive(Clone)]
pub struct Llm {
    api: Arc<dyn LlmApi>,
    cache: AnalyzerCache,
}

impl Llm {
    pub fn new(api: Arc<dyn LlmApi>, cache: AnalyzerCache) -> Self {
        Self { api, cache }
    }

    /// Streamed chat (`send`): content deltas; dropping the stream cancels it (`cancel`).
    pub fn send(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxStream<'static, Result<String, LlmError>> {
        self.api.stream_chat(messages, settings)
    }

    /// Classify a terminal buffer, answering from the shared cache when the same request
    /// was classified before. Failures are not cached.
    pub fn analyze_terminal(
        &self,
        input: ClassificationInput,
        settings: ClassifierSettings,
    ) -> BoxFuture<'static, AnalyzeOutcome> {
        let api = self.api.clone();
        let cache = self.cache.clone();
        async move {
            let system_prompt = effective_prompt(&prepare_classification_input(&input), &settings);
            let identity = match classification_identity(&input, &settings) {
                Ok(identity) => identity,
                Err(error) => {
                    return AnalyzeOutcome {
                        result: Err(error.to_string()),
                        system_prompt,
                    };
                }
            };
            if let Some(classification) = cache.get(&identity) {
                return AnalyzeOutcome {
                    result: Ok(Analyzed {
                        classification,
                        cached: true,
                    }),
                    system_prompt,
                };
            }
            let result = match classification::classify(api.as_ref(), &settings, &input).await {
                Ok(classification) => {
                    cache.insert(identity, classification.clone());
                    Ok(Analyzed {
                        classification,
                        cached: false,
                    })
                }
                Err(error) => Err(error.to_string()),
            };
            AnalyzeOutcome {
                result,
                system_prompt,
            }
        }
        .boxed()
    }

    pub fn clear_analyzer_cache(&self) {
        self.cache.clear();
    }

    /// Suggest a title, description and branch name for a terminal buffer.
    pub fn generate_title(
        &self,
        buffer: String,
        settings: TitleSettings,
    ) -> BoxFuture<'static, TitleOutcome> {
        let api = self.api.clone();
        async move {
            let messages = vec![
                ChatMessage::system(settings.title_system_prompt.clone()),
                ChatMessage::user(buffer),
            ];
            let chat = ChatSettings {
                base_url: settings.base_url,
                api_key: settings.api_key,
                model: settings.model,
                reasoning: settings.reasoning_effort,
            };
            let result = match api.complete_chat(messages, chat).await {
                Ok(response) => parse_title(&response),
                Err(error) => Err(error.to_string()),
            };
            TitleOutcome {
                result,
                system_prompt: settings.title_system_prompt,
            }
        }
        .boxed()
    }
}

/// The title model's JSON; a missing or non-string field reads as empty.
fn parse_title(response: &str) -> Result<TitleSuggestion, String> {
    let parsed = parse_llm_json(response).map_err(|error| error.to_string())?;
    if parsed.is_null() {
        return Err("LLM returned null instead of a JSON object".to_owned());
    }
    let field = |key: &str| {
        parsed
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Ok(TitleSuggestion {
        title: field("title"),
        description: field("description"),
        branch_name: field("branchName"),
    })
}
