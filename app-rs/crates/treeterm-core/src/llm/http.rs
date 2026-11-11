//! [`LlmApi`] over HTTP. Chat completions follow the OpenAI SDK the Electron app used: the
//! same URL, body and headers, retries on 408/409/429/5xx and connection failures with its
//! back-off (honouring `x-should-retry`, `retry-after-ms` and `retry-after`), its per-attempt
//! timeout, and its error messages.

use std::collections::VecDeque;
use std::hash::{BuildHasher, Hasher};
use std::time::{Duration, SystemTime};

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};
use reqwest::header::{
    ACCEPT, AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue, USER_AGENT,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::sse::ChatStreamParser;
use super::{ChatMessage, ChatSettings, HttpPost, HttpResponse, LlmApi, LlmError, error_chain};
use crate::settings::ReasoningEffort;

/// Retries and timeouts (the OpenAI SDK's defaults).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    pub max_retries: u32,
    /// Per attempt, until the response headers arrive.
    pub timeout: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_retries: 2,
            timeout: Duration::from_secs(600),
        }
    }
}

/// Chat completions and plain posts over `reqwest` (rustls, native roots). Cheap to clone.
#[derive(Clone)]
pub struct HttpLlm {
    client: reqwest::Client,
    retry: RetryPolicy,
}

impl HttpLlm {
    pub fn new(retry: RetryPolicy) -> Result<Self, LlmError> {
        let client = reqwest::Client::builder()
            .build()
            .map_err(|error| LlmError::Connection(error_chain(&error)))?;
        Ok(Self { client, retry })
    }

    /// Post a chat-completion request, retrying like the SDK; the successful response.
    async fn post_chat(
        &self,
        settings: &ChatSettings,
        body: String,
    ) -> Result<reqwest::Response, LlmError> {
        let url = chat_url(&settings.base_url)?;
        let headers = chat_headers(&settings.api_key)?;
        let mut retries_remaining = self.retry.max_retries;
        loop {
            let attempt = self
                .client
                .post(url.clone())
                .headers(headers.clone())
                .body(body.clone())
                .send();
            let failure = match tokio::time::timeout(self.retry.timeout, attempt).await {
                Ok(Ok(response)) if response.status().is_success() => return Ok(response),
                Ok(Ok(response)) => {
                    if retries_remaining > 0 && should_retry(&response) {
                        let delay = retry_delay(
                            response.headers(),
                            self.retry.max_retries,
                            retries_remaining,
                        );
                        drop(response);
                        retries_remaining -= 1;
                        tokio::time::sleep(delay).await;
                        continue;
                    }
                    return Err(status_error(response).await);
                }
                Ok(Err(error)) if error.is_timeout() => LlmError::Timeout,
                Ok(Err(error)) => LlmError::Connection(error_chain(&error)),
                Err(_elapsed) => LlmError::Timeout,
            };
            if retries_remaining == 0 {
                return Err(failure);
            }
            retries_remaining -= 1;
            tokio::time::sleep(default_retry_delay(
                self.retry.max_retries,
                retries_remaining + 1,
            ))
            .await;
        }
    }
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [ChatMessage],
    stream: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

/// The request body, keys in the SDK's order.
pub(super) fn chat_body(messages: &[ChatMessage], settings: &ChatSettings, stream: bool) -> String {
    let request = ChatRequest {
        model: &settings.model,
        messages,
        stream,
        reasoning_effort: match settings.reasoning {
            ReasoningEffort::Off => None,
            effort @ (ReasoningEffort::Low | ReasoningEffort::Medium | ReasoningEffort::High) => {
                Some(effort)
            }
        },
    };
    // Strings and enums only: serializing cannot fail.
    serde_json::to_string(&request).expect("chat request serializes")
}

/// `{baseURL}/chat/completions`, as the SDK joins it.
pub(super) fn chat_url(base_url: &str) -> Result<reqwest::Url, LlmError> {
    let joined = if base_url.ends_with('/') {
        format!("{base_url}chat/completions")
    } else {
        format!("{base_url}/chat/completions")
    };
    reqwest::Url::parse(&joined)
        .map_err(|error| LlmError::InvalidUrl(format!("{joined} ({error})")))
}

fn chat_headers(api_key: &str) -> Result<HeaderMap, LlmError> {
    let mut headers = HeaderMap::new();
    headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(USER_AGENT, HeaderValue::from_static("TreeTerm"));
    let bearer = HeaderValue::from_str(&format!("Bearer {api_key}"))
        .map_err(|_invalid| LlmError::InvalidHeader(AUTHORIZATION.to_string()))?;
    headers.insert(AUTHORIZATION, bearer);
    Ok(headers)
}

/// `shouldRetry`: the server's `x-should-retry` wins, else 408, 409, 429 and 5xx.
fn should_retry(response: &reqwest::Response) -> bool {
    match response
        .headers()
        .get("x-should-retry")
        .map(HeaderValue::as_bytes)
    {
        Some(b"true") => return true,
        Some(b"false") => return false,
        Some(_) | None => {}
    }
    let status = response.status().as_u16();
    matches!(status, 408 | 409 | 429) || status >= 500
}

/// `retryRequest`: `retry-after-ms`, else `retry-after` (seconds or an HTTP date), else the
/// default back-off.
fn retry_delay(headers: &HeaderMap, max_retries: u32, retries_remaining: u32) -> Duration {
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::trim)
    };
    let mut millis = header("retry-after-ms").and_then(|value| value.parse::<f64>().ok());
    if let Some(retry_after) = header("retry-after")
        && millis.is_none_or(|millis| millis == 0.0)
    {
        millis = match retry_after.parse::<f64>() {
            Ok(seconds) => Some(seconds * 1000.0),
            Err(_) => httpdate::parse_http_date(retry_after).ok().map(|at| {
                at.duration_since(SystemTime::now())
                    .map_or(0.0, |wait| wait.as_secs_f64() * 1000.0)
            }),
        };
    }
    match millis {
        Some(millis) if millis.is_finite() => Duration::from_secs_f64(millis.max(0.0) / 1000.0),
        Some(_) | None => default_retry_delay(max_retries, retries_remaining),
    }
}

