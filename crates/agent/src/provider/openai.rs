//! OpenAI-compatible chat provider (OpenAI, DeepSeek, OpenRouter, Ollama `/v1`…).
//!
//! Wire-shape conventions adapted primarily from zeroclaw
//! `crates/zeroclaw-providers/src/openai.rs` (MIT/Apache-2.0), with claw-code
//! `rust/crates/api/src/providers/openai_compat.rs` (MIT) as secondary source;
//! see `dev-docs/agent/reference-analysis.md` C-C1.

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{ChatRequest, ChatResponse, ModelProvider, ProviderError, StreamEvent};
use crate::messages::{ChatMessage, ChatRole, TokenUsage, ToolCall};
use crate::tool::ToolSpec;

/// `POST {base_url}/chat/completions`.
const ENDPOINT: &str = "chat/completions";

pub struct OpenAiCompatProvider {
    http: reqwest::Client,
    base_url: String,
    api_key: Option<String>,
}

impl OpenAiCompatProvider {
    /// `base_url` is the API root, e.g. `https://api.openai.com/v1` or
    /// `http://localhost:11434/v1`. `api_key` is optional (Ollama / no-auth).
    pub fn new(base_url: impl Into<String>, api_key: Option<String>) -> Self {
        let http = reqwest::Client::builder()
            .timeout(Duration::from_secs(120))
            .build()
            .expect("reqwest client build is infallible");
        Self {
            http,
            base_url: base_url.into(),
            api_key,
        }
    }
}

#[async_trait]
impl ModelProvider for OpenAiCompatProvider {
    fn name(&self) -> &str {
        "openai_compat"
    }

    async fn chat(
        &self,
        request: &ChatRequest<'_>,
        model: &str,
    ) -> Result<ChatResponse, ProviderError> {
        let body = self.build_body(request, model, false)?;
        let text = self.send_json(&body).await?;

        let parsed: OpenAiChatResponse = serde_json::from_str(&text)
            .map_err(|e| ProviderError::Parse(format!("{e}: {text}")))?;

        Ok(parsed.into_response())
    }

    async fn chat_stream(
        &self,
        request: &ChatRequest<'_>,
        model: &str,
        on_event: &mut (dyn FnMut(StreamEvent) + Send),
    ) -> Result<ChatResponse, ProviderError> {
        let body = self.build_body(request, model, true)?;
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), ENDPOINT);

        let mut http_req = self.http.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            http_req = http_req.bearer_auth(key);
        }
        let resp = http_req
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        if !(200..300).contains(&status) {
            let text = resp
                .text()
                .await
                .map_err(|e| ProviderError::Transport(e.to_string()))?;
            return Err(ProviderError::Http { status, body: text });
        }

        let mut state = StreamState::default();
        let mut buffer: Vec<u8> = Vec::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ProviderError::Transport(e.to_string()))?;
            buffer.extend_from_slice(&chunk);
            // Drain complete lines, keep the trailing partial line buffered.
            while let Some(idx) = buffer.iter().position(|b| *b == b'\n') {
                let line = buffer.split_off(idx + 1);
                let line = std::mem::replace(&mut buffer, line);
                feed_line(&mut state, on_event, &String::from_utf8_lossy(&line));
            }
            if state.done {
                break;
            }
        }
        if !state.done && !buffer.is_empty() {
            feed_line(&mut state, on_event, &String::from_utf8_lossy(&buffer));
        }

        // Emit fully assembled tool calls, then finish.
        let tool_calls: Vec<ToolCall> = state
            .calls
            .into_values()
            .map(|c| {
                let id = if c.id.is_empty() {
                    format!("call_{}", c.name)
                } else {
                    c.id
                };
                ToolCall {
                    id,
                    name: c.name,
                    arguments: c.arguments,
                }
            })
            .collect();
        for call in &tool_calls {
            on_event(StreamEvent::ToolCall(call.clone()));
        }
        on_event(StreamEvent::Final);

        Ok(ChatResponse {
            text: (!state.text.is_empty()).then_some(state.text),
            tool_calls,
            usage: state.usage.map(TokenUsage::from),
        })
    }
}

