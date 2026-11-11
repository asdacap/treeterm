//! Scripted [`LlmApi`] for tests (here and in the GPUI model's): each call is recorded and
//! answered by a responder closure (the TS tests' `vi.fn().mockResolvedValue`).

use std::sync::{Arc, Mutex, PoisonError};

use futures::future::BoxFuture;
use futures::stream::{self, BoxStream};
use futures::{FutureExt, StreamExt};

use super::{ChatMessage, ChatSettings, HttpPost, HttpResponse, LlmApi, LlmError};

/// One recorded call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FakeCall {
    CompleteChat {
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    },
    StreamChat {
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    },
    Fetch(HttpPost),
}

type ChatResponder =
    Box<dyn FnMut(&[ChatMessage], &ChatSettings) -> Result<String, LlmError> + Send>;
type StreamResponder =
    Box<dyn FnMut(&[ChatMessage], &ChatSettings) -> Vec<Result<String, LlmError>> + Send>;
type FetchResponder = Box<dyn FnMut(&HttpPost) -> Result<HttpResponse, LlmError> + Send>;

struct State {
    calls: Vec<FakeCall>,
    chat: ChatResponder,
    stream: StreamResponder,
    fetch: FetchResponder,
}

fn unscripted(what: &str) -> LlmError {
    LlmError::Fetch(format!("FakeLlm: no {what} response scripted"))
}

/// Answers immediately; unscripted calls fail. Cheap to clone; clones share the script.
#[derive(Clone)]
pub struct FakeLlm {
    state: Arc<Mutex<State>>,
}

impl Default for FakeLlm {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(State {
                calls: Vec::new(),
                chat: Box::new(|_, _| Err(unscripted("chat"))),
                stream: Box::new(|_, _| vec![Err(unscripted("stream"))]),
                fetch: Box::new(|_| Err(unscripted("fetch"))),
            })),
        }
    }
}

impl FakeLlm {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn on_complete_chat(
        &self,
        responder: impl FnMut(&[ChatMessage], &ChatSettings) -> Result<String, LlmError>
        + Send
        + 'static,
    ) {
        self.lock().chat = Box::new(responder);
    }

    pub fn on_stream_chat(
        &self,
        responder: impl FnMut(&[ChatMessage], &ChatSettings) -> Vec<Result<String, LlmError>>
        + Send
        + 'static,
    ) {
        self.lock().stream = Box::new(responder);
    }

    pub fn on_fetch(
        &self,
        responder: impl FnMut(&HttpPost) -> Result<HttpResponse, LlmError> + Send + 'static,
    ) {
        self.lock().fetch = Box::new(responder);
    }

    pub fn calls(&self) -> Vec<FakeCall> {
        self.lock().calls.clone()
    }

    pub fn chat_calls(&self) -> Vec<(Vec<ChatMessage>, ChatSettings)> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                FakeCall::CompleteChat { messages, settings } => Some((messages, settings)),
                FakeCall::StreamChat { .. } | FakeCall::Fetch(_) => None,
            })
            .collect()
    }

    pub fn fetch_calls(&self) -> Vec<HttpPost> {
        self.calls()
            .into_iter()
            .filter_map(|call| match call {
                FakeCall::Fetch(request) => Some(request),
                FakeCall::CompleteChat { .. } | FakeCall::StreamChat { .. } => None,
            })
            .collect()
    }
}

impl LlmApi for FakeLlm {
    fn complete_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxFuture<'static, Result<String, LlmError>> {
        let mut state = self.lock();
        let result = (state.chat)(&messages, &settings);
        state
            .calls
            .push(FakeCall::CompleteChat { messages, settings });
        futures::future::ready(result).boxed()
    }

    fn stream_chat(
        &self,
        messages: Vec<ChatMessage>,
        settings: ChatSettings,
    ) -> BoxStream<'static, Result<String, LlmError>> {
        let mut state = self.lock();
        let items = (state.stream)(&messages, &settings);
        state
            .calls
            .push(FakeCall::StreamChat { messages, settings });
        stream::iter(items).boxed()
    }

    fn fetch(&self, request: HttpPost) -> BoxFuture<'static, Result<HttpResponse, LlmError>> {
        let mut state = self.lock();
        let result = (state.fetch)(&request);
        state.calls.push(FakeCall::Fetch(request));
        futures::future::ready(result).boxed()
    }
}
