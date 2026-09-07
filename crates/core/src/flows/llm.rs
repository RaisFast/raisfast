//! `llm` node executor (dev-docs/workflow/llm-node.md §4).
//!
//! Template rendering via `expr::resolve_text` (C3.1), provider call through
//! the shared `[ai]` runtime (or an injected test provider), structured output
//! via prompt-constrained JSON + one corrective regeneration, error mapping
//! 4xx→BadRequest so the engine's blind retry fails fast.

use std::sync::Arc;
use std::time::Instant;

use raisfast_agent::ChatMessage;
use raisfast_agent::provider::{ChatRequest, ModelProvider, ProviderError};
use serde_json::{Map, Value, json};

use crate::errors::app_error::{AppError, AppResult};

use super::engine::{ExecOutcome, Pool};
use super::expr;
use super::graph::GraphNode;
use super::nodes::LlmConfig;

/// LLM runtime an executor resolves per call: injected mock (tests) or the
/// process-wide shared runtime (production).
pub struct LlmRuntime {
    pub provider: Arc<dyn ModelProvider>,
    pub default_model: Option<String>,
    pub timeout_ms: u64,
}

impl LlmRuntime {
    /// Production runtime from the shared `[ai]` singleton.
    ///
    /// # Errors
    /// `BadRequest` when AI is disabled/unconfigured — an authoring-visible
    /// 400 that also short-circuits the engine retry loop.
    pub fn shared() -> AppResult<Self> {
        let shared = crate::agent::service::shared_llm().ok_or_else(|| {
            AppError::BadRequest("llm 节点需要 [ai] 配置（未启用或未配置）".into())
        })?;
        Ok(Self {
            provider: shared.provider.clone(),
            default_model: shared.default_model.clone(),
            timeout_ms: shared.timeout_ms,
        })
    }
}

/// Map provider errors for the engine: 4xx non-429 (bad key/model) and config
/// errors are authoring problems → `BadRequest` (fail fast, no retry burn);
/// 429/5xx/transport/parse are transient → `Internal` (retryable).
fn map_provider_error(e: ProviderError) -> AppError {
    match &e {
        ProviderError::Config(msg) => AppError::BadRequest(format!("llm provider config: {msg}")),
        ProviderError::Http { status, .. } if *status >= 400 && *status < 500 && *status != 429 => {
            AppError::BadRequest(format!("llm provider http {status}: {e}"))
        }
        _ => AppError::Internal(anyhow::anyhow!("llm provider: {e}")),
    }
}

fn render_prompt_text(text: &str, pool: &Pool) -> AppResult<String> {
    // Prompts are always text: a whole-string `{{#ref#}}` returning an object
    // is stringified instead of failing (C3.1 keeps typed values).
    match expr::resolve_text(text, pool)? {
        Value::String(s) => Ok(s),
        other => Ok(match &other {
            Value::String(s) => s.clone(),
            v => serde_json::to_string(v).unwrap_or_default(),
        }),
    }
}

fn to_chat_messages(cfg: &LlmConfig, pool: &Pool) -> AppResult<Vec<ChatMessage>> {
    let mut out = Vec::with_capacity(cfg.messages.len());
    for m in &cfg.messages {
        let text = render_prompt_text(&m.text, pool)?;
        let msg = match m.role.as_str() {
            "system" => ChatMessage::system(text),
            "assistant" => ChatMessage::assistant(Some(text), None),
            _ => ChatMessage::user(text),
        };
        out.push(msg);
    }
    Ok(out)
}

fn usage_json(u: Option<raisfast_agent::TokenUsage>) -> Value {
    let u = u.unwrap_or_default();
    let input = u.input_tokens.map_or(0_u64, |v| v);
    let output = u.output_tokens.map_or(0_u64, |v| v);
    json!({
        "prompt_tokens": input,
        "completion_tokens": output,
        "total_tokens": input + output,
    })
}

