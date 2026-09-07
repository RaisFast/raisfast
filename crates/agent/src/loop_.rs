//! Minimal native function-calling turn loop (MVP).
//!
//! One `run()` = one user turn: repeatedly call the model, execute any
//! requested tools (feeding results back), until the model answers without
//! tool calls or the iteration cap is reached. Conversation state lives in
//! the caller-owned `history`; the engine keeps no durable state (full design:
//! `dev-docs/agent/loop-engine.md`).

use std::sync::Arc;

use serde_json::Value;
use tokio_util::sync::CancellationToken;

use crate::memory::{Memory, render_memory_context};
use crate::messages::{ChatMessage, ChatRole, TokenUsage};
use crate::provider::{ChatRequest, ModelProvider, ProviderError, StreamEvent};
use crate::tool::ToolRegistry;

#[derive(Debug, Clone, Copy)]
pub struct TurnConfig {
    /// Maximum model round-trips inside one turn.
    pub max_iterations: usize,
    pub temperature: Option<f64>,
}

impl Default for TurnConfig {
    fn default() -> Self {
        Self {
            max_iterations: 10,
            temperature: None,
        }
    }
}

/// Events surfaced to the caller (UI/tool trace). Not persisted by the engine.
#[derive(Debug, Clone)]
pub enum TurnEvent {
    /// Live text delta (only emitted by `run_streamed`).
    Chunk { delta: String },
    /// Live reasoning/thinking delta (only emitted by `run_streamed`).
    Thinking { delta: String },
    /// Assistant text produced during an iteration (non-streaming `run`).
    Text { text: String },
    /// The model requested a tool.
    ToolCall { name: String, arguments: Value },
    /// A tool finished; `output` is what was fed back to the model.
    ToolResult { name: String, output: String },
}

#[derive(Debug, thiserror::Error)]
pub enum TurnError {
    #[error("provider error: {0}")]
    Provider(#[from] ProviderError),
}

#[derive(Debug, Default)]
pub struct TurnOutcome {
    pub text: String,
    pub events: Vec<TurnEvent>,
    pub iterations: usize,
    pub tool_calls_made: usize,
    pub usage: Option<TokenUsage>,
    /// One entry per LLM call, aligned with the appended assistant rows.
    pub per_call_usage: Vec<TokenUsage>,
    /// True when the turn was cancelled (partial state should be persisted).
    pub cancelled: bool,
}

/// A model + tool registry + config bound for repeated turns.
pub struct TurnEngine {
    provider: Arc<dyn ModelProvider>,
    model: String,
    tools: Arc<ToolRegistry>,
    cfg: TurnConfig,
    memory: Option<Arc<dyn Memory>>,
    cancel: Option<CancellationToken>,
}

impl TurnEngine {
    pub fn new(
        provider: Arc<dyn ModelProvider>,
        model: impl Into<String>,
        tools: Arc<ToolRegistry>,
        cfg: TurnConfig,
    ) -> Self {
        Self {
            provider,
            model: model.into(),
            tools,
            cfg,
            memory: None,
            cancel: None,
        }
    }

    /// Attach a long-term memory handle (facts are injected before each user
    /// turn; the agent may also read/write them via the `memory_*` tools).
    pub fn with_memory(mut self, memory: Arc<dyn Memory>) -> Self {
        self.memory = Some(memory);
        self
    }

    /// Attach a cancellation token: when cancelled the turn stops at the next
    /// checkpoint and returns a partial outcome (`cancelled = true`).
    pub fn with_cancel(mut self, cancel: CancellationToken) -> Self {
        self.cancel = Some(cancel);
        self
    }

    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(CancellationToken::is_cancelled)
    }

    /// Run one turn: append `user`, loop until terminal or `max_iterations`.
    /// All messages produced are appended to `history` (caller persists).
    pub async fn run(
        &self,
        history: &mut Vec<ChatMessage>,
        system: Option<&str>,
        user: &str,
    ) -> Result<TurnOutcome, TurnError> {
        self.run_impl(history, system, user, None).await
    }

