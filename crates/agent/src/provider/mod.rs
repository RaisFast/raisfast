pub mod openai;

use async_trait::async_trait;

use crate::messages::{ChatMessage, TokenUsage, ToolCall};
use crate::tool::ToolSpec;

/// A chat request to a provider.
#[derive(Debug)]
pub struct ChatRequest<'a> {
    pub messages: &'a [ChatMessage],
    pub tools: Option<&'a [ToolSpec]>,
    pub temperature: Option<f64>,
    /// Sampling budget; provider wire name `max_tokens` (None = server default).
    pub max_tokens: Option<i64>,
    /// Up to 4 stop sequences (OpenAI wire limit).
    pub stop: Option<Vec<String>>,
}

/// A completed non-streaming chat response.
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub usage: Option<TokenUsage>,
}

impl ChatResponse {
    pub fn text_only(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            tool_calls: Vec::new(),
            usage: None,
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        !self.tool_calls.is_empty()
    }
}

/// A live event while a response is being generated.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    TextDelta {
        delta: String,
    },
    /// Reasoning/thinking delta (shown separately, never merged into text).
    ReasoningDelta {
        delta: String,
    },
    /// A fully assembled tool call (emitted at end of stream).
    ToolCall(ToolCall),
    Usage(TokenUsage),
    Final,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("provider config error: {0}")]
    Config(String),
    #[error("http {status}: {body}")]
    Http { status: u16, body: String },
    #[error("transport error: {0}")]
    Transport(String),
    #[error("cannot parse provider response: {0}")]
    Parse(String),
}

/// Abstraction over an LLM chat provider (non-streaming for MVP).
#[async_trait]
pub trait ModelProvider: Send + Sync {
    fn name(&self) -> &str;

    async fn chat(
        &self,
        request: &ChatRequest<'_>,
        model: &str,
    ) -> Result<ChatResponse, ProviderError>;

    /// Streaming variant: feed incremental events to `on_event` while the
    /// response is produced, and return the fully assembled response.
    /// Default = non-streaming `chat` replayed as a single batch of events.
    async fn chat_stream(
        &self,
        request: &ChatRequest<'_>,
        model: &str,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<ChatResponse, ProviderError> {
        let response = self.chat(request, model).await?;
        if let Some(text) = &response.text {
            on_event(StreamEvent::TextDelta {
                delta: text.clone(),
            });
        }
        for call in &response.tool_calls {
            on_event(StreamEvent::ToolCall(call.clone()));
        }
        if let Some(usage) = response.usage {
            on_event(StreamEvent::Usage(usage));
        }
        on_event(StreamEvent::Final);
        Ok(response)
    }
}
