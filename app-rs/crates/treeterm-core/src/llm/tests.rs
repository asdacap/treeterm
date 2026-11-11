//! Port of `llmClient.test.ts`. The OpenAI SDK mock becomes a real HTTP exchange with a local
//! server (or [`FakeLlm`] where the transport does not matter).

use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use futures::executor::block_on;
use serde_json::json;
use tokio::sync::oneshot;

use super::test_server::{Reply, TestServer, event_stream, json as reply_json, json_with};
use super::*;
use crate::activity::ActivityState;
use crate::classification::Provider;
use crate::settings::ClassifierCriteria;

// --- parseLlmJson ---------------------------------------------------------------------------

#[test]
fn parses_plain_json() {
    assert_eq!(
        parse_llm_json(r#"{"state":"running","reason":"ok"}"#).unwrap(),
        json!({"state": "running", "reason": "ok"})
    );
}

#[test]
fn strips_markdown_fences() {
    assert_eq!(
        parse_llm_json("```json\n{\"state\":\"done\"}\n```").unwrap(),
        json!({"state": "done"})
    );
}

#[test]
fn merges_nested_json_strings() {
    let result = parse_llm_json(r#"{"state":"ok","nested":"{\"title\":\"hello\"}"}"#).unwrap();
    assert_eq!(result["title"], "hello");
}

#[test]
fn throws_on_invalid_json() {
    assert!(parse_llm_json("not json").is_err());
}

#[test]
fn merges_only_non_empty_nested_fields_and_keeps_non_json_strings() {
    let raw = r#"{"title":"outer","inner":"```json\n{\"title\":\"\",\"description\":\"d\",\"x\":null}\n```","text":"{not json"}"#;
    let result = parse_llm_json(raw).unwrap();
    assert_eq!(result["title"], "outer");
    assert_eq!(result["description"], "d");
    assert_eq!(result["text"], "{not json");
    assert!(result.get("x").is_none());
    assert_eq!(parse_llm_json("[1]").unwrap(), json!([1]));
}

// --- formatLlmError -------------------------------------------------------------------------

#[test]
fn formats_error_instances() {
    assert_eq!(LlmError::Fetch("boom".to_owned()).to_string(), "boom");
}

// `formats unknown values`: errors are typed; there is no unknown value to format.

#[test]
fn formats_api_error_with_error_body() {
    let error = LlmError::Api {
        status: Some(429),
        error: Some(json!({"message": "too many requests"})),
        message: None,
    };
    // The SDK's message prefers the body's message; the body follows on its own line.
    assert_eq!(
        error.to_string(),
        "429 too many requests\n{\"message\":\"too many requests\"}"
    );
}

#[test]
fn formats_api_error_without_error_body() {
    let error = LlmError::Api {
        status: Some(400),
        error: None,
        message: Some("bad request".to_owned()),
    };
    assert_eq!(error.to_string(), "400 bad request");
}

#[test]
fn formats_api_errors_like_the_sdk_without_message_or_status() {
    let api = |status, error, message: Option<&str>| {
        LlmError::Api {
            status,
            error,
            message: message.map(str::to_owned),
        }
        .to_string()
    };
    assert_eq!(api(Some(500), None, None), "500 status code (no body)");
    assert_eq!(api(Some(500), None, Some("")), "500 status code (no body)");
    assert_eq!(
        api(None, Some(json!({"code": 1})), None),
        "{\"code\":1}\n{\"code\":1}"
    );
    assert_eq!(
        api(None, Some(json!({"message": {"a": 1}})), None),
        "{\"a\":1}\n{\"message\":{\"a\":1}}"
    );
    assert_eq!(api(None, None, None), "(no status code or body)");
    assert_eq!(
        api(Some(502), Some(json!(null)), None),
        "502 status code (no body)"
    );
}

// --- send (streaming) -----------------------------------------------------------------------

fn chat_settings(base_url: &str) -> ChatSettings {
    ChatSettings {
        base_url: format!("{base_url}/v1"),
        api_key: "test-key".to_owned(),
        model: "gpt-4".to_owned(),
        reasoning: ReasoningEffort::Off,
    }
}

fn http_llm() -> Llm {
    Llm::new(
        Arc::new(HttpLlm::new(RetryPolicy::default()).unwrap()),
        AnalyzerCache::default(),
    )
}

fn delta(text: &str) -> String {
    json!({"choices": [{"delta": {"content": text}}]}).to_string()
}

#[tokio::test]
async fn streams_deltas_to_listeners_and_fires_done() {
    let server = TestServer::start(vec![event_stream(&[
        &delta("Hello"),
        &delta(" world"),
        "[DONE]",
    ])])
    .await;
    let items: Vec<_> = http_llm()
        .send(vec![ChatMessage::user("hi")], chat_settings(&server.url))
        .collect()
        .await;
    assert_eq!(items, vec![Ok("Hello".to_owned()), Ok(" world".to_owned())]);
    let request = &server.requests()[0];
    assert_eq!(
        (request.method.as_str(), request.path.as_str()),
        ("POST", "/v1/chat/completions")
    );
    assert_eq!(request.headers["authorization"], "Bearer test-key");
    assert_eq!(request.headers["content-type"], "application/json");
    assert_eq!(request.headers["accept"], "application/json");
    assert_eq!(
        request.json(),
        json!({"model": "gpt-4", "messages": [{"role": "user", "content": "hi"}], "stream": true})
    );
}

#[tokio::test]
async fn skips_chunks_without_content() {
    let empty = json!({"choices": [{"delta": {}}]}).to_string();
    let server = TestServer::start(vec![event_stream(&[&empty, &delta("data")])]).await;
    let items: Vec<_> = http_llm()
        .send(Vec::new(), chat_settings(&server.url))
        .collect()
        .await;
    assert_eq!(items, vec![Ok("data".to_owned())]);
}

#[tokio::test]
async fn sends_error_on_exception() {
    let server = TestServer::start(vec![reply_json(
        400,
        r#"{"error":{"message":"network error"}}"#,
    )])
    .await;
    let items: Vec<_> = http_llm()
        .send(Vec::new(), chat_settings(&server.url))
        .collect()
        .await;
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].as_ref().unwrap_err().to_string(),
        "400 network error\n{\"message\":\"network error\"}"
    );
}

