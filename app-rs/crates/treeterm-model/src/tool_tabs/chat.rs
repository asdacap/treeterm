//! The Chat tab (`Chat.tsx`): a conversation with the configured OpenAI-compatible model,
//! streamed. Tab state is Electron's `ChatState` (`{ messages: [{ id, role, content }] }`),
//! written after every change as Electron does.

use futures::StreamExt as _;
use gpui_kit::{App, Context, EventEmitter, Task, WeakEntity};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use treeterm_core::llm::{ChatMessage as LlmMessage, ChatSettings, Role};
use treeterm_core::settings::{LlmSettings, ReasoningEffort};

use crate::analyzer::LlmService;
use crate::workspace_store::WorkspaceStore;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatRole {
    User,
    Assistant,
}

/// One message (`ChatMessage`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatEntry {
    pub id: String,
    pub role: ChatRole,
    pub content: String,
}

pub const MISSING_API_KEY: &str = "LLM API key not configured. Go to Settings > LLM to set it up.";

#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    /// The tab's new state, to persist.
    StateChanged(Value),
}

impl EventEmitter<ChatEvent> for ChatModel {}

enum Stream {
    Idle,
    /// Dropping the task drops the request's stream, which cancels it.
    Running { _task: Task<()> },
}

pub struct ChatModel {
    /// `None`: this session has no LLM client (sending fails with an error).
    llm: Option<LlmService>,
    /// Where the endpoint settings are read from (they change live).
    workspace: WeakEntity<WorkspaceStore>,
    messages: Vec<ChatEntry>,
    reasoning: ReasoningEffort,
    stream: Stream,
    error: Option<String>,
}

impl ChatModel {
    pub fn new(llm: Option<LlmService>, state: &Value, workspace: WeakEntity<WorkspaceStore>) -> Self {
        let (messages, error) = match state.get("messages") {
            None => (Vec::new(), None),
            Some(saved) => match serde_json::from_value::<Vec<ChatEntry>>(saved.clone()) {
                Ok(messages) => (messages, None),
                Err(error) => (Vec::new(), Some(format!("The saved conversation could not be read: {error}"))),
            },
        };
        Self { llm, workspace, messages, reasoning: ReasoningEffort::Off, stream: Stream::Idle, error }
    }

    pub fn messages(&self) -> &[ChatEntry] {
        &self.messages
    }

    pub fn is_streaming(&self) -> bool {
        match self.stream {
            Stream::Idle => false,
            Stream::Running { .. } => true,
        }
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn reasoning(&self) -> ReasoningEffort {
        self.reasoning
    }

    pub fn set_reasoning(&mut self, reasoning: ReasoningEffort, cx: &mut Context<Self>) {
        self.reasoning = reasoning;
        cx.notify();
    }

    /// The endpoint settings, read now.
    pub fn llm_settings(&self, cx: &App) -> Option<LlmSettings> {
        Some(self.workspace.upgrade()?.read(cx).settings().llm.clone())
    }

    fn persist(&self, cx: &mut Context<Self>) {
        cx.emit(ChatEvent::StateChanged(json!({ "messages": self.messages })));
    }

    /// Send `input`; whether it was taken (the input box is cleared then). Nothing happens
    /// while a reply streams or for blank input.
    pub fn send(&mut self, input: &str, cx: &mut Context<Self>) -> bool {
        let text = input.trim();
        if text.is_empty() || self.is_streaming() {
            return false;
        }
        let Some(settings) = self.llm_settings(cx) else {
            return false; // the workspace is closing
        };
        if settings.api_key.is_empty() {
            self.error = Some(MISSING_API_KEY.to_owned());
            cx.notify();
            return false;
        }
        let Some(llm) = self.llm.clone() else {
            self.error = Some("No LLM client is available in this session".to_owned());
            cx.notify();
            return false;
        };
        self.error = None;
        self.messages.push(ChatEntry { id: uuid::Uuid::new_v4().to_string(), role: ChatRole::User, content: text.to_owned() });
        self.messages.push(ChatEntry { id: uuid::Uuid::new_v4().to_string(), role: ChatRole::Assistant, content: String::new() });
        // The history so far, without the empty reply being written.
        let history: Vec<LlmMessage> = self
            .messages
            .iter()
            .filter(|m| !m.content.is_empty())
            .map(|m| LlmMessage {
                role: match m.role {
                    ChatRole::User => Role::User,
                    ChatRole::Assistant => Role::Assistant,
                },
                content: m.content.clone(),
            })
            .collect();
        let chat = ChatSettings {
            base_url: settings.base_url,
            api_key: settings.api_key,
            model: settings.model,
            reasoning: self.reasoning,
        };
        let mut deltas = llm.rt().subscribe(llm.llm().send(history, chat));
        let task = cx.spawn(async move |this, cx| {
            while let Some(delta) = deltas.next().await {
                let more = this.update(cx, |model, cx| match delta {
                    Ok(text) => {
                        model.append(&text, cx);
                        true
                    }
                    Err(error) => {
                        model.error = Some(error.to_string());
                        model.stream = Stream::Idle;
                        cx.notify();
                        false
                    }
                });
                match more {
                    Ok(true) => {}
                    Ok(false) | Err(_) => return, // failed, or the tab closed
                }
            }
            match this.update(cx, |model, cx| {
                model.stream = Stream::Idle;
                cx.notify();
            }) {
                Ok(()) | Err(_) => {}
            }
        });
        self.stream = Stream::Running { _task: task };
        self.persist(cx);
        cx.notify();
        true
    }

    /// Add streamed text to the reply being written.
    fn append(&mut self, text: &str, cx: &mut Context<Self>) {
        match self.messages.last_mut() {
            Some(last) if last.role == ChatRole::Assistant => last.content.push_str(text),
            Some(_) | None => return,
        }
        self.persist(cx);
        cx.notify();
    }

    /// Stop the reply; what arrived so far stays.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        self.stream = Stream::Idle;
        cx.notify();
    }
}

/// The reasoning choices (`<select>`), in order.
pub const REASONING_CHOICES: [(ReasoningEffort, &str); 4] = [
    (ReasoningEffort::Off, "Off"),
    (ReasoningEffort::Low, "Low"),
    (ReasoningEffort::Medium, "Medium"),
    (ReasoningEffort::High, "High"),
];