/// Parse model text as JSON: direct parse first, then strip a ```json fence
/// (llm-node.md §4 implementation memo).
fn parse_json_text(text: &str) -> Option<Value> {
    if let Ok(v) = serde_json::from_str::<Value>(text.trim()) {
        return Some(v);
    }
    let t = text.trim();
    let inner = t
        .strip_prefix("```json")
        .or_else(|| t.strip_prefix("```"))?;
    let inner = inner.trim_end().strip_suffix("```").unwrap_or(inner);
    serde_json::from_str::<Value>(inner.trim()).ok()
}

/// Execute the `llm` node against the variable pool.
///
/// # Errors
/// `BadRequest` on missing template refs, disabled `[ai]`, or a non-retryable
/// provider error; `Internal` on transient provider/timeout failures.
pub async fn run_llm(
    runtime: &LlmRuntime,
    node: &GraphNode,
    pool: &Pool,
) -> AppResult<ExecOutcome> {
    let cfg: LlmConfig = serde_json::from_value(node.data.config.clone())
        .map_err(|e| AppError::BadRequest(format!("llm config: {e}")))?;
    let model = cfg
        .model
        .clone()
        .or_else(|| runtime.default_model.clone().filter(|m| !m.is_empty()));
    let Some(model) = model else {
        return Err(AppError::BadRequest(
            "llm: 未指定 model 且 [ai].model 未配置".into(),
        ));
    };

    let messages = to_chat_messages(&cfg, pool)?;
    let timeout_ms = cfg
        .timeout_ms
        .filter(|t| *t > 0)
        .map_or(runtime.timeout_ms, |t| t as u64);
    let started = Instant::now();

    let mut call_messages = messages.clone();
    let mut final_text: Option<String> = None;
    let mut final_usage: Option<raisfast_agent::TokenUsage> = None;
    let mut structured: Option<Value> = None;

    // Up to 2 passes when json_schema is set: initial + one corrective
    // regeneration feeding the parse error back (llm-node.md W3).
    let passes = if cfg.json_schema.is_some() { 2 } else { 1 };
    for pass in 0..passes {
        let request = ChatRequest {
            messages: &call_messages,
            tools: None,
            temperature: cfg.temperature,
            max_tokens: cfg.max_tokens,
            stop: cfg.stop.clone(),
        };
        let response = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            runtime.provider.chat(&request, &model),
        )
        .await
        .map_err(|_| AppError::Internal(anyhow::anyhow!("llm 超时 {timeout_ms}ms")))?
        .map_err(map_provider_error)?;
        if let Some(u) = response.usage {
            final_usage = Some(u);
        }
        let Some(text) = response.text else {
            return Err(AppError::Internal(anyhow::anyhow!("llm 响应无文本内容")));
        };
        if let Some(schema) = &cfg.json_schema {
            let Some(parsed) = parse_json_text(&text) else {
                if pass + 1 < passes {
                    call_messages.push(ChatMessage::assistant(Some(text.clone()), None));
                    call_messages.push(ChatMessage::user(format!(
                        "你上一条回复不是合法 JSON（解析失败）。请严格只输出符合此 JSON Schema 的 JSON，不要任何解释或代码围栏：\n{schema}"
                    )));
                    continue;
                }
                return Err(AppError::Internal(anyhow::anyhow!(
                    "llm structured output 解析失败（含一次纠错重生成）"
                )));
            };
            if let Err(reason) = super::nodes::shallow_schema_check(&parsed, schema) {
                if pass + 1 < passes {
                    call_messages.push(ChatMessage::assistant(Some(text.clone()), None));
                    call_messages.push(ChatMessage::user(format!(
                        "你上一条回复不符合 JSON Schema（{reason}）。请严格只输出符合此 Schema 的 JSON，不要任何解释或代码围栏：\n{schema}"
                    )));
                    continue;
                }
                return Err(AppError::Internal(anyhow::anyhow!(
                    "llm structured output 校验失败: {reason}"
                )));
            }
            structured = Some(parsed);
        }
        final_text = Some(text);
        break;
    }

    let latency_ms = started.elapsed().as_millis() as i64;
    let mut out = Map::new();
    out.insert("text".into(), json!(final_text.unwrap_or_default()));
    if let Some(s) = structured {
        out.insert("structured".into(), s);
    }
    out.insert("usage".into(), usage_json(final_usage));
    out.insert("latency_ms".into(), json!(latency_ms));
    Ok(ExecOutcome {
        output: Value::Object(out),
        usage: Some(usage_json(final_usage)),
        latency_ms: Some(latency_ms),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flows::graph::NodeData;
    use async_trait::async_trait;
    use raisfast_agent::TokenUsage;
    use raisfast_agent::provider::ChatResponse;
    use std::sync::Mutex;

    /// Scripted mock: pops queued responses, records every ChatRequest.
    struct MockProvider {
        responses: Mutex<std::collections::VecDeque<Result<ChatResponse, ProviderError>>>,
        seen: Mutex<Vec<Seen>>,
    }

    #[derive(Debug, Clone, PartialEq)]
    struct Seen {
        model: String,
        temperature: Option<f64>,
        max_tokens: Option<i64>,
        stop: Option<Vec<String>>,
        n_messages: usize,
    }

    impl MockProvider {
        fn new(responses: Vec<Result<ChatResponse, ProviderError>>) -> Self {
            Self {
                responses: Mutex::new(responses.into()),
                seen: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl ModelProvider for MockProvider {
        fn name(&self) -> &str {
            "mock"
        }
        async fn chat(
            &self,
            request: &ChatRequest<'_>,
            model: &str,
        ) -> Result<ChatResponse, ProviderError> {
            self.seen.lock().unwrap().push(Seen {
                model: model.to_string(),
                temperature: request.temperature,
                max_tokens: request.max_tokens,
                stop: request.stop.clone(),
                n_messages: request.messages.len(),
            });
            self.responses
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(ChatResponse::text_only("ok")))
        }
    }

    fn runtime(provider: Arc<MockProvider>) -> LlmRuntime {
        LlmRuntime {
            provider,
            default_model: Some("default-model".into()),
            timeout_ms: 5000,
        }
    }

    fn llm_node(config: Value) -> GraphNode {
        GraphNode {
            id: "n1".into(),
            data: NodeData {
                kind: "llm".into(),
                version: 1,
                title: String::new(),
                desc: None,
                config,
                modifiers: Value::Null,
            },
        }
    }

    fn pool_with(pairs: &[(&str, &str, Value)]) -> Pool {
        let mut pool = Pool::new();
        for (ns, name, v) in pairs {
            pool.entry((*ns).to_string())
                .or_default()
                .insert((*name).to_string(), v.clone());
        }
        pool
    }

    fn usage_ok() -> TokenUsage {
        TokenUsage {
            input_tokens: Some(812),
            output_tokens: Some(150),
            cache_read: None,
            cache_write: None,
        }
    }

    fn resp_with_usage(text: &str) -> Result<ChatResponse, ProviderError> {
        Ok(ChatResponse {
            text: Some(text.into()),
            tool_calls: Vec::new(),
            usage: Some(usage_ok()),
        })
    }

    #[tokio::test]
    async fn happy_path_output_shape_and_usage() {
        let mock = Arc::new(MockProvider::new(vec![resp_with_usage("答案是42")]));
        let node = llm_node(json!({
            "model": "m1",
            "messages": [{"role": "user", "text": "Q: {{#start.q#}}"}]
        }));
        let pool = pool_with(&[("start", "q", json!("1+1"))]);
        let out = run_llm(&runtime(mock.clone()), &node, &pool).await.unwrap();
        assert_eq!(out.output["text"], "答案是42");
        assert_eq!(out.output["usage"]["total_tokens"], 962);
        assert_eq!(out.usage.as_ref().unwrap()["prompt_tokens"], 812);
        assert!(out.latency_ms.is_some());
        assert_eq!(mock.seen.lock().unwrap()[0].model, "m1");
    }

    #[tokio::test]
    async fn params_passthrough_and_default_model() {
        let mock = Arc::new(MockProvider::new(vec![resp_with_usage("ok")]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "hi"}],
            "temperature": 0.2, "max_tokens": 99, "stop": ["\n"]
        }));
        run_llm(&runtime(mock.clone()), &node, &Pool::new())
            .await
            .unwrap();
        let seen = &mock.seen.lock().unwrap()[0];
        assert_eq!(seen.model, "default-model");
        assert_eq!(seen.max_tokens, Some(99));
        assert_eq!(seen.stop.as_deref(), Some(&["\n".to_string()][..]));
    }

    #[tokio::test]
    async fn missing_template_ref_is_bad_request() {
        let mock = Arc::new(MockProvider::new(vec![resp_with_usage("x")]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "{{#start.nope#}}"}]
        }));
        let err = run_llm(&runtime(mock), &node, &Pool::new())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
    }

    #[tokio::test]
    async fn auth_error_maps_to_bad_request_fail_fast() {
        let mock = Arc::new(MockProvider::new(vec![Err(ProviderError::Http {
            status: 401,
            body: "bad key".into(),
        })]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "hi"}]
        }));
        let err = run_llm(&runtime(mock), &node, &Pool::new())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::BadRequest(_)), "{err}");
    }

    #[tokio::test]
    async fn transport_error_maps_to_internal_retryable() {
        let mock = Arc::new(MockProvider::new(vec![Err(ProviderError::Transport(
            "conn reset".into(),
        ))]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "hi"}]
        }));
        let err = run_llm(&runtime(mock), &node, &Pool::new())
            .await
            .unwrap_err();
        assert!(matches!(err, AppError::Internal(_)), "{err}");
    }

    #[tokio::test]
    async fn fenced_json_parsed_without_regen() {
        let mock = Arc::new(MockProvider::new(vec![resp_with_usage(
            "```json\n{\"score\": 9}\n```",
        )]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "质检"}],
            "json_schema": {"type":"object","properties":{"score":{"type":"number"}},"required":["score"]}
        }));
        let out = run_llm(&runtime(mock.clone()), &node, &Pool::new())
            .await
            .unwrap();
        assert_eq!(out.output["structured"]["score"], 9);
        assert_eq!(mock.seen.lock().unwrap().len(), 1, "一次成功不触发纠错");
    }

    #[tokio::test]
    async fn corrective_regen_recovers_from_garbage() {
        let mock = Arc::new(MockProvider::new(vec![
            resp_with_usage("抱歉，我无法输出 JSON"),
            resp_with_usage("{\"score\": 7}"),
        ]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "质检"}],
            "json_schema": {"type":"object","properties":{"score":{"type":"number"}},"required":["score"]}
        }));
        let out = run_llm(&runtime(mock.clone()), &node, &Pool::new())
            .await
            .unwrap();
        assert_eq!(out.output["structured"]["score"], 7);
        assert_eq!(mock.seen.lock().unwrap().len(), 2);
        assert_eq!(
            mock.seen.lock().unwrap()[1].n_messages,
            3,
            "纠错轮 = 原始 + 模型回复 + 反馈指令"
        );
    }

    #[tokio::test]
    async fn schema_violation_twice_fails_node() {
        let mock = Arc::new(MockProvider::new(vec![
            resp_with_usage("{\"nope\": 1}"),
            resp_with_usage("{\"still_nope\": 2}"),
        ]));
        let node = llm_node(json!({
            "messages": [{"role": "user", "text": "质检"}],
            "json_schema": {"type":"object","properties":{"score":{"type":"number"}},"required":["score"]}
        }));
        let err = run_llm(&runtime(mock), &node, &Pool::new())
            .await
            .unwrap_err();
        assert!(err.to_string().contains("校验失败"), "{err}");
    }

    #[test]
    fn parse_json_text_variants() {
        assert_eq!(parse_json_text(" {\"a\":1} "), Some(json!({"a":1})));
        assert_eq!(
            parse_json_text("```json\n{\"a\":1}\n```"),
            Some(json!({"a":1}))
        );
        assert_eq!(parse_json_text("plain junk"), None);
    }

    #[test]
    fn shallow_schema_check_types_and_required() {
        let schema = json!({"type":"object","properties":{"score":{"type":"number"},"tag":{"type":"string"}},"required":["score"]});
        assert!(crate::flows::nodes::shallow_schema_check(&json!({"score": 1}), &schema).is_ok());
        assert!(crate::flows::nodes::shallow_schema_check(&json!({}), &schema).is_err());
        assert!(crate::flows::nodes::shallow_schema_check(&json!({"score":"x"}), &schema).is_err());
    }
}