#[tokio::test]
async fn passes_reasoning_effort_when_reasoning_is_enabled() {
    let server = TestServer::start(vec![event_stream(&[])]).await;
    let settings = ChatSettings {
        reasoning: ReasoningEffort::Medium,
        ..chat_settings(&server.url)
    };
    let items: Vec<_> = http_llm().send(Vec::new(), settings).collect().await;
    assert!(items.is_empty());
    assert_eq!(server.requests()[0].json()["reasoning_effort"], "medium");
}

#[tokio::test]
async fn reports_an_unreachable_endpoint_as_a_connection_error() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    let llm = Llm::new(
        Arc::new(
            HttpLlm::new(RetryPolicy {
                max_retries: 0,
                ..RetryPolicy::default()
            })
            .unwrap(),
        ),
        AnalyzerCache::default(),
    );
    let items: Vec<_> = llm.send(Vec::new(), chat_settings(&url)).collect().await;
    assert!(
        matches!(items.as_slice(), [Err(LlmError::Connection(_))]),
        "{items:?}"
    );
}

// --- analyzeTerminal (completeChatCall) -----------------------------------------------------

fn analyzer_settings(base_url: &str) -> ClassifierSettings {
    ClassifierSettings {
        base_url: format!("{base_url}/v1"),
        api_key: "test-key".to_owned(),
        model: "gpt-4".to_owned(),
        provider: Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::Off,
        },
    }
}

fn analyzer_input(buffer: &str) -> ClassificationInput {
    ClassificationInput {
        buffer: buffer.to_owned(),
        cwd: "/home/user".to_owned(),
        safe_paths: vec!["/usr/bin".to_owned()],
        system_prompt: "You are analyzing a terminal at {{cwd}}. Safe paths: {{safe_paths}}"
            .to_owned(),
        criteria: ClassifierCriteria::default(),
    }
}

fn completion(content: serde_json::Value) -> String {
    json!({"choices": [{"message": {"content": content}}]}).to_string()
}

