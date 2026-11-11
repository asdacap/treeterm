//! Port of `classificationProvider.test.ts` and `classificationTypes.test.ts`.

use std::sync::Arc;

use futures::executor::block_on;
use serde_json::json;

use super::*;
use crate::llm::{AnalyzerCache, FakeLlm, HttpResponse, Llm};

fn criteria() -> ClassifierCriteria {
    ClassifierCriteria {
        idle: "Prompt at {{cwd}}".to_owned(),
        safe_permission_requested: "Only mutates {{safe_paths}}".to_owned(),
        ..ClassifierCriteria::default()
    }
}

fn input() -> ClassificationInput {
    ClassificationInput {
        buffer: "Allow write? y/n".to_owned(),
        cwd: "/workspace".to_owned(),
        safe_paths: vec!["/tmp".to_owned()],
        system_prompt: "Custom policy at {{cwd}}; safe: {{safe_paths}}".to_owned(),
        criteria: criteria(),
    }
}

fn jev() -> ClassifierSettings {
    ClassifierSettings {
        base_url: "https://openrouter.ai/api/v1".to_owned(),
        api_key: "test-secret".to_owned(),
        model: "typesafe/jev-1.13".to_owned(),
        provider: Provider::Classifier,
    }
}

fn chat() -> ClassifierSettings {
    ClassifierSettings {
        provider: Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::Off,
        },
        ..jev()
    }
}

fn decision(choice: &str) -> String {
    json!({"answers": {"activity_state": {"type": "choice", "choice": choice}}}).to_string()
}

fn ok(body: String) -> Result<HttpResponse, LlmError> {
    Ok(HttpResponse {
        status: 200,
        status_text: "OK".to_owned(),
        body,
    })
}