    /// Like [`Self::run`], but text and reasoning are delivered live as
    /// [`TurnEvent::Chunk`]/[`TurnEvent::Thinking`] while generated.
    pub async fn run_streamed(
        &self,
        history: &mut Vec<ChatMessage>,
        system: Option<&str>,
        user: &str,
        on_event: &mut (dyn FnMut(TurnEvent) + Send),
    ) -> Result<TurnOutcome, TurnError> {
        self.run_impl(history, system, user, Some(on_event)).await
    }

    async fn run_impl(
        &self,
        history: &mut Vec<ChatMessage>,
        system: Option<&str>,
        user: &str,
        mut emitter: Option<&mut (dyn FnMut(TurnEvent) + Send)>,
    ) -> Result<TurnOutcome, TurnError> {
        // Ensure a single leading system message exists.
        if let Some(system) = system
            && !matches!(history.first(), Some(m) if m.role == ChatRole::System)
        {
            history.insert(0, ChatMessage::system(system));
        }
        // Memory injection: recall relevant facts and prepend the
        // `[Memory context]` block to the user message (idempotent per message).
        let user_msg = if let Some(memory) = &self.memory {
            let prefixed = user.starts_with("[Memory context]");
            match (prefixed, memory.recall(Some(user), 4).await) {
                (true, _) => user.to_string(),
                (false, Ok(entries)) => match render_memory_context(&entries) {
                    Some(block) => format!("{block}\n\n{user}"),
                    None => user.to_string(),
                },
                // Recall failures must never fail a turn.
                (false, Err(_)) => user.to_string(),
            }
        } else {
            user.to_string()
        };
        history.push(ChatMessage::user(user_msg));

        let specs = self.tools.specs();
        let tools_arg = (!specs.is_empty()).then_some(specs.as_slice());

        let streaming = emitter.is_some();
        let mut outcome = TurnOutcome::default();
        let mut narration: Option<String> = None;

        loop {
            // Cancellation is a first-class checkpoint between iterations.
            if self.cancelled() {
                outcome.cancelled = true;
                break;
            }
            if outcome.iterations >= self.cfg.max_iterations {
                if outcome.text.is_empty() {
                    outcome.text = narration
                        .take()
                        .unwrap_or_else(|| "已到最大迭代次数，尚未收敛".to_string());
                }
                break;
            }
            outcome.iterations += 1;

            let request = ChatRequest {
                messages: history.as_slice(),
                tools: tools_arg,
                temperature: self.cfg.temperature,
                max_tokens: None,
                stop: None,
            };
            let resp = {
                match emitter.take() {
                    Some(emit) => {
                        let result = {
                            let mut stream = |event: StreamEvent| match event {
                                StreamEvent::TextDelta { delta } => {
                                    emit(TurnEvent::Chunk { delta });
                                }
                                StreamEvent::ReasoningDelta { delta } => {
                                    emit(TurnEvent::Thinking { delta });
                                }
                                // Tool call / usage / final are not surfaced here;
                                // the engine re-emits tool calls around execution
                                // and settles usage itself.
                                StreamEvent::ToolCall(_)
                                | StreamEvent::Usage(_)
                                | StreamEvent::Final => {}
                            };
                            self.provider
                                .chat_stream(&request, &self.model, &mut stream)
                                .await
                        }?;
                        emitter = Some(emit);
                        result
                    }
                    None => self.provider.chat(&request, &self.model).await?,
                }
            };

            // Per-iteration usage aligned with the assistant rows appended below.
            let call_usage = match resp.usage {
                Some(u) => {
                    match &mut outcome.usage {
                        Some(acc) => acc.accumulate(u),
                        None => outcome.usage = Some(u),
                    }
                    u
                }
                None => TokenUsage::default(),
            };
            outcome.per_call_usage.push(call_usage);

            // Assistant text (any iteration) is narration; terminal if no tools.
            let text = resp.text.clone().filter(|t| !t.trim().is_empty());
            if !streaming && let Some(t) = &text {
                narration = Some(t.clone());
                outcome.events.push(TurnEvent::Text { text: t.clone() });
            }

            // Don't start a new tool round after cancellation fired mid-call.
            if self.cancelled() {
                outcome.cancelled = true;
                break;
            }

            if !resp.has_tool_calls() {
                outcome.text = text
                    .clone()
                    .or(narration.clone())
                    .unwrap_or_default()
                    .trim()
                    .to_string();
                // The terminal assistant message is part of the canonical
                // history (needed for multi-turn continuity and persistence).
                history.push(ChatMessage::assistant(text.clone(), None));
                break;
            }

            // Keep the assistant message with its tool_calls for the provider,
            // then execute each requested tool sequentially and feed back.
            history.push(ChatMessage::assistant(
                text.clone(),
                Some(resp.tool_calls.clone()),
            ));
            for call in &resp.tool_calls {
                outcome.tool_calls_made += 1;
                let arguments = serde_json::from_str(&call.arguments)
                    .unwrap_or(Value::String(call.arguments.clone()));
                let ev = TurnEvent::ToolCall {
                    name: call.name.clone(),
                    arguments: arguments.clone(),
                };
                outcome.events.push(ev.clone());
                if let Some(cb) = emitter.as_mut() {
                    (*cb)(ev);
                }

                let output = match self.tools.get(&call.name) {
                    Some(tool) => match tool.execute(arguments).await {
                        Ok(o) => o,
                        Err(e) => format!("工具执行失败: {e}"),
                    },
                    None => format!("工具不存在: {}", call.name),
                };
                let ev = TurnEvent::ToolResult {
                    name: call.name.clone(),
                    output: output.clone(),
                };
                outcome.events.push(ev.clone());
                if let Some(cb) = emitter.as_mut() {
                    (*cb)(ev);
                }
                history.push(ChatMessage::tool(call.id.clone(), output));
            }
        }

        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory::{InMemoryMemory, Memory};
    use crate::messages::{ChatMessage, ChatRole, ToolCall};
    use crate::provider::{ChatRequest, ChatResponse, ModelProvider, ProviderError};
    use crate::tool::Tool;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    fn num_params_schema() -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "a": { "type": "number" },
                "b": { "type": "number" }
            },
            "required": ["a", "b"]
        })
    }

    /// Scripted provider: each call pops the next canned response.
    struct ScriptedProvider {
        responses: Mutex<VecDeque<ChatResponse>>,
    }

    #[async_trait]
    impl ModelProvider for ScriptedProvider {
        fn name(&self) -> &str {
            "scripted"
        }

        async fn chat(
            &self,
            _request: &ChatRequest<'_>,
            _model: &str,
        ) -> Result<ChatResponse, ProviderError> {
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .ok_or_else(|| ProviderError::Config("test script exhausted".into()))
        }
    }

    fn text_response(text: &str) -> ChatResponse {
        ChatResponse {
            text: Some(text.to_string()),
            tool_calls: Vec::new(),
            usage: None,
        }
    }

    fn tool_calls_response(calls: Vec<ToolCall>) -> ChatResponse {
        ChatResponse {
            text: None,
            tool_calls: calls,
            usage: None,
        }
    }

    fn call(id: &str, name: &str, a: f64, b: f64) -> ToolCall {
        ToolCall {
            id: id.to_string(),
            name: name.to_string(),
            arguments: serde_json::json!({ "a": a, "b": b }).to_string(),
        }
    }

    struct AddTool;

    #[async_trait]
    impl Tool for AddTool {
        fn name(&self) -> &str {
            "add"
        }
        fn description(&self) -> &str {
            "Add two numbers"
        }
        fn parameters_schema(&self) -> Value {
            num_params_schema()
        }
        async fn execute(&self, args: Value) -> crate::tool::ToolExecution {
            let a = args.get("a").and_then(Value::as_f64).ok_or("a required")?;
            let b = args.get("b").and_then(Value::as_f64).ok_or("b required")?;
            Ok(format!("{}", a + b))
        }
    }

    struct MulTool;

    #[async_trait]
    impl Tool for MulTool {
        fn name(&self) -> &str {
            "mul"
        }
        fn description(&self) -> &str {
            "Multiply two numbers"
        }
        fn parameters_schema(&self) -> Value {
            num_params_schema()
        }
        async fn execute(&self, args: Value) -> crate::tool::ToolExecution {
            let a = args.get("a").and_then(Value::as_f64).ok_or("a required")?;
            let b = args.get("b").and_then(Value::as_f64).ok_or("b required")?;
            Ok(format!("{}", a * b))
        }
    }

    fn engine_with(tools: Vec<Arc<dyn Tool>>, provider: Arc<dyn ModelProvider>) -> TurnEngine {
        let mut reg = ToolRegistry::new();
        for t in tools {
            reg.push(t);
        }
        TurnEngine::new(provider, "test-model", Arc::new(reg), TurnConfig::default())
    }

    #[tokio::test]
    async fn single_tool_chain_feeds_result_and_terminates() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                tool_calls_response(vec![call("c1", "add", 1.0, 2.0)]),
                text_response("结果是 3"),
            ])),
        });
        let engine = engine_with(vec![Arc::new(AddTool)], provider);

        let mut history = Vec::new();
        let outcome = engine
            .run(&mut history, Some("你是助手"), "1+2?")
            .await
            .unwrap();

        assert_eq!(outcome.iterations, 2);
        assert_eq!(outcome.tool_calls_made, 1);
        assert!(outcome.text.contains('3'));
        // system only once, at the front
        assert!(matches!(history.first(), Some(m) if m.role == ChatRole::System));
        // assistant(tool_calls) then tool result are in history
        assert!(
            history
                .iter()
                .any(|m| matches!(&m.role, ChatRole::Assistant))
        );
        let tool_msgs: Vec<&ChatMessage> = history
            .iter()
            .filter(|m| m.role == ChatRole::Tool)
            .collect();
        assert_eq!(tool_msgs.len(), 1);
        assert_eq!(tool_msgs[0].content.as_deref(), Some("3"));
        assert_eq!(tool_msgs[0].tool_call_id.as_deref(), Some("c1"));
    }

    #[tokio::test]
    async fn multiple_tool_calls_in_one_turn_all_execute() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                tool_calls_response(vec![
                    call("c1", "add", 2.0, 3.0),
                    call("c2", "mul", 4.0, 5.0),
                ]),
                text_response("完成"),
            ])),
        });
        let engine = engine_with(vec![Arc::new(AddTool), Arc::new(MulTool)], provider);

        let mut history = Vec::new();
        let outcome = engine.run(&mut history, None, "一起算").await.unwrap();

        assert_eq!(outcome.tool_calls_made, 2);
        let results: Vec<&String> = outcome
            .events
            .iter()
            .filter_map(|e| match e {
                TurnEvent::ToolResult { name, output } if name == "add" => Some(output),
                _ => None,
            })
            .collect();
        assert_eq!(results, vec!["5"]);
        let mul_out = outcome
            .events
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolResult { name, output } if name == "mul" && output == "20"));
        assert!(mul_out, "mul(4,5)=20 must be fed back");
        assert_eq!(
            history.iter().filter(|m| m.role == ChatRole::Tool).count(),
            2
        );
    }

    #[tokio::test]
    async fn unknown_tool_soft_fails_and_loop_continues() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                tool_calls_response(vec![ToolCall {
                    id: "c1".into(),
                    name: "nope".into(),
                    arguments: "{}".into(),
                }]),
                text_response("继续"),
            ])),
        });
        let engine = engine_with(vec![Arc::new(AddTool)], provider);

        let mut history = Vec::new();
        let outcome = engine
            .run(&mut history, None, "用不存在的工具")
            .await
            .unwrap();

        assert_eq!(outcome.iterations, 2);
        assert_eq!(outcome.text, "继续");
        assert!(outcome.events.iter().any(|e| matches!(
            e,
            TurnEvent::ToolResult { name, output } if name == "nope" && output.contains("工具不存在")
        )));
    }

    #[tokio::test]
    async fn iteration_cap_stops_without_infinite_loop() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                tool_calls_response(vec![call("c1", "add", 1.0, 1.0)]),
                tool_calls_response(vec![call("c2", "add", 1.0, 1.0)]),
            ])),
        });
        let mut reg = ToolRegistry::new();
        reg.register(AddTool);
        let engine = TurnEngine::new(
            provider,
            "test-model",
            Arc::new(reg),
            TurnConfig {
                max_iterations: 2,
                ..Default::default()
            },
        );
        let mut history = Vec::new();
        let outcome = engine.run(&mut history, None, "别停").await.unwrap();
        assert_eq!(outcome.iterations, 2);
        assert_eq!(outcome.tool_calls_made, 2);
        assert!(!outcome.text.is_empty(), "cap path returns a fallback text");
    }

    #[tokio::test]
    async fn streamed_turn_emits_chunks_and_tool_events() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([
                tool_calls_response(vec![call("c1", "add", 1.0, 2.0)]),
                text_response("结果是 3"),
            ])),
        });
        let engine = engine_with(vec![Arc::new(AddTool)], provider);

        let mut history = Vec::new();
        let mut seen: Vec<TurnEvent> = Vec::new();
        let outcome = engine
            .run_streamed(&mut history, Some("你是助手"), "1+2?", &mut |e| {
                seen.push(e)
            })
            .await
            .unwrap();

        assert_eq!(outcome.iterations, 2);
        assert!(outcome.text.contains('3'));
        let chunk = seen.iter().find_map(|e| match e {
            TurnEvent::Chunk { delta } => Some(delta.clone()),
            _ => None,
        });
        assert!(
            chunk.is_some_and(|d| d.contains('3')),
            "final text streamed as Chunk"
        );
        assert!(
            seen.iter()
                .any(|e| matches!(e, TurnEvent::ToolCall { name, .. } if name == "add"))
        );
        assert!(seen
            .iter()
            .any(|e| matches!(e, TurnEvent::ToolResult { name, output } if name == "add" && output == "3")));
        // Whole-batch Text events are not emitted in streamed mode.
        assert!(!seen.iter().any(|e| matches!(e, TurnEvent::Text { .. })));
    }

    #[tokio::test]
    async fn pre_cancelled_turn_returns_partial_without_calling_model() {
        let provider = Arc::new(ScriptedProvider {
            responses: Mutex::new(VecDeque::from([text_response("不应被调用")])),
        });
        let cancel = CancellationToken::new();
        cancel.cancel();
        let engine = engine_with(vec![Arc::new(AddTool)], provider).with_cancel(cancel);

        let mut history = Vec::new();
        let outcome = engine.run(&mut history, None, "hello").await.unwrap();
        assert!(outcome.cancelled, "turn marked cancelled");
        assert_eq!(outcome.iterations, 0, "no model call happened");
        // Only the user message was appended → service persists the partial (user row only).
        assert_eq!(
            history.iter().filter(|m| m.role == ChatRole::User).count(),
            1
        );
    }

    #[tokio::test]
    async fn memory_facts_are_injected_before_turn() {
        let memory: Arc<dyn Memory> = Arc::new(InMemoryMemory::new());
        memory
            .store("today", "今天的安排：先检查发布，再开周会（用户偏好中文）")
            .await
            .unwrap();

        let captured: Arc<Mutex<Vec<ChatMessage>>> = Arc::new(Mutex::new(Vec::new()));
        struct CaptureProvider {
            responses: Mutex<VecDeque<ChatResponse>>,
            captured: Arc<Mutex<Vec<ChatMessage>>>,
        }
        #[async_trait]
        impl ModelProvider for CaptureProvider {
            fn name(&self) -> &str {
                "capture"
            }
            async fn chat(
                &self,
                request: &ChatRequest<'_>,
                _model: &str,
            ) -> Result<ChatResponse, ProviderError> {
                *self.captured.lock().unwrap() = request.messages.to_vec();
                Ok(self
                    .responses
                    .lock()
                    .unwrap()
                    .pop_front()
                    .ok_or_else(|| ProviderError::Config("script exhausted".into()))?)
            }
        }

        let provider = Arc::new(CaptureProvider {
            responses: Mutex::new(VecDeque::from([text_response("好的")])),
            captured: captured.clone(),
        });
        let engine = engine_with(Vec::new(), provider).with_memory(memory);

        let mut history = Vec::new();
        let _ = engine
            .run(&mut history, None, "帮我看看今天的安排")
            .await
            .unwrap();

        let sent = captured.lock().unwrap();
        let last_user = sent
            .iter()
            .rfind(|m| m.role == ChatRole::User)
            .expect("a user message was sent");
        let content = last_user.content.as_deref().unwrap();
        assert!(
            content.contains("[Memory context]"),
            "block header injected"
        );
        assert!(content.contains("先检查发布"), "recalled fact injected");
        assert!(
            content.contains("帮我看看今天的安排"),
            "original prompt kept"
        );
    }
}