#[tokio::test]
async fn returns_content_from_completion() {
    let server = TestServer::start(vec![reply_json(
        200,
        &completion(json!(r#"{"state":"idle","reason":"waiting"}"#)),
    )])
    .await;
    let outcome = http_llm()
        .analyze_terminal(analyzer_input("$ ls"), analyzer_settings(&server.url))
        .await;
    let analyzed = outcome.result.unwrap();
    assert_eq!(
        analyzed.classification,
        Classification {
            state: ActivityState::Idle,
            reason: "waiting".to_owned()
        }
    );
    let request = server.requests()[0].json();
    assert_eq!(request["stream"], false);
    assert_eq!(
        request["messages"][1],
        json!({"role": "user", "content": "$ ls"})
    );
    assert!(
        request["messages"][0]["content"]
            .as_str()
            .unwrap()
            .starts_with(
                "You are analyzing a terminal at /home/user. Safe paths: /usr/bin, /home/user\n\n"
            )
    );
}

#[tokio::test]
async fn returns_error_when_no_content() {
    let server = TestServer::start(vec![reply_json(200, &completion(json!(null)))]).await;
    let outcome = http_llm()
        .analyze_terminal(analyzer_input("$ ls"), analyzer_settings(&server.url))
        .await;
    // An empty completion is not JSON.
    assert!(outcome.result.is_err());
}

#[tokio::test]
async fn returns_error_when_no_choices() {
    let server = TestServer::start(vec![reply_json(200, r#"{"choices":[]}"#)]).await;
    let outcome = http_llm()
        .analyze_terminal(analyzer_input("$ ls"), analyzer_settings(&server.url))
        .await;
    assert!(outcome.result.is_err());
}

#[tokio::test]
async fn passes_reasoning_effort_when_reasoning_enabled() {
    let server = TestServer::start(vec![reply_json(
        200,
        &completion(json!(r#"{"state":"idle","reason":"test"}"#)),
    )])
    .await;
    let settings = ClassifierSettings {
        provider: Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::High,
        },
        ..analyzer_settings(&server.url)
    };
    let outcome = http_llm()
        .analyze_terminal(analyzer_input("$ ls"), settings)
        .await;
    assert!(outcome.result.is_ok());
    let request = server.requests()[0].json();
    assert_eq!(
        (
            request["reasoning_effort"].as_str(),
            request["stream"].as_bool()
        ),
        (Some("high"), Some(false))
    );
}

// --- retries (the SDK's policy) -------------------------------------------------------------

fn instant_retry() -> Vec<(&'static str, String)> {
    vec![("retry-after-ms", "0".to_owned())]
}

#[tokio::test]
async fn retries_rate_limits_and_server_errors_then_succeeds() {
    let server = TestServer::start(vec![
        json_with(429, r#"{"error":{"message":"slow down"}}"#, instant_retry()),
        json_with(503, "unavailable", instant_retry()),
        reply_json(200, &completion(json!("done"))),
    ])
    .await;
    let api = HttpLlm::new(RetryPolicy::default()).unwrap();
    assert_eq!(
        api.complete_chat(Vec::new(), chat_settings(&server.url))
            .await
            .unwrap(),
        "done"
    );
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn gives_up_after_the_last_retry_with_the_final_error() {
    let server = TestServer::start(vec![
        json_with(500, "boom", instant_retry()),
        json_with(500, "boom", instant_retry()),
        json_with(500, "<html>final</html>", instant_retry()),
    ])
    .await;
    let api = HttpLlm::new(RetryPolicy::default()).unwrap();
    let error = api
        .complete_chat(Vec::new(), chat_settings(&server.url))
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "500 <html>final</html>");
    assert_eq!(server.requests().len(), 3);
}

#[tokio::test]
async fn does_not_retry_client_errors_or_when_the_server_says_not_to() {
    let server = TestServer::start(vec![
        reply_json(401, r#"{"error":{"message":"bad key"}}"#),
        json_with(500, "{}", vec![("x-should-retry", "false".to_owned())]),
    ])
    .await;
    let api = HttpLlm::new(RetryPolicy::default()).unwrap();
    let first = api
        .complete_chat(Vec::new(), chat_settings(&server.url))
        .await
        .unwrap_err();
    assert_eq!(first.to_string(), "401 bad key\n{\"message\":\"bad key\"}");
    let second = api
        .complete_chat(Vec::new(), chat_settings(&server.url))
        .await
        .unwrap_err();
    assert_eq!(second.to_string(), "500 status code (no body)");
    assert_eq!(server.requests().len(), 2);
}

#[tokio::test]
async fn posts_plain_requests_and_reports_any_status() {
    let server = TestServer::start(vec![reply_json(429, "rate limited")]).await;
    let api = HttpLlm::new(RetryPolicy::default()).unwrap();
    let request = HttpPost {
        url: format!("{}/api/alpha/decisions", server.url),
        headers: vec![("Authorization".to_owned(), "Bearer k".to_owned())],
        body: "{}".to_owned(),
    };
    let response = api.fetch(request).await.unwrap();
    assert_eq!(
        response,
        HttpResponse {
            status: 429,
            status_text: "Too Many Requests".to_owned(),
            body: "rate limited".to_owned()
        }
    );
    assert!(!response.ok());
    assert_eq!(server.requests()[0].headers["authorization"], "Bearer k");
    assert_eq!(server.requests().len(), 1);
}

// --- cancel ---------------------------------------------------------------------------------

// `does nothing for unknown request id`: there are no request ids; a stream is cancelled by
// dropping it.

#[tokio::test]
async fn aborts_an_active_stream() {
    let (disconnected, client_gone) = oneshot::channel();
    let chunk = format!("data: {}\n\n", delta("start"));
    let server = TestServer::start(vec![Reply::StreamThenHold {
        chunks: vec![chunk],
        disconnected,
    }])
    .await;
    let mut stream = http_llm().send(Vec::new(), chat_settings(&server.url));
    assert_eq!(stream.next().await, Some(Ok("start".to_owned())));
    drop(stream);
    tokio::time::timeout(Duration::from_secs(5), client_gone)
        .await
        .unwrap()
        .unwrap();
}

// --- clearAnalyzerCache / analyzeTerminal cache ---------------------------------------------

#[test]
fn clear_analyzer_cache_empties_the_cache() {
    let llm = Llm::new(Arc::new(FakeLlm::default()), AnalyzerCache::default());
    llm.clear_analyzer_cache();
}

// `onDelta/onDone/onError return unsubscribe functions`: there are no listeners; `send`
// returns the stream itself.

fn idle_fake(reason: &'static str) -> FakeLlm {
    let fake = FakeLlm::default();
    fake.on_complete_chat(move |_, _| Ok(format!(r#"{{"state":"idle","reason":"{reason}"}}"#)));
    fake
}

fn analyze(llm: &Llm, buffer: &str) -> AnalyzeOutcome {
    block_on(llm.analyze_terminal(
        analyzer_input(buffer),
        analyzer_settings("https://api.openai.com"),
    ))
}

#[test]
fn cache_hit_returns_cached_result_with_cached_true() {
    let fake = idle_fake("waiting");
    let llm = Llm::new(Arc::new(fake.clone()), AnalyzerCache::default());
    let first = analyze(&llm, "$ ls").result.unwrap();
    assert!(!first.cached);
    assert_eq!(first.classification.reason, "waiting");
    let second = analyze(&llm, "$ ls").result.unwrap();
    assert!(second.cached);
    assert_eq!(second.classification, first.classification);
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn cache_is_shared_across_client_instances_survives_reconnects() {
    let fake = idle_fake("waiting");
    let cache = AnalyzerCache::default();
    let first = Llm::new(Arc::new(fake.clone()), cache.clone());
    assert!(analyze(&first, "$ shared-buffer").result.is_ok());
    drop(first);
    let second = Llm::new(Arc::new(fake.clone()), cache);
    assert!(analyze(&second, "$ shared-buffer").result.unwrap().cached);
    assert_eq!(fake.calls().len(), 1);
}

#[test]
fn clear_analyzer_cache_on_one_client_clears_the_global_cache() {
    let fake = idle_fake("waiting");
    let cache = AnalyzerCache::default();
    let first = Llm::new(Arc::new(fake.clone()), cache.clone());
    assert!(analyze(&first, "$ buf").result.is_ok());
    Llm::new(Arc::new(FakeLlm::default()), cache.clone()).clear_analyzer_cache();
    assert!(cache.is_empty());
    assert!(!analyze(&first, "$ buf").result.unwrap().cached);
    assert_eq!(fake.calls().len(), 2);
}

#[test]
fn cache_evicts_oldest_entry_when_full_fifo() {
    let fake = FakeLlm::default();
    let mut count = 0;
    fake.on_complete_chat(move |_, _| {
        count += 1;
        Ok(format!(r#"{{"state":"idle","reason":"r{count}"}}"#))
    });
    let llm = Llm::new(Arc::new(fake.clone()), AnalyzerCache::default());
    for i in 0..ANALYZER_CACHE_SIZE {
        assert!(analyze(&llm, &format!("buffer-{i}")).result.is_ok());
    }
    assert_eq!(fake.calls().len(), ANALYZER_CACHE_SIZE);
    // One more evicts the oldest (buffer-0).
    assert!(
        analyze(&llm, &format!("buffer-{ANALYZER_CACHE_SIZE}"))
            .result
            .is_ok()
    );
    assert_eq!(fake.calls().len(), ANALYZER_CACHE_SIZE + 1);
    // buffer-0 was evicted (re-querying re-inserts it, evicting the next-oldest).
    assert!(!analyze(&llm, "buffer-0").result.unwrap().cached);
    assert_eq!(fake.calls().len(), ANALYZER_CACHE_SIZE + 2);
    // One in the middle is still there.
    assert!(
        analyze(&llm, &format!("buffer-{}", ANALYZER_CACHE_SIZE / 2))
            .result
            .unwrap()
            .cached
    );
    assert_eq!(fake.calls().len(), ANALYZER_CACHE_SIZE + 2);
}

// --- generateTitle --------------------------------------------------------------------------

fn title_settings() -> TitleSettings {
    TitleSettings {
        base_url: "https://api.test/v1".to_owned(),
        api_key: "k".to_owned(),
        model: "title-model".to_owned(),
        title_system_prompt: "title prompt".to_owned(),
        reasoning_effort: ReasoningEffort::Low,
    }
}

#[test]
fn generates_a_title_from_the_fenced_json_answer() {
    let fake = FakeLlm::default();
    fake.on_complete_chat(|_, _| Ok("```json\n{\"title\":\"Fix build\",\"description\":\"d\",\"branchName\":\"fix-build\"}\n```".to_owned()));
    let llm = Llm::new(Arc::new(fake.clone()), AnalyzerCache::default());
    let outcome = block_on(llm.generate_title("$ make".to_owned(), title_settings()));
    let suggestion = TitleSuggestion {
        title: "Fix build".into(),
        description: "d".into(),
        branch_name: "fix-build".into(),
    };
    assert_eq!(
        outcome,
        TitleOutcome {
            result: Ok(suggestion),
            system_prompt: "title prompt".to_owned()
        }
    );
    assert_eq!(
        outcome.to_json(),
        r#"{"title":"Fix build","description":"d","branchName":"fix-build","systemPrompt":"title prompt"}"#
    );
    let (messages, settings) = fake.chat_calls().remove(0);
    assert_eq!(
        messages,
        vec![
            ChatMessage::system("title prompt"),
            ChatMessage::user("$ make")
        ]
    );
    assert_eq!(
        (settings.model.as_str(), settings.reasoning),
        ("title-model", ReasoningEffort::Low)
    );
}

#[test]
fn reads_missing_title_fields_as_empty_and_reports_failures() {
    let fake = FakeLlm::default();
    fake.on_complete_chat(|_, _| Ok(r#"{"title":5}"#.to_owned()));
    let llm = Llm::new(Arc::new(fake.clone()), AnalyzerCache::default());
    let outcome = block_on(llm.generate_title(String::new(), title_settings()));
    assert_eq!(
        outcome.result,
        Ok(TitleSuggestion {
            title: String::new(),
            description: String::new(),
            branch_name: String::new()
        })
    );
    fake.on_complete_chat(|_, _| Ok("null".to_owned()));
    assert!(
        block_on(llm.generate_title(String::new(), title_settings()))
            .result
            .is_err()
    );
    fake.on_complete_chat(|_, _| Err(LlmError::Timeout));
    let failed = block_on(llm.generate_title(String::new(), title_settings()));
    assert_eq!(failed.result, Err("Request timed out.".to_owned()));
    assert_eq!(
        failed.to_json(),
        r#"{"error":"Request timed out.","systemPrompt":"title prompt"}"#
    );
}

#[test]
fn records_analyze_outcomes_as_the_ts_result_objects() {
    let classification = Classification {
        state: ActivityState::Idle,
        reason: "r".to_owned(),
    };
    let fresh = AnalyzeOutcome {
        result: Ok(Analyzed {
            classification: classification.clone(),
            cached: false,
        }),
        system_prompt: "p".to_owned(),
    };
    assert_eq!(
        fresh.to_json(),
        r#"{"state":"idle","reason":"r","systemPrompt":"p"}"#
    );
    let cached = AnalyzeOutcome {
        result: Ok(Analyzed {
            classification,
            cached: true,
        }),
        system_prompt: "p".to_owned(),
    };
    assert_eq!(
        cached.to_json(),
        r#"{"state":"idle","reason":"r","cached":true,"systemPrompt":"p"}"#
    );
    let failed = AnalyzeOutcome {
        result: Err("e".to_owned()),
        system_prompt: "p".to_owned(),
    };
    assert_eq!(failed.to_json(), r#"{"error":"e","systemPrompt":"p"}"#);
}