/// `calculateDefaultRetryTimeoutMillis`: 0.5 s doubling per retry, at most 8 s, minus up to
/// 25 % jitter.
fn default_retry_delay(max_retries: u32, retries_remaining: u32) -> Duration {
    let retries = max_retries.saturating_sub(retries_remaining);
    let seconds = (0.5 * 2f64.powi(retries as i32)).min(8.0);
    let jitter = 1.0 - random_unit() * 0.25;
    Duration::from_secs_f64(seconds * jitter)
}

/// A random number in `[0, 1)`.
fn random_unit() -> f64 {
    let bits = std::collections::hash_map::RandomState::new()
        .build_hasher()
        .finish();
    (bits >> 11) as f64 / (1u64 << 53) as f64
}

/// The SDK's `APIError` for a non-success response.
async fn status_error(response: reqwest::Response) -> LlmError {
    let status = response.status().as_u16();
    let text = match response.text().await {
        Ok(text) => text,
        Err(error) => error_chain(&error),
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(body) => LlmError::Api {
            status: Some(status),
            error: body.get("error").cloned(),
            message: None,
        },
        Err(_) => LlmError::Api {
            status: Some(status),
            error: None,
            message: Some(text),
        },
    }
}

#[derive(Deserialize)]
struct ChatCompletion {
    choices: Vec<CompletionChoice>,
}

#[derive(Deserialize)]
struct CompletionChoice {
    message: CompletionMessage,
}

#[derive(Deserialize)]
struct CompletionMessage {
    content: Option<String>,
}

/// `response.choices[0]?.message.content ?? ''`.
pub(super) fn completion_content(body: &str) -> Result<String, LlmError> {
    let completion: ChatCompletion = serde_json::from_str(body)
        .map_err(|error| LlmError::InvalidResponse(format!("chat completion: {error}")))?;
    Ok(completion
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .unwrap_or_default())
}

struct DeltaStream {
    body: BoxStream<'static, reqwest::Result<Vec<u8>>>,
    parser: ChatStreamParser,
    ready: VecDeque<Result<String, LlmError>>,
    ended: bool,
}

/// The deltas of an accepted streaming response; ends at the body's end or the first error.
fn deltas(response: reqwest::Response) -> BoxStream<'static, Result<String, LlmError>> {
    let state = DeltaStream {
        body: response
            .bytes_stream()
            .map(|chunk| chunk.map(|bytes| bytes.to_vec()))
            .boxed(),
        parser: ChatStreamParser::default(),
        ready: VecDeque::new(),
        ended: false,
    };
    stream::unfold(state, |mut state| async move {
        loop {
            if let Some(item) = state.ready.pop_front() {
                if item.is_err() {
                    state.ready.clear();
                    state.ended = true;
                }
                return Some((item, state));
            }
            if state.ended {
                return None;
            }
            match state.body.next().await {
                Some(Ok(bytes)) => state.ready.extend(state.parser.feed(&bytes)),
                Some(Err(error)) => state
                    .ready
                    .push_back(Err(LlmError::Connection(error_chain(&error)))),
                None => {
                    state.ready.extend(state.parser.finish());
                    state.ended = true;
                }
            }
        }
    })
    .boxed()
}