/// `transports()`: Decisions answers idle, the chat model answers idle/Waiting.
fn transports() -> FakeLlm {
    let fake = FakeLlm::default();
    fake.on_fetch(|_| ok(decision("idle")));
    fake.on_complete_chat(|_, _| Ok(r#"{"state":"idle","reason":"Waiting"}"#.to_owned()));
    fake
}

fn classify_with(
    fake: &FakeLlm,
    settings: &ClassifierSettings,
) -> Result<Classification, ClassifyError> {
    block_on(classify(fake, settings, &input()))
}

fn facade(fake: &FakeLlm) -> Llm {
    Llm::new(Arc::new(fake.clone()), AnalyzerCache::default())
}

// === OpenRouter Decisions contract ===

#[test]
fn uses_the_configured_origin_and_exact_decisions_request_preserving_custom_guidance_context() {
    let fake = transports();
    classify_with(&fake, &jev()).unwrap();
    let expected_criteria = ClassifierCriteria {
        idle: "Prompt at /workspace".to_owned(),
        safe_permission_requested: "Only mutates /tmp, /workspace".to_owned(),
        ..ClassifierCriteria::default()
    };
    let body = json!({
        "model": "typesafe/jev-1.13",
        "state": { "terminal": "Allow write? y/n", "cwd": "/workspace", "safe_paths": ["/tmp", "/workspace"] },
        "questions": { "activity_state": {
            "type": "choice",
            "instructions": {
                "guidance": "Custom policy at /workspace; safe: /tmp, /workspace",
                "output": "Choose the activity state using these criteria. Return the typed choice, not generated JSON or an explanation.",
            },
            "criteria": expected_criteria,
        } },
    });
    assert_eq!(
        fake.fetch_calls(),
        vec![HttpPost {
            url: "https://openrouter.ai/api/alpha/decisions".to_owned(),
            headers: vec![
                ("Authorization".to_owned(), "Bearer test-secret".to_owned()),
                ("Content-Type".to_owned(), "application/json".to_owned()),
            ],
            body: body.to_string(),
        }]
    );
    assert!(fake.chat_calls().is_empty());
    assert_eq!(
        decisions_url("https://example.test/proxy/api/v1/").unwrap(),
        "https://example.test/api/alpha/decisions"
    );
    assert_eq!(
        decisions_url("https://openrouter.ai/api/v1/").unwrap(),
        "https://openrouter.ai/api/alpha/decisions"
    );
}

#[test]
fn normalizes_each_state_without_requiring_confidence() {
    for state in ClassifiedState::ALL {
        let fake = transports();
        fake.on_fetch(move |_| ok(decision(state.as_str())));
        let expected = Classification {
            state: state.into(),
            reason: format!("Classifier decision: {}", state.as_str()),
        };
        assert_eq!(classify_with(&fake, &jev()).unwrap(), expected);
    }
}

#[test]
fn sends_any_configured_classifier_model_rather_than_hardcoding_jev() {
    let fake = transports();
    classify_with(
        &fake,
        &ClassifierSettings {
            model: "other/classifier".to_owned(),
            ..jev()
        },
    )
    .unwrap();
    let body: serde_json::Value = serde_json::from_str(&fake.fetch_calls()[0].body).unwrap();
    assert_eq!(body["model"], "other/classifier");
    assert!(fake.chat_calls().is_empty());
}

#[test]
fn allows_optional_response_metadata() {
    let fake = transports();
    let body = json!({"model": "resolved-model", "usage": {}, "answers": {"activity_state": {"type": "choice", "choice": "idle", "confidence": 0.9, "probabilities": {"idle": 1}}}});
    fake.on_fetch(move |_| ok(body.to_string()));
    assert_eq!(
        classify_with(&fake, &jev()).unwrap().state,
        ActivityState::Idle
    );
}

#[test]
fn rejects_invalid_response() {
    for body in [
        "invalid json",
        "{}",
        r#"{"answers":{}}"#,
        r#"{"answers":{"activity_state":{"type":"score","choice":"idle"}}}"#,
        r#"{"answers":{"activity_state":{"type":"choice","choice":"unknown"}}}"#,
        r#"{"answers":{"activity_state":{"type":"choice","choice":"error"}}}"#,
    ] {
        let fake = transports();
        fake.on_fetch(move |_| ok(body.to_owned()));
        assert!(
            matches!(
                classify_with(&fake, &jev()),
                Err(ClassifyError::InvalidDecision(_))
            ),
            "{body}"
        );
    }
}

#[test]
fn surfaces_http_status_without_falling_back() {
    for (status, body, message) in [
        (
            401,
            r#"{"error":{"code":401,"message":"Unauthorized"}}"#,
            "Unauthorized",
        ),
        (429, "<html>Rate limited</html>", "Too Many Requests"),
        (500, r#"{"unexpected":"body"}"#, "Internal Server Error"),
    ] {
        let fake = transports();
        fake.on_fetch(move |_| {
            Ok(HttpResponse {
                status,
                status_text: message.to_owned(),
                body: body.to_owned(),
            })
        });
        let error = classify_with(&fake, &jev()).unwrap_err();
        assert_eq!(
            error.to_string(),
            format!("OpenRouter Decisions HTTP {status}: {message}")
        );
        assert!(fake.chat_calls().is_empty());
    }
}

#[test]
fn returns_transport_errors_through_the_facade_and_does_not_cache_failures() {
    let fake = transports();
    fake.on_fetch(|_| Err(LlmError::Fetch("Network unavailable".to_owned())));
    let llm = facade(&fake);
    for _ in 0..2 {
        let outcome = block_on(llm.analyze_terminal(input(), jev()));
        assert_eq!(outcome.result, Err("Network unavailable".to_owned()));
        assert_eq!(
            outcome.system_prompt,
            "Custom policy at /workspace; safe: /tmp, /workspace"
        );
    }
    assert_eq!(fake.fetch_calls().len(), 2);
}

// === unified chat adapter and identity ===

#[test]
fn preserves_chat_messages_expanded_instructions_and_reasoning() {
    let fake = transports();
    assert_eq!(
        classify_with(&fake, &chat()).unwrap(),
        Classification {
            state: ActivityState::Idle,
            reason: "Waiting".to_owned()
        }
    );
    let (messages, settings) = fake.chat_calls().remove(0);
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].role, crate::llm::Role::System);
    assert_eq!(messages[1], ChatMessage::user("Allow write? y/n"));
    assert_eq!(
        settings,
        ChatSettings {
            base_url: "https://openrouter.ai/api/v1".to_owned(),
            api_key: "test-secret".to_owned(),
            model: "typesafe/jev-1.13".to_owned(),
            reasoning: ReasoningEffort::Off,
        }
    );
    assert!(fake.fetch_calls().is_empty());
}

#[test]
fn generates_the_output_format_and_state_list_from_the_configured_criteria() {
    let fake = transports();
    classify_with(&fake, &chat()).unwrap();
    let system = fake.chat_calls()[0].0[0].content.clone();
    assert!(system.starts_with(
        "Custom policy at /workspace; safe: /tmp, /workspace\n\nRespond with ONLY a JSON object: {\"state\": \"<state>\", \"reason\": \"<reason>\"}"
    ));
    assert!(system.contains(" - \"idle\": Prompt at /workspace"));
    assert!(system.contains(" - \"safe_permission_requested\": Only mutates /tmp, /workspace"));
    assert!(system.contains(&format!(
        "- \"working\": {}",
        ClassifierCriteria::default().working
    )));
    let order: Vec<usize> = [
        "working",
        "safe_permission_requested",
        "permission_request",
        "completed",
        "user_input_required",
        "application_error",
        "idle",
    ]
    .iter()
    .map(|state| system.find(&format!("- \"{state}\": ")).unwrap())
    .collect();
    let mut sorted = order.clone();
    sorted.sort_unstable();
    assert_eq!(order, sorted);
    assert!(!system.contains("\"error\""));
    assert!(system.ends_with(
        "The reason field should give the reason for the verdict, in no more than 10 words."
    ));
}

#[test]
fn reports_the_prompt_each_provider_actually_receives() {
    let fake = transports();
    let llm = facade(&fake);
    let chat_result = block_on(llm.analyze_terminal(input(), chat()));
    assert!(
        chat_result
            .system_prompt
            .contains(" - \"idle\": Prompt at /workspace")
    );
    llm.clear_analyzer_cache();
    let decisions_result = block_on(llm.analyze_terminal(input(), jev()));
    assert_eq!(
        decisions_result.system_prompt,
        "Custom policy at /workspace; safe: /tmp, /workspace"
    );
}

#[test]
fn rejects_invalid_chat_classification() {
    for raw in [
        r#"{"state":"unknown","reason":"oops"}"#,
        r#"{"state":"idle"}"#,
        r#"{"state":"idle","reason":3}"#,
        "null",
    ] {
        let fake = transports();
        fake.on_complete_chat(move |_, _| Ok(raw.to_owned()));
        assert!(
            matches!(
                classify_with(&fake, &chat()),
                Err(ClassifyError::InvalidClassification(_))
            ),
            "{raw}"
        );
    }
}

#[test]
fn isolates_global_cache_by_provider_endpoint_model_instructions_criteria_cwd_paths_and_reasoning()
{
    let fake = transports();
    let llm = facade(&fake);
    let run = |input: ClassificationInput, settings: ClassifierSettings| {
        block_on(llm.analyze_terminal(input, settings))
    };
    let edited_criteria = ClassifierCriteria {
        completed: "other".to_owned(),
        ..criteria()
    };
    run(input(), chat());
    run(
        input(),
        ClassifierSettings {
            model: "other".to_owned(),
            ..chat()
        },
    );
    run(
        input(),
        ClassifierSettings {
            base_url: "https://other.test/v1".to_owned(),
            ..chat()
        },
    );
    run(
        ClassificationInput {
            system_prompt: "other".to_owned(),
            ..input()
        },
        chat(),
    );
    run(
        ClassificationInput {
            criteria: edited_criteria,
            ..input()
        },
        chat(),
    );
    run(
        ClassificationInput {
            cwd: "/different".to_owned(),
            ..input()
        },
        chat(),
    );
    run(
        ClassificationInput {
            safe_paths: vec!["/different".to_owned()],
            ..input()
        },
        chat(),
    );
    run(
        input(),
        ClassifierSettings {
            provider: Provider::ChatCompletions {
                reasoning_effort: ReasoningEffort::High,
            },
            ..chat()
        },
    );
    run(input(), jev());
    assert_eq!(fake.chat_calls().len(), 8);
    assert_eq!(fake.fetch_calls().len(), 1);
    assert!(run(input(), chat()).result.unwrap().cached);
    assert!(
        !classification_identity(&input(), &chat())
            .unwrap()
            .contains("test-secret")
    );
}

#[test]
fn reports_an_invalid_endpoint_through_the_facade() {
    let llm = facade(&transports());
    let outcome = block_on(llm.analyze_terminal(
        input(),
        ClassifierSettings {
            base_url: "not a url".to_owned(),
            ..jev()
        },
    ));
    assert!(outcome.result.is_err());
}

// === classificationTypes.test.ts ===

#[test]
fn accepts_existing_activity_state() {
    for state in ActivityState::ALL {
        let raw = json!({"state": state, "reason": "classification reason"}).to_string();
        assert_eq!(
            parse_chat_classification(&raw).unwrap(),
            Classification {
                state,
                reason: "classification reason".to_owned()
            }
        );
    }
}

#[test]
fn rejects_invalid_classifier_output() {
    for raw in [
        "null",
        "{}",
        r#"{"state":"unknown","reason":""}"#,
        r#"{"state":"idle"}"#,
        r#"{"state":"idle","reason":42}"#,
    ] {
        assert!(parse_chat_classification(raw).is_err(), "{raw}");
    }
}

// `accepts configured provider %s and preserves empty models` / `rejects invalid persisted
// classifier configuration %j`: the settings schema, ported and tested in `settings`.

#[test]
fn names_the_protocol_provider_classifier_rather_than_a_particular_model() {
    assert_eq!(
        serde_json::to_value(ClassifierProvider::Classifier).unwrap(),
        "classifier"
    );
    assert_eq!(
        serde_json::to_value(ClassifierProvider::ChatCompletions).unwrap(),
        "chat_completions"
    );
    assert!(serde_json::from_str::<ClassifierProvider>("\"jev\"").is_err());
}

// --- Rust additions ---

#[test]
fn expands_templates_and_adds_cwd_to_the_safe_paths_once() {
    let prepared = prepare_classification_input(&ClassificationInput {
        safe_paths: vec![
            "/tmp".to_owned(),
            "/workspace".to_owned(),
            "/tmp".to_owned(),
        ],
        ..input()
    });
    assert_eq!(prepared.safe_paths, vec!["/tmp", "/workspace"]);
    assert_eq!(
        prepared.system_prompt,
        "Custom policy at /workspace; safe: /tmp, /workspace"
    );
    assert_eq!(
        prepared.criteria.get(ClassifiedState::Idle),
        "Prompt at /workspace"
    );
}

#[test]
fn builds_the_classifier_from_settings() {
    let mut settings = crate::settings::Settings::default();
    settings.terminal_analyzer.reasoning_effort = ReasoningEffort::High;
    let classifier = ClassifierSettings::from_settings(&settings.llm, &settings.terminal_analyzer);
    assert_eq!(
        classifier.provider,
        Provider::ChatCompletions {
            reasoning_effort: ReasoningEffort::High
        }
    );
    assert_eq!(
        classifier.provider.kind(),
        ClassifierProvider::ChatCompletions
    );
    settings.terminal_analyzer.provider = ClassifierProvider::Classifier;
    let classifier = ClassifierSettings::from_settings(&settings.llm, &settings.terminal_analyzer);
    assert_eq!(classifier.provider, Provider::Classifier);
    assert_eq!(classifier.base_url, settings.llm.base_url);
}