impl OpenAiCompatProvider {
    fn build_body(
        &self,
        request: &ChatRequest<'_>,
        model: &str,
        stream: bool,
    ) -> Result<Value, ProviderError> {
        let mut payload = Map::new();
        payload.insert("model".into(), Value::String(model.to_string()));
        payload.insert(
            "messages".into(),
            Value::Array(request.messages.iter().map(to_wire_message).collect()),
        );
        if let Some(tools) = request.tools
            && !tools.is_empty()
        {
            payload.insert(
                "tools".into(),
                Value::Array(tools.iter().map(to_wire_tool).collect()),
            );
        }
        if let Some(temperature) = request.temperature {
            payload.insert("temperature".into(), Value::from(temperature));
        }
        if let Some(max_tokens) = request.max_tokens {
            payload.insert("max_tokens".into(), Value::from(max_tokens));
        }
        if let Some(stop) = &request.stop
            && !stop.is_empty()
        {
            payload.insert(
                "stop".into(),
                Value::Array(stop.iter().map(|s| Value::String(s.clone())).collect()),
            );
        }
        if stream {
            payload.insert("stream".into(), Value::Bool(true));
            payload.insert("stream_options".into(), json!({ "include_usage": true }));
        }
        Ok(Value::Object(payload))
    }

    async fn send_json(&self, body: &Value) -> Result<String, ProviderError> {
        let url = format!("{}/{}", self.base_url.trim_end_matches('/'), ENDPOINT);
        let mut http_req = self.http.post(&url).json(body);
        if let Some(key) = &self.api_key {
            http_req = http_req.bearer_auth(key);
        }
        let resp = http_req
            .send()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        let status = resp.status().as_u16();
        let text = resp
            .text()
            .await
            .map_err(|e| ProviderError::Transport(e.to_string()))?;
        if !(200..300).contains(&status) {
            return Err(ProviderError::Http { status, body: text });
        }
        Ok(text)
    }
}

/// Streaming accumulation state + SSE feed (wire conventions adapted from
/// zeroclaw `zeroclaw-providers/src/openai.rs`; reference-analysis C-C1).
#[derive(Default)]
struct StreamState {
    done: bool,
    text: String,
    reasoning: String,
    calls: BTreeMap<u32, StreamCall>,
    next_index: u32,
    usage: Option<OpenAiUsage>,
}

#[derive(Default)]
struct StreamCall {
    id: String,
    name: String,
    arguments: String,
}

fn feed_line(state: &mut StreamState, on_event: &mut (dyn FnMut(StreamEvent) + Send), line: &str) {
    if state.done {
        return;
    }
    let line = line.trim_end();
    if line.is_empty() {
        return;
    }
    let Some(payload) = line.strip_prefix("data:") else {
        return; // ignore `event:`/`id:`/`: keepalive` lines
    };
    let payload = payload.trim_start();
    if payload == "[DONE]" {
        state.done = true;
        return;
    }
    let chunk: ChatStreamChunk = match serde_json::from_str(payload) {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Some(u) = chunk.usage {
        on_event(StreamEvent::Usage(u.clone().into()));
        state.usage = Some(u);
    }

    for choice in chunk.choices {
        let delta = choice.delta;
        if let Some(content) = delta.content
            && !content.is_empty()
        {
            state.text.push_str(&content);
            on_event(StreamEvent::TextDelta { delta: content });
        }
        // DeepSeek/GLM style reasoning surfaces in streaming deltas.
        for reasoning in [delta.reasoning_content, delta.reasoning]
            .into_iter()
            .flatten()
            .filter(|r| !r.is_empty())
        {
            state.reasoning.push_str(&reasoning);
            on_event(StreamEvent::ReasoningDelta { delta: reasoning });
        }
        if let Some(calls) = delta.tool_calls {
            for call in calls {
                let index = call.index.unwrap_or_else(|| {
                    let i = state.next_index;
                    state.next_index += 1;
                    i
                });
                let entry = state.calls.entry(index).or_default();
                if let Some(id) = call.id {
                    entry.id = id;
                }
                if let Some(function) = call.function {
                    if let Some(name) = function.name {
                        entry.name = name;
                    }
                    if let Some(args) = function.arguments {
                        entry.arguments.push_str(&args);
                    }
                }
            }
        }
    }
}

// ── wire mapping ────────────────────────────────────────────────────────────

fn to_wire_role(role: ChatRole) -> &'static str {
    role.as_wire()
}