impl LlmApi for HttpLlm {
    fn complete_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxFuture<'static, Result<String, LlmError>> {
        let this = self.clone();
        async move {
            let body = chat_body(&messages, &settings, false);
            let response = this.post_chat(&settings, body).await?;
            let text = response
                .text()
                .await
                .map_err(|error| LlmError::Connection(error_chain(&error)))?;
            completion_content(&text)
        }
        .boxed()
    }

    fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxStream<'static, Result<String, LlmError>> {
        let this = self.clone();
        stream::once(async move {
            let body = chat_body(&messages, &settings, true);
            this.post_chat(&settings, body).await
        })
        .flat_map(|response| match response {
            Ok(response) => deltas(response),
            Err(error) => stream::iter([Err(error)]).boxed(),
        })
        .boxed()
    }

    fn fetch(&self, request: HttpPost) -> BoxFuture<'static, Result<HttpResponse, LlmError>> {
        let client = self.client.clone();
        async move {
            let mut headers = HeaderMap::new();
            for (name, value) in &request.headers {
                let invalid = || LlmError::InvalidHeader(name.clone());
                let name = HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
                let value = HeaderValue::from_str(value).map_err(|_| invalid())?;
                headers.append(name, value);
            }
            let url = reqwest::Url::parse(&request.url)
                .map_err(|error| LlmError::InvalidUrl(format!("{} ({error})", request.url)))?;
            let response = client
                .post(url)
                .headers(headers)
                .body(request.body)
                .send()
                .await
                .map_err(|error| LlmError::Fetch(error_chain(&error)))?;
            let status = response.status();
            let body = response
                .text()
                .await
                .map_err(|error| LlmError::Fetch(error_chain(&error)))?;
            Ok(HttpResponse {
                status: status.as_u16(),
                status_text: status.canonical_reason().unwrap_or_default().to_owned(),
                body,
            })
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::Role;

    #[test]
    fn joins_the_chat_path_like_the_sdk() {
        assert_eq!(
            chat_url("https://openrouter.ai/api/v1").unwrap().as_str(),
            "https://openrouter.ai/api/v1/chat/completions"
        );
        assert_eq!(
            chat_url("http://localhost:11434/v1/").unwrap().as_str(),
            "http://localhost:11434/v1/chat/completions"
        );
        assert!(matches!(chat_url(""), Err(LlmError::InvalidUrl(_))));
    }

    #[test]
    fn writes_the_body_in_the_sdks_key_order_and_omits_reasoning_when_off() {
        let messages = [
            ChatMessage {
                role: Role::System,
                content: "s".into(),
            },
            ChatMessage::user("u"),
        ];
        let mut settings = ChatSettings {
            base_url: String::new(),
            api_key: String::new(),
            model: "m".into(),
            reasoning: ReasoningEffort::Off,
        };
        assert_eq!(
            chat_body(&messages, &settings, false),
            r#"{"model":"m","messages":[{"role":"system","content":"s"},{"role":"user","content":"u"}],"stream":false}"#
        );
        settings.reasoning = ReasoningEffort::Medium;
        assert_eq!(
            chat_body(&[], &settings, true),
            r#"{"model":"m","messages":[],"stream":true,"reasoning_effort":"medium"}"#
        );
    }

    #[test]
    fn reads_the_first_choice_and_defaults_missing_content_to_empty() {
        assert_eq!(
            completion_content(
                r#"{"choices":[{"message":{"content":"hi"}},{"message":{"content":"no"}}]}"#
            )
            .unwrap(),
            "hi"
        );
        assert_eq!(
            completion_content(r#"{"choices":[{"message":{"content":null}}]}"#).unwrap(),
            ""
        );
        assert_eq!(completion_content(r#"{"choices":[]}"#).unwrap(), "");
        assert!(matches!(
            completion_content(r#"{"error":{"message":"x"}}"#),
            Err(LlmError::InvalidResponse(_))
        ));
    }

    #[test]
    fn honours_retry_after_headers_before_the_default_back_off() {
        let headers = |pairs: &[(&'static str, &str)]| {
            let mut map = HeaderMap::new();
            for (name, value) in pairs {
                map.insert(*name, HeaderValue::from_str(value).unwrap());
            }
            map
        };
        assert_eq!(
            retry_delay(&headers(&[("retry-after-ms", "250")]), 2, 2),
            Duration::from_millis(250)
        );
        assert_eq!(
            retry_delay(&headers(&[("retry-after", "1.5")]), 2, 2),
            Duration::from_millis(1500)
        );
        assert_eq!(
            retry_delay(
                &headers(&[("retry-after-ms", "0"), ("retry-after", "2")]),
                2,
                2
            ),
            Duration::from_secs(2)
        );
        assert_eq!(
            retry_delay(
                &headers(&[("retry-after", "Wed, 21 Oct 2015 07:28:00 GMT")]),
                2,
                2
            ),
            Duration::ZERO
        );
        let first = retry_delay(&HeaderMap::new(), 2, 2);
        assert!(
            first > Duration::from_millis(374) && first <= Duration::from_millis(500),
            "{first:?}"
        );
        let second = default_retry_delay(2, 1);
        assert!(
            second > Duration::from_millis(749) && second <= Duration::from_secs(1),
            "{second:?}"
        );
        assert!(default_retry_delay(10, 0) <= Duration::from_secs(8));
    }
}