fn to_wire_message(msg: &ChatMessage) -> Value {
    let mut m = Map::new();
    m.insert("role".into(), Value::String(to_wire_role(msg.role).into()));

    // content may legitimately be null (assistant with only tool_calls).
    m.insert(
        "content".into(),
        msg.content
            .as_deref()
            .map_or(Value::Null, |c| Value::String(c.to_string())),
    );

    // Only attach tool_calls when non-empty: some providers reject an explicit
    // empty array on assistant messages.
    if let Some(calls) = &msg.tool_calls
        && !calls.is_empty()
    {
        let wire: Vec<Value> = calls
            .iter()
            .map(|c| {
                json!({
                    "id": c.id,
                    "type": "function",
                    "function": { "name": c.name, "arguments": c.arguments }
                })
            })
            .collect();
        m.insert("tool_calls".into(), Value::Array(wire));
    }

    if let Some(id) = &msg.tool_call_id {
        m.insert("tool_call_id".into(), Value::String(id.clone()));
    }

    Value::Object(m)
}

fn to_wire_tool(spec: &ToolSpec) -> Value {
    json!({
        "type": "function",
        "function": {
            "name": spec.name,
            "description": spec.description,
            "parameters": spec.parameters,
        }
    })
}

// ── response types ──────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

impl OpenAiChatResponse {
    fn into_response(self) -> ChatResponse {
        // content may be a string or (rarely) a structured value.
        let message = self.choices.into_iter().next().map(|c| c.message);
        let (text, tool_calls) = match message {
            Some(m) => {
                let text = m.content.as_ref().and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    Value::Null => None,
                    other => Some(other.to_string()),
                });
                let calls = m
                    .tool_calls
                    .unwrap_or_default()
                    .into_iter()
                    .map(Into::into)
                    .collect();
                (text, calls)
            }
            None => (None, Vec::new()),
        };
        ChatResponse {
            text,
            tool_calls,
            usage: self.usage.map(Into::into),
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: Option<Value>,
    // Some providers emit `"tool_calls": null`; Option<Vec> handles both.
    #[serde(default)]
    tool_calls: Option<Vec<OpenAiToolCall>>,
}

#[derive(Debug, Deserialize)]
struct OpenAiToolCall {
    #[serde(default)]
    id: Option<String>,
    function: OpenAiFunction,
}

#[derive(Debug, Deserialize)]
struct OpenAiFunction {
    name: String,
    #[serde(default)]
    arguments: String,
}

impl From<OpenAiToolCall> for ToolCall {
    fn from(c: OpenAiToolCall) -> Self {
        let id = c.id.unwrap_or_default();
        // Synthesise an id when the provider omitted one, so history pairing
        // and events stay consistent.
        let id = if id.is_empty() {
            format!("call_{}", c.function.name)
        } else {
            id
        };
        Self {
            id,
            name: c.function.name,
            arguments: c.function.arguments,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
struct OpenAiUsage {
    #[serde(default)]
    prompt_tokens: Option<u64>,
    #[serde(default)]
    completion_tokens: Option<u64>,
    /// DeepSeek field for auto KV-cache hits.
    #[serde(default)]
    prompt_cache_hit_tokens: Option<u64>,
    /// OpenAI field (non-streaming chat completions): `prompt_tokens_details.cached_tokens`.
    #[serde(default)]
    prompt_tokens_details: Option<PromptTokensDetails>,
}

#[derive(Debug, Clone, Deserialize)]
struct PromptTokensDetails {
    #[serde(default)]
    cached_tokens: Option<u64>,
}

impl From<OpenAiUsage> for TokenUsage {
    fn from(u: OpenAiUsage) -> Self {
        let cache_read = u.prompt_cache_hit_tokens.or_else(|| {
            u.prompt_tokens_details
                .as_ref()
                .and_then(|d| d.cached_tokens)
        });
        Self {
            input_tokens: u.prompt_tokens,
            output_tokens: u.completion_tokens,
            cache_read,
            cache_write: None,
        }
    }
}

// ── streaming response types ────────────────────────────────────────────────

/// One SSE `data:` frame of a streaming chat completion.
#[derive(Debug, Deserialize)]
struct ChatStreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<OpenAiUsage>,
}

#[derive(Debug, Deserialize)]
struct StreamChoice {
    #[serde(default)]
    delta: StreamDelta,
}

#[derive(Debug, Deserialize, Default)]
struct StreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    reasoning_content: Option<String>,
    #[serde(default)]
    reasoning: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<StreamToolCallDelta>>,
}

#[derive(Debug, Deserialize)]
struct StreamToolCallDelta {
    #[serde(default)]
    index: Option<u32>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<StreamFunctionDelta>,
}

#[derive(Debug, Deserialize)]
struct StreamFunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

use serde_json::json;
