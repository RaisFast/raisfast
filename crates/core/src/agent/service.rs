//! AgentService: glue between `raisfast-agent` (engine) and the `ai_*` tables.
//!
//! One turn = two-phase persistence (loop-engine §2 落库时序契约):
//!   running 置位 → 先落 user 行 → 引擎跑回合 → 落 assistant/tool 行 +
//!   `turn:meta` → 幂等推进 `last_seq` → 回 `open`。
//! Provider/base-url/key resolution is MVP env-driven (`RAISFAST_AI_*`) until the
//! `[ai]` config section lands.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use raisfast_agent::provider::openai::OpenAiCompatProvider;
use raisfast_agent::{
    CancellationToken, ChatMessage, ChatRole, ModelProvider, TokenUsage, ToolCall, ToolRegistry,
    TurnConfig, TurnEngine, TurnError, TurnEvent, register_memory_tools,
};
use serde_json::json;

use crate::agent::memory_sql::ScopedMemory;
use crate::agent::models::ai_agent::AiAgent;
use crate::agent::models::ai_message::AiMessage;
use crate::agent::models::ai_session::AiSession;
use crate::agent::models::{ai_message, ai_session};
use crate::config::app::AiConfig;
use crate::errors::app_error::{AppError, AppResult};
use crate::types::snowflake_id::SnowflakeId;

/// In-process registry of sessions with a running turn. A session found
/// `running` in the DB but NOT here was left behind by a previous crash/panic
/// → AgentService recovers it to `open` automatically (single-process BaaS).
static ACTIVE_TURNS: OnceLock<Mutex<HashSet<i64>>> = OnceLock::new();

fn active_turns() -> &'static Mutex<HashSet<i64>> {
    ACTIVE_TURNS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// Returns true when this process may start a turn on `session_id`.
fn claim_turn(session_id: i64) -> bool {
    active_turns().lock().unwrap().insert(session_id)
}

fn release_turn(session_id: i64) {
    active_turns().lock().unwrap().remove(&session_id);
}

/// Tool allowlist semantics: memory tools are always available; domain tools
/// only when named in `ai_agents.tools` (or `"*"`).
fn apply_tool_allowlist(tools: &mut ToolRegistry, agent: &AiAgent) {
    let memory = ["memory_store", "memory_recall", "memory_forget"];
    let allow: Vec<String> = agent
        .tools
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let all = allow.iter().any(|n| n == "*");
    tools.retain(|name| memory.contains(&name) || all || allow.iter().any(|a| a == name));
}

/// Outcome of a service-level turn.
#[derive(Debug)]
pub struct AgentTurnResult {
    pub text: String,
    pub iterations: usize,
    pub tool_calls_made: usize,
    pub usage: Option<TokenUsage>,
    pub messages_appended: usize,
}

/// Build the platform LLM provider from the `[ai]` config section — shared by
/// agent turns and the flows `llm` node (OpenAI-compatible endpoint).
pub fn provider_from_config(ai: &AiConfig) -> AppResult<Arc<dyn ModelProvider>> {
    let base = ai
        .base_url
        .clone()
        .unwrap_or_else(|| "https://api.openai.com/v1".to_string());
    Ok(Arc::new(OpenAiCompatProvider::new(
        base,
        ai.api_key.clone(),
    )))
}

/// Process-wide shared LLM runtime for the flows `llm` node — set once at
/// startup (mirrors `integration::set_shared`), read by executors constructed
/// before/outside AppState. `None` when `[ai]` is disabled/unconfigured.
pub struct SharedLlm {
    pub provider: Arc<dyn ModelProvider>,
    /// `[ai].model` — default when the node config omits one.
    pub default_model: Option<String>,
    /// `[ai].timeout_secs` in milliseconds (node `timeout_ms` overrides).
    pub timeout_ms: u64,
}

static SHARED_LLM: std::sync::OnceLock<Option<Arc<SharedLlm>>> = std::sync::OnceLock::new();

/// Install the shared LLM runtime (called once from `build_app_state`).
pub fn set_shared_llm(runtime: Option<Arc<SharedLlm>>) {
    let _ = SHARED_LLM.set(runtime);
}

/// Access the shared LLM runtime, if initialized.
#[must_use]
pub fn shared_llm() -> Option<Arc<SharedLlm>> {
    SHARED_LLM.get().cloned().flatten()
}

/// Create the model provider for an agent from the `[ai]` config section.
fn provider_for(agent: &AiAgent, ai: &AiConfig) -> AppResult<Arc<dyn ModelProvider>> {
    if agent.provider == "ollama" && ai.base_url.is_none() {
        return Ok(Arc::new(OpenAiCompatProvider::new(
            "http://localhost:11434/v1".to_string(),
            ai.api_key.clone(),
        )));
    }
    provider_from_config(ai)
}

/// Model context window (tokens), zeroclaw config semantics: per-model map
/// (`RAISFAST_AI_MODEL_CONTEXT_JSON`) wins; otherwise the operator fallback
/// (`RAISFAST_AI_CONTEXT_WINDOW_FALLBACK`). `None` = windowing disabled.
fn model_context_window(agent: &AiAgent, ai: &AiConfig) -> Option<i64> {
    let window = ai
        .context_window_map
        .as_ref()
        .and_then(|m| m.get(&agent.model))
        .and_then(serde_json::Value::as_i64)
        .or(Some(ai.context_window_fallback))
        .unwrap_or(0);
    (window > 0).then_some(window)
}

/// Rough tokens consumed by the tool schemas sent to the model.
fn tool_overhead_tokens(tools: &ToolRegistry) -> i64 {
    let mut chars = 0usize;
    for spec in tools.specs() {
        chars += spec.name.len() + spec.description.len();
        chars += serde_json::to_string(&spec.parameters).map_or(0, |s| s.len());
    }
    (chars / 4) as i64
}

fn turn_error(e: TurnError) -> AppError {
    AppError::Internal(anyhow::anyhow!(e.to_string()))
}

/// Heuristic context-overflow detection (provider error wording varies).
fn is_context_overflow(e: &AppError) -> bool {
    let s = e.to_string().to_ascii_lowercase();
    [
        "maximum context",
        "context length",
        "context_length_exceeded",
        "reduce the length",
        "token limit",
        "too many tokens",
        "maximum tokens",
    ]
    .iter()
    .any(|k| s.contains(k))
}

/// Rough tokens of one chat message (chars/4; tool payload serialized).
fn estimate_message_tokens(m: &ChatMessage) -> usize {
    let mut chars = m.content.as_deref().map_or(0, |s| s.chars().count());
    chars += m.tool_call_id.as_deref().map_or(0, |s| s.len() / 4);
    if let Some(calls) = &m.tool_calls {
        chars += serde_json::to_string(calls).map_or(0, |s| s.len() / 4);
    }
    chars.div_ceil(4) + 1
}

/// Emergency overflow recovery (zeroclaw loop semantics): drop oldest messages
/// until the estimate fits `target` tokens, then prepend a breadcrumb. Never
/// empties the history and never leaves a dangling tool/assistant prefix.
fn trim_history_to_budget(history: &mut Vec<ChatMessage>, target: usize) -> bool {
    if history.len() <= 1 {
        return false;
    }
    let mut est: usize = history.iter().map(estimate_message_tokens).sum();
    if est <= target {
        return false;
    }
    let original = history.len();
    while history.len() > 1 && est > target {
        history.remove(0);
        est = history.iter().map(estimate_message_tokens).sum();
    }
    // Cut any orphaned tool/assistant prefix down to the next user/system turn.
    while !history.is_empty() && !matches!(history[0].role, ChatRole::User | ChatRole::System) {
        history.remove(0);
        est = history.iter().map(estimate_message_tokens).sum();
    }
    if history.len() >= original {
        return false;
    }
    if let Some(first) = history.first()
        && first
            .content
            .as_deref()
            .is_some_and(|c| c.contains("自动摘要") || c.contains("自动裁剪"))
    {
        return true;
    }
    history.insert(
        0,
        ChatMessage {
            role: ChatRole::User,
            content: Some(
                "（为适应上下文上限，较早对话已被自动裁剪；需要时可先用 memory_recall 检索已记忆内容）"
                    .to_string(),
            ),
            tool_calls: None,
            tool_call_id: None,
        },
    );
    let _ = est;
    true
}

/// Map a stored row back to the flat engine message (meta/system skipped).
fn row_to_chat_message(row: &AiMessage) -> Option<ChatMessage> {
    let role = match row.role.as_str() {
        "user" => ChatRole::User,
        "assistant" => ChatRole::Assistant,
        "tool" => ChatRole::Tool,
        _ => return None,
    };
    let tool_calls = row.tool_calls.as_ref().and_then(|v| {
        serde_json::from_value(v.clone())
            .ok()
            .filter(|calls: &Vec<ToolCall>| !calls.is_empty())
    });
    Some(ChatMessage {
        role,
        content: (!row.content.is_empty()).then_some(row.content.clone()),
        tool_calls,
        tool_call_id: row.tool_call_id.clone(),
    })
}

fn base_message_in(
    session_id: SnowflakeId,
    seq: i64,
    role: &str,
    kind: &str,
    content: &str,
) -> ai_message::AiMessageIn {
    ai_message::AiMessageIn {
        session_id,
        seq,
        role: role.to_string(),
        kind: kind.to_string(),
        content: content.to_string(),
        tool_calls: None,
        tool_call_id: None,
        tool_name: None,
        tool_success: None,
        tool_error: None,
        tool_elapsed_ms: None,
        tool_truncated: None,
        reasoning_content: None,
        usage: None,
    }
}

/// Persist the new messages the engine appended to `history`, one row each.
#[allow(clippy::too_many_arguments)]
async fn persist_delta(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    session_id: SnowflakeId,
    history: &[ChatMessage],
    appended_start: usize,
    seq: &mut i64,
    per_call_usage: &[TokenUsage],
    messages_appended: &mut usize,
) -> AppResult<()> {
    let mut usage_idx = 0usize;
    let mut last_tool_calls: Option<&Vec<ToolCall>> = None;

    for message in &history[appended_start..] {
        // The user message was persisted up-front by the caller.
        if message.role == ChatRole::User {
            continue;
        }
        let role_str = message.role.as_wire();
        let kind = match (&message.role, message.tool_calls.as_ref()) {
            (ChatRole::Assistant, Some(calls)) if !calls.is_empty() => "assistant_tool_calls",
            (ChatRole::Tool, _) => "tool_result",
            _ => "chat",
        };

        let mut row = base_message_in(
            session_id,
            *seq,
            role_str,
            kind,
            message.content.as_deref().unwrap_or(""),
        );
        if message.role == ChatRole::Assistant {
            if let Some(calls) = &message.tool_calls
                && !calls.is_empty()
            {
                row.tool_calls = Some(serde_json::to_value(calls).unwrap_or_default());
            }
            // Per-iteration usage, aligned with assistant rows in order.
            if let Some(u) = per_call_usage.get(usage_idx) {
                row.usage = Some(json!({
                    "input": u.input_tokens,
                    "output": u.output_tokens,
                    "cache_read": u.cache_read,
                    "cache_write": u.cache_write,
                }));
            }
            usage_idx += 1;
            last_tool_calls = message.tool_calls.as_ref();
        } else if message.role == ChatRole::Tool {
            row.tool_call_id = message.tool_call_id.clone();
            row.tool_name = last_tool_calls
                .and_then(|calls| {
                    message
                        .tool_call_id
                        .as_ref()
                        .and_then(|id| calls.iter().find(|c| &c.id == id))
                })
                .map(|c| c.name.clone());
            let output = message.content.as_deref().unwrap_or("");
            row.tool_success =
                Some(!output.starts_with("工具执行失败") && !output.starts_with("工具不存在"));
        }

        ai_message::append_message(pool, tenant_id, &row).await?;
        *seq += 1;
        *messages_appended += 1;
    }
    Ok(())
}

/// Run one turn for an existing session and persist its transcript.
pub async fn run_turn(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    session_id: SnowflakeId,
    user: &str,
) -> AppResult<AgentTurnResult> {
    let tenant_id = agent.tenant_id.as_deref();
    let session = ai_session::find_session_by_id(pool, session_id, tenant_id).await?;
    if !claim_turn(session.id.0) {
        return Err(AppError::Conflict("session_busy".into()));
    }
    // Recover a session stuck `running` from a previous crash (not in this process).
    if session.status == "running" {
        tracing::warn!(session = session.id.0, "recovering stale running session");
        ai_session::set_session_status(pool, session_id, tenant_id, "open").await?;
    }

    ai_session::set_session_status(pool, session_id, tenant_id, "running").await?;
    let executed = run_turn_inner(
        pool, ai, agent, session_id, tenant_id, user, None, None, None,
    )
    .await;
    // Always release the busy flag before propagating errors.
    let result = match executed {
        Ok(r) => r,
        Err(e) => {
            let _ = ai_session::set_session_status(pool, session_id, tenant_id, "open").await;
            release_turn(session.id.0);
            return Err(e);
        }
    };
    ai_session::set_session_status(pool, session_id, tenant_id, "open").await?;
    release_turn(session.id.0);
    Ok(result)
}

/// Streamed variant of [`run_turn`]: live `TurnEvent`s are pushed to
/// `on_event` (SSE sink) while the turn runs and persists. `extra_tools` are
/// domain tools bound to this session's actor (see `agent::tools`).
#[allow(clippy::too_many_arguments)]
pub async fn run_turn_streamed(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    session_id: SnowflakeId,
    user: &str,
    extra_tools: ToolRegistry,
    cancel: Option<CancellationToken>,
    on_event: &mut (dyn FnMut(TurnEvent) + Send),
) -> AppResult<AgentTurnResult> {
    let tenant_id = agent.tenant_id.as_deref();
    let session = ai_session::find_session_by_id(pool, session_id, tenant_id).await?;
    if !claim_turn(session.id.0) {
        return Err(AppError::Conflict("session_busy".into()));
    }
    if session.status == "running" {
        tracing::warn!(session = session.id.0, "recovering stale running session");
        ai_session::set_session_status(pool, session_id, tenant_id, "open").await?;
    }

    ai_session::set_session_status(pool, session_id, tenant_id, "running").await?;
    tracing::info!(
        session = session_id.0,
        agent = agent.id.0,
        "agent_service: streamed turn start"
    );
    let executed = run_turn_inner(
        pool,
        ai,
        agent,
        session_id,
        tenant_id,
        user,
        Some(on_event),
        Some(extra_tools),
        cancel,
    )
    .await;
    tracing::debug!(
        session = session_id.0,
        err = executed.is_err(),
        "agent_service: streamed turn end"
    );
    let result = match executed {
        Ok(r) => r,
        Err(e) => {
            let _ = ai_session::set_session_status(pool, session_id, tenant_id, "open").await;
            release_turn(session.id.0);
            return Err(e);
        }
    };
    ai_session::set_session_status(pool, session_id, tenant_id, "open").await?;
    release_turn(session.id.0);
    Ok(result)
}

#[allow(clippy::too_many_arguments)]
async fn run_turn_inner(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    session_id: SnowflakeId,
    tenant_id: Option<&str>,
    user: &str,
    mut emitter: Option<&mut (dyn FnMut(TurnEvent) + Send)>,
    extra_tools: Option<ToolRegistry>,
    cancel: Option<CancellationToken>,
) -> AppResult<AgentTurnResult> {
    // Load existing transcript (meta/system rows skipped) for continuity.
    let existing =
        ai_message::list_messages_after(pool, session_id, tenant_id, None, 10_000).await?;
    let mut history: Vec<ChatMessage> = existing.iter().filter_map(row_to_chat_message).collect();

    // Session owner = the memory user: all long-term memory is scoped
    // (tenant, agent, user) so different users never share facts.
    let session_owner =
        crate::agent::models::ai_session::find_session_by_id(pool, session_id, tenant_id).await?;
    let memory_user: Option<SnowflakeId> = Some(session_owner.user_id);

    // Memory-tier hygiene (zeroclaw budget.rs semantics): keep core/daily rows
    // within configured caps. Best effort — eviction failures never fail a turn.
    memory_hygiene(pool, ai, agent.id, tenant_id, memory_user).await;
    // We always send an assembled framework system prompt (agent/system_prompt
    // is embedded inside it), so the engine always inserts one leading System.
    let had_system = true;

    // Two-phase (1): durable user row before any model call.
    let mut seq = ai_message::next_seq(pool, session_id, tenant_id).await?;
    ai_message::append_message(
        pool,
        tenant_id,
        &base_message_in(session_id, seq, "user", "chat", user),
    )
    .await?;
    seq += 1;

    // Build the engine with a scoped memory handle + its tools.
    let memory = ScopedMemory::new(pool.clone(), agent.tenant_id.clone(), agent.id, memory_user);
    let mut tools = extra_tools.unwrap_or_default();
    register_memory_tools(&mut tools, memory.clone());
    apply_tool_allowlist(&mut tools, agent);

    // M5-A skills: resolve config + skills before building tool_names.
    let skills_root = crate::agent::skills::skills_root();
    let skill_enabled = crate::agent::skills::enabled_bundles(agent);
    let skills_full = !agent
        .params
        .as_ref()
        .and_then(|p| p.get("skills_mode"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|m| m == "compact");
    let loaded_skills =
        crate::agent::skills::load_skills(&skills_root, agent.tenant_id.as_deref(), &skill_enabled);
    // `read_skill` is only meaningful in Compact mode (Full already inlines).
    if !skill_enabled.is_empty() && !skills_full {
        tools.register(crate::agent::tools::skills::ReadSkillTool::new(
            skills_root.clone(),
            agent.tenant_id.clone(),
            skill_enabled.clone(),
        ));
    }
    // Composed `skill__<tool>` wrappers for declared, available platform tools
    // (§12-B): registered after the allowlist so availability is accurate.
    crate::agent::tools::skills::register_skill_composed(&mut tools, &loaded_skills);
    let tool_names = tools.names();

    // Load enabled skills and render the system section.
    let skills_section = crate::agent::skills::render_skills(&loaded_skills, skills_full);
    let assembled =
        crate::agent::prompt::assemble_with_skills(agent, &tool_names, skills_section.as_deref());

    // Mini Epoch (opencode context-epoch): when context windowing is on,
    // fingerprint the inputs behind the system text and persist a stable
    // baseline; rebuild (and record why) only when a fingerprint changes.
    let mut epoch_event: Option<(bool, &'static str)> = None;
    if model_context_window(agent, ai).is_some() {
        let cur = epoch_snapshot_for(agent, &tool_names, &loaded_skills);
        let session =
            crate::agent::models::ai_session::find_session_by_id(pool, session_id, tenant_id)
                .await?;
        let stored: Option<EpochState> = session
            .meta
            .as_ref()
            .and_then(|m| m.get("epoch"))
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        let (reused, reason) = match &stored {
            Some(e) if e.snapshot == cur => (true, "reuse"),
            Some(e) => (false, epoch_reason(&e.snapshot, &cur)),
            None => (false, "first"),
        };
        if !reused {
            let state = EpochState {
                baseline: assembled.text.clone(),
                snapshot: cur,
                baseline_seq: existing.last().map_or(0, |r| r.seq),
            };
            let mut meta = session
                .meta
                .clone()
                .unwrap_or_else(|| serde_json::json!({}));
            if let Some(o) = meta.as_object_mut() {
                o.insert(
                    "epoch".to_string(),
                    serde_json::to_value(&state).unwrap_or(serde_json::Value::Null),
                );
            }
            crate::agent::models::ai_session::update_session_meta(
                pool, session_id, tenant_id, meta,
            )
            .await?;
            tracing::info!(session = session_id.0, reason, "context epoch rebuilt");
        }
        epoch_event = Some((reused, reason));
    }

    // Context-window fold decision (opencode compaction semantics): usable =
    // window − reserve; folding triggers when estimated history exceeds
    // `usable − (system+tools+user)`; the retained tail budget is opencode's
    // `preserve_recent = clamp(2k, 15k, usable*0.25)`.
    let ctx_params = model_context_window(agent, ai).map(|window| {
        let reserve = if ai.context_output_reserve > 0 {
            ai.context_output_reserve.min(window * 9 / 10)
        } else {
            let auto = (window / 10).max(20_000);
            auto.min(window * 9 / 10)
        };
        let usable = window - reserve;
        let overhead = (assembled.system_chars as i64) / 4
            + tool_overhead_tokens(&tools)
            + (user.chars().count() as i64) / 4;
        let trigger = usable - overhead;
        let tail = (usable * 25 / 100).clamp(2_000, 15_000);
        (trigger, tail)
    });
    let prev_ctx = latest_ctx_row(&existing);
    let prior_cover = prev_ctx.as_ref().map_or(0, |p| p.cover_seq);
    let (cover_seq, ctx_summary, folded_now) = match ctx_params {
        Some((trigger, tail)) if trigger > 0 => {
            ensure_ctx_window(ai, agent, &existing, prev_ctx, trigger, tail, false).await?
        }
        _ => (0, None, false),
    };
    // Persist the fold as an observable transcript row (opencode compaction
    // shape: the canonical `context:summary` message), then advance seq.
    if folded_now && let Some(text) = ctx_summary.as_deref() {
        let state = crate::agent::context::CtxState {
            cover_seq,
            text: text.to_string(),
        };
        let content = serde_json::to_string(&state).unwrap_or_else(|_| text.to_string());
        ai_message::append_message(
            pool,
            tenant_id,
            &base_message_in(session_id, seq, "meta", "context:summary", &content),
        )
        .await?;
        seq += 1;
    }
    // Memory consolidation (zeroclaw classify/consolidation): the turns just
    // folded leave the transcript window — extract durable facts into Core
    // memory so they stay recallable. Optional (RAISFAST_AI_MEMORY_CONSOLIDATE).
    if folded_now
        && ai.memory_consolidate
        && let Err(e) = consolidate_folded_range(
            pool,
            ai,
            agent,
            tenant_id,
            memory_user,
            &existing,
            prior_cover,
            cover_seq,
        )
        .await
    {
        tracing::warn!(session = session_id.0, error = %e, "memory consolidation failed");
    } else if folded_now && !ai.memory_consolidate {
        tracing::warn!(
            session = session_id.0,
            "turn folded but memory consolidation disabled; set RAISFAST_AI_MEMORY_CONSOLIDATE=true"
        );
    }
    if cover_seq > 0 {
        history = existing
            .iter()
            .filter(|r| r.seq > cover_seq)
            .filter_map(row_to_chat_message)
            .collect();
        if let Some(text) = ctx_summary {
            history.insert(
                0,
                ChatMessage {
                    role: ChatRole::User,
                    content: Some(format!(
                        "（以下是较早对话的自动摘要；需要找回摘要前的细节时用 memory_recall 或明确提问）\n{text}"
                    )),
                    tool_calls: None,
                    tool_call_id: None,
                },
            );
        }
    }
    let mut old_len = history.len();

    let provider = provider_for(agent, ai)?;
    let engine = TurnEngine::new(
        provider,
        agent.model.clone(),
        Arc::new(tools),
        TurnConfig {
            max_iterations: agent.max_iterations.clamp(1, 50) as usize,
            temperature: agent.temperature,
        },
    )
    .with_memory(memory);
    let mut engine = engine;
    if let Some(c) = cancel {
        engine = engine.with_cancel(c);
    }

    // Context overflow fallback (zeroclaw loop recovery): if the provider
    // still rejects on context length (estimation drift), drop oldest whole
    // messages and retry once — no summarization in this path.
    let mut overflow_trimmed = false;
    let outcome = loop {
        let result = match emitter.as_mut() {
            Some(cb) => engine
                .run_streamed(&mut history, Some(&assembled.text), user, cb)
                .await
                .map_err(turn_error),
            None => engine
                .run(&mut history, Some(&assembled.text), user)
                .await
                .map_err(turn_error),
        };
        match result {
            Ok(outcome) => break outcome,
            Err(e) => {
                let Some(window) = model_context_window(agent, ai) else {
                    return Err(e);
                };
                if overflow_trimmed || !is_context_overflow(&e) {
                    return Err(e);
                }
                let target = (window as usize) * 8 / 10;
                if !trim_history_to_budget(&mut history, target) {
                    return Err(e);
                }
                old_len = history.len();
                overflow_trimmed = true;
                tracing::warn!(
                    session = session_id.0,
                    "context overflow: dropped old turns and retrying"
                );
            }
        }
    };

    // Two-phase (2): persist assistant/tool rows appended by the engine.
    let appended_start = old_len + usize::from(had_system);
    let mut messages_appended = 0usize;
    persist_delta(
        pool,
        tenant_id,
        session_id,
        &history,
        appended_start,
        &mut seq,
        &outcome.per_call_usage,
        &mut messages_appended,
    )
    .await?;

    // turn:meta terminal row, then the idempotent cursor advance.
    let stop_reason = if outcome.cancelled {
        "cancelled"
    } else {
        "completed"
    };
    let meta = json!({
        "stop_reason": stop_reason,
        "system_hash": assembled.hash,
        "prompt_version": assembled.version,
        "prompt": {
            "system_chars": assembled.system_chars,
            "skills_chars": assembled.skills_chars,
        },
        "iterations": outcome.iterations,
        "tool_calls_made": outcome.tool_calls_made,
        "usage_total": outcome.usage.as_ref().map(|u| json!({
            "input": u.input_tokens,
            "output": u.output_tokens,
            "cache_read": u.cache_read,
            "cache_write": u.cache_write,
        })),
        "epoch": epoch_event.map(|(reused, reason)| json!({
            "reused": reused,
            "reason": reason,
        })),
    });
    ai_message::append_message(
        pool,
        tenant_id,
        &base_message_in(session_id, seq, "meta", "turn:meta", &meta.to_string()),
    )
    .await?;
    ai_session::advance_last_seq(pool, session_id, tenant_id, seq).await?;
    touch_daily_log(pool, agent, tenant_id, memory_user).await;

    Ok(AgentTurnResult {
        text: outcome.text,
        iterations: outcome.iterations,
        tool_calls_made: outcome.tool_calls_made,
        usage: outcome.usage,
        messages_appended,
    })
}

// ── thin service wrappers for handlers ──────────────────────────────────────

/// Create an agent (admin).
#[allow(clippy::too_many_arguments)]
pub async fn create_agent(
    pool: &crate::db::Pool,
    tenant: Option<String>,
    owner: Option<SnowflakeId>,
    name: String,
    system_prompt: String,
    provider: String,
    model: String,
    temperature: Option<f64>,
    tools: Vec<String>,
    memory_enabled: bool,
    params: Option<serde_json::Value>,
) -> AppResult<AiAgent> {
    crate::agent::models::ai_agent::create_agent(
        pool,
        tenant.as_deref(),
        owner,
        &name,
        &system_prompt,
        &provider,
        &model,
        temperature,
        tools,
        memory_enabled,
        params,
    )
    .await
}

/// Find an agent by id (tenant-scoped).
pub async fn find_agent(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant: Option<&str>,
) -> AppResult<AiAgent> {
    crate::agent::models::ai_agent::find_agent_by_id(pool, id, tenant).await
}

/// List agents of a tenant (admin/selection).
pub async fn list_agents(pool: &crate::db::Pool, tenant: Option<&str>) -> AppResult<Vec<AiAgent>> {
    crate::agent::models::ai_agent::list_agents(pool, tenant).await
}

/// Partial update payload for an agent (admin). Fields present are applied.
#[derive(Debug, Clone, Default, serde::Deserialize)]
pub struct AgentPatch {
    pub system_prompt: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f64>,
    pub max_iterations: Option<i32>,
    pub tools: Option<Vec<String>>,
    pub memory_enabled: Option<bool>,
    pub params: Option<serde_json::Value>,
}

/// Apply a partial patch to an agent (overlay on current row) and return it.
pub async fn update_agent(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    id: SnowflakeId,
    patch: &AgentPatch,
) -> AppResult<AiAgent> {
    let current = crate::agent::models::ai_agent::find_agent_by_id(pool, id, tenant_id).await?;
    let tools = match &patch.tools {
        Some(t) => serde_json::to_value(t).unwrap_or(serde_json::Value::Array(vec![])),
        None => current.tools,
    };
    crate::agent::models::ai_agent::update_agent(
        pool,
        tenant_id,
        id,
        patch
            .system_prompt
            .as_deref()
            .unwrap_or(&current.system_prompt),
        patch.provider.as_deref().unwrap_or(&current.provider),
        patch.model.as_deref().unwrap_or(&current.model),
        patch.temperature.or(current.temperature),
        patch.max_iterations.unwrap_or(current.max_iterations),
        tools,
        patch.memory_enabled.unwrap_or(current.memory_enabled),
        patch.params.clone().or(current.params),
    )
    .await?;
    crate::agent::models::ai_agent::find_agent_by_id(pool, id, tenant_id).await
}

/// Create a session owned by `user_id` on an agent.
pub async fn create_session(
    pool: &crate::db::Pool,
    tenant: Option<String>,
    agent_id: SnowflakeId,
    user_id: SnowflakeId,
    title: &str,
) -> AppResult<AiSession> {
    crate::agent::models::ai_session::create_session(
        pool,
        tenant.as_deref(),
        agent_id,
        user_id,
        title,
    )
    .await
}

/// Find a session by id (tenant-scoped).
pub async fn find_session(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant: Option<&str>,
) -> AppResult<AiSession> {
    crate::agent::models::ai_session::find_session_by_id(pool, id, tenant).await
}

/// Sessions of one agent owned by `user_id`.
pub async fn list_my_sessions(
    pool: &crate::db::Pool,
    tenant: Option<&str>,
    agent_id: SnowflakeId,
    user_id: SnowflakeId,
) -> AppResult<Vec<AiSession>> {
    let all = crate::agent::models::ai_session::list_sessions(pool, tenant, agent_id).await?;
    Ok(all.into_iter().filter(|s| s.user_id == user_id).collect())
}

/// Replay slice of the session log.
pub async fn list_messages(
    pool: &crate::db::Pool,
    session_id: SnowflakeId,
    tenant: Option<&str>,
    since: Option<i64>,
    limit: i64,
) -> AppResult<Vec<AiMessage>> {
    crate::agent::models::ai_message::list_messages_after(pool, session_id, tenant, since, limit)
        .await
}

/// Best-effort memory-tier budget compaction (`core`/`daily`), run once per
/// turn. Failures are logged and never fail the turn (hygiene semantics).
async fn memory_hygiene(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent_id: SnowflakeId,
    tenant: Option<&str>,
    user: Option<SnowflakeId>,
) {
    use crate::agent::models::ai_memory::MemoryBudgetConfig;
    let budget = MemoryBudgetConfig {
        core_max_rows: ai.memory_core_max_rows,
        core_max_bytes: ai.memory_core_max_bytes,
        daily_max_rows: ai.memory_daily_max_rows,
    };
    if budget.core_max_rows <= 0 && budget.core_max_bytes <= 0 && budget.daily_max_rows <= 0 {
        return;
    }
    for category in ["core", "daily"] {
        match crate::agent::models::ai_memory::compact_category_to_budget(
            pool, tenant, agent_id, user, category, budget,
        )
        .await
        {
            Ok(report) => {
                if report.evicted_by_count > 0 || report.evicted_by_bytes > 0 {
                    tracing::info!(
                        agent = agent_id.0,
                        category,
                        evicted_by_count = report.evicted_by_count,
                        evicted_by_bytes = report.evicted_by_bytes,
                        "memory budget compaction"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(agent = agent_id.0, category, error = %e, "memory budget compaction failed")
            }
        }
    }
}

/// Durable system-baseline state (mini Epoch, opencode context-epoch shape).
/// `snapshot` fingerprints every input that produces the system text; when it
/// changes we rebuild `baseline` and record a coarse rebuild reason.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
struct EpochSnapshot {
    template: u32,
    agent: String,
    tools: String,
    skills: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct EpochState {
    baseline: String,
    snapshot: EpochSnapshot,
    baseline_seq: i64,
}

fn sha256_hex(s: &str) -> String {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(s.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(64);
    for b in digest {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

fn normalize_memory_text(s: &str) -> String {
    let mut out = s.to_ascii_lowercase();
    if let Ok(re) = regex::Regex::new(r"alpha[-_ ]?[0-9]+") {
        out = re.replace_all(&out, "").into_owned();
    }
    // Identity normalization for dedupe only: keep letters/digits, drop
    // whitespace + punctuation so identical facts differing only in spacing or
    // 标点 compare equal. Content stored is never rewritten.
    if let Ok(re) = regex::Regex::new(r"[^\p{L}\p{N}]+") {
        out = re.replace_all(&out, "").into_owned();
    }
    out
}

/// Near-duplicate check across already-stored Core rows: identical normalized
/// text, or one strongly contained in the other (LLM wording variants of the
/// same fact) — prevents repeated fold passes from stacking duplicates.
fn memory_text_duplicate(existing: &[String], norm: &str) -> bool {
    if norm.is_empty() {
        return true;
    }
    existing
        .iter()
        .any(|e| e == norm || (e.len() > 20 && (e.contains(norm) || norm.contains(e))))
}

fn epoch_snapshot_for(
    agent: &AiAgent,
    tool_names: &[String],
    loaded_skills: &[crate::agent::skills::LoadedSkill],
) -> EpochSnapshot {
    let mut tools: Vec<String> = tool_names.to_vec();
    tools.sort();
    let skills_fp: String = loaded_skills
        .iter()
        .map(|sk| {
            format!(
                "{}:{}",
                sk.name,
                sha256_hex(&format!("{}||{}", sk.instructions, sk.tools.join(",")))
            )
        })
        .collect::<Vec<_>>()
        .join("|");
    EpochSnapshot {
        template: crate::agent::prompt::PromptRegistry.current_version(),
        agent: sha256_hex(&format!("{}||{}", agent.name, agent.system_prompt)),
        tools: sha256_hex(&tools.join(",")),
        skills: sha256_hex(&skills_fp),
    }
}

fn epoch_reason(stored: &EpochSnapshot, current: &EpochSnapshot) -> &'static str {
    if stored.agent != current.agent {
        "agent"
    } else if stored.tools != current.tools {
        "tools"
    } else if stored.skills != current.skills {
        "skills"
    } else if stored.template != current.template {
        "template"
    } else {
        "unknown"
    }
}

/// Consolidate a folded transcript slice into durable Core memory facts
/// (zeroclaw classify/consolidation). One extraction LLM call; failures degrade
/// to a warn and never fail the turn.
async fn consolidate_folded_memory(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    tenant: Option<&str>,
    user: Option<SnowflakeId>,
    slice_text: &str,
) -> AppResult<()> {
    let provider = provider_for(agent, ai)?;
    let messages = [ChatMessage {
        role: ChatRole::User,
        content: Some(format!(
            "从下面的对话中抽取值得长期记住的用户偏好/决策/规则/政策/事实（含关键数字）。\
             如果没有任何值得长期记住的内容，直接返回 []，不要编造，不要保存寒暄、一次性计算或临时任务（宁缺毋滥）。\
             不要抽取助手关于自身机制/工具使用的自述（如「我不使用主动存储」「系统自动归纳」）、对话过程性描述（谁说了什么、编号递进）。\
             多轮重复陈述的同一件事必须合并为一条（key 取同一标识），不要按轮次生成多条。\
             content 只写事实本身（一句可直接使用的规则/偏好），不要把\"规则 ALPHA\"、\"ALPHA-1至N为同一内容\"等编号/重复性说明或括号注释写进 content。\
             只输出 JSON 数组，每项为 {{\"key\": 简短英文驼峰标识, \"content\": 一句话事实, \"importance\": 0到1数字}}，\
             最多 8 项，importance 低于 0.6 的不要包含，不要输出其它文字。\n\n{slice_text}"
        )),
        tool_calls: None,
        tool_call_id: None,
    }];
    let request = raisfast_agent::provider::ChatRequest {
        messages: &messages,
        tools: None,
        temperature: Some(0.0),
        max_tokens: None,
        stop: None,
    };
    let response = provider
        .chat(&request, &agent.model)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("memory consolidate failed: {e}")))?;
    let Some(text) = response.text.filter(|t| !t.trim().is_empty()) else {
        return Ok(());
    };
    let trimmed = text
        .trim()
        .trim_start_matches("```json")
        .trim_end_matches("```")
        .trim();
    let Ok(items) = serde_json::from_str::<serde_json::Value>(trimmed) else {
        tracing::warn!("memory consolidate: could not parse LLM output");
        return Ok(());
    };
    let Some(list) = items.as_array() else {
        return Ok(());
    };
    tracing::info!(n = list.len(), "memory consolidate: LLM extracted items");

    let mut existing_norms: Vec<String> = if list.is_empty() {
        Vec::new()
    } else {
        crate::agent::models::ai_memory::recall_memories(pool, agent.id, user, tenant, None, 1_000)
            .await?
            .into_iter()
            .filter(|m| m.category == "core")
            .map(|m| normalize_memory_text(&m.content))
            .collect()
    };
    let mut seen_content = std::collections::HashSet::new();
    let mut stored = 0usize;
    let mut skipped_low = 0usize;
    let mut skipped_dup = 0usize;
    for item in list.iter().take(8) {
        let Some(content) = item.get("content").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let content = content.trim();
        if content.is_empty() {
            continue;
        }
        let importance = item
            .get("importance")
            .and_then(serde_json::Value::as_f64)
            .unwrap_or_else(|| crate::agent::memory_sql::importance_for("core", content))
            .clamp(0.0, 1.0);
        // Low-value / fabricated entries are discarded defensively: not every
        // folded conversation deserves durable memory.
        if importance < 0.6 {
            skipped_low += 1;
            continue;
        }
        let norm = normalize_memory_text(content);
        if !seen_content.insert(norm.clone()) || memory_text_duplicate(&existing_norms, &norm) {
            skipped_dup += 1;
            continue;
        }
        // Deterministic key from normalized content: same fact (even under
        // LLM wording/numbering drift) upserts instead of piling duplicates.
        let key = format!("fact_{}", &sha256_hex(&norm)[..12]);
        if let Err(e) = crate::agent::models::ai_memory::store_memory(
            pool, tenant, agent.id, user, &key, content, "core", importance, false,
        )
        .await
        {
            tracing::warn!(key = %key, error = %e, "memory consolidate store failed");
            continue;
        }
        existing_norms.push(norm);
        stored += 1;
    }
    tracing::info!(
        stored,
        skipped_low,
        skipped_dup,
        "memory consolidation finished"
    );
    Ok(())
}

/// Extract durable facts from rows whose `seq` falls in `(prior_cover,
/// new_cover]` — i.e. exactly the turns this fold took out of the window.
#[allow(clippy::too_many_arguments)]
async fn consolidate_folded_range(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    tenant: Option<&str>,
    user: Option<SnowflakeId>,
    existing: &[AiMessage],
    prior_cover: i64,
    new_cover: i64,
) -> AppResult<()> {
    let is_conv = |r: &AiMessage| matches!(r.role.as_str(), "user" | "assistant" | "tool");
    let rows: Vec<(String, String)> = existing
        .iter()
        .filter(|r| r.seq > prior_cover && r.seq <= new_cover && is_conv(r))
        .map(|r| (r.role.clone(), r.content.clone()))
        .collect();
    if !rows.is_empty() {
        let text = crate::agent::context::fold_text(&rows);
        consolidate_folded_memory(pool, ai, agent, tenant, user, &text).await?;
    }
    Ok(())
}

/// Latest durable fold state from the transcript (`context:summary` rows are
/// the single source; the newest row wins). Returns `None` when never folded.
fn latest_ctx_row(rows: &[AiMessage]) -> Option<crate::agent::context::CtxState> {
    rows.iter()
        .filter(|r| r.kind == "context:summary")
        .filter_map(|r| serde_json::from_str::<crate::agent::context::CtxState>(&r.content).ok())
        .next_back()
}

/// Manual compaction (`POST /ai/sessions/{id}/compact`): force a fold with the
/// model-window tail budget regardless of the trigger. Returns the new
/// `(cover_seq, summary)` or `None` when there was nothing to compact. Also
/// persists an observable `context:summary` transcript row when folded.
pub async fn compact_session(
    pool: &crate::db::Pool,
    ai: &AiConfig,
    agent: &AiAgent,
    session_id: SnowflakeId,
    tenant: Option<&str>,
) -> AppResult<Option<(i64, String)>> {
    let existing = ai_message::list_messages_after(pool, session_id, tenant, None, 10_000).await?;
    let tail = model_context_window(agent, ai)
        .map(|window| {
            let reserve = if ai.context_output_reserve > 0 {
                ai.context_output_reserve.min(window * 9 / 10)
            } else {
                let auto = (window / 10).max(20_000);
                auto.min(window * 9 / 10)
            };
            let usable = window - reserve;
            (usable * 25 / 100).clamp(2_000, 15_000)
        })
        .unwrap_or(8_000);
    let prev_ctx = latest_ctx_row(&existing);
    let prior_cover = prev_ctx.as_ref().map_or(0, |p| p.cover_seq);
    let session_owner =
        crate::agent::models::ai_session::find_session_by_id(pool, session_id, tenant).await?;
    let compact_user: Option<SnowflakeId> = Some(session_owner.user_id);
    let (cover, summary, folded_now) =
        ensure_ctx_window(ai, agent, &existing, prev_ctx, 0, tail, true).await?;
    if folded_now && let Some(text) = summary.as_deref() {
        let marker_seq = ai_message::next_seq(pool, session_id, tenant).await?;
        let state = crate::agent::context::CtxState {
            cover_seq: cover,
            text: text.to_string(),
        };
        let content = serde_json::to_string(&state).unwrap_or_else(|_| text.to_string());
        ai_message::append_message(
            pool,
            tenant,
            &base_message_in(session_id, marker_seq, "meta", "context:summary", &content),
        )
        .await?;
        // Manual compact also consolidates the freshly folded turns into Core
        // memory (same semantics as the automatic fold path).
        if ai.memory_consolidate
            && let Err(e) = consolidate_folded_range(
                pool,
                ai,
                agent,
                tenant,
                compact_user,
                &existing,
                prior_cover,
                cover,
            )
            .await
        {
            tracing::warn!(session = session_id.0, error = %e, "memory consolidation failed");
        } else if !ai.memory_consolidate {
            tracing::warn!(
                session = session_id.0,
                "compact folded but memory consolidation disabled; set RAISFAST_AI_MEMORY_CONSOLIDATE=true"
            );
        }
    }
    Ok((cover > 0).then(|| (cover, summary.unwrap_or_default())))
}

/// Summarize a folded transcript slice with one provider call (temperature 0).
/// Fails turn-friendly: caller degrades to no-folding on any error.
async fn summarize_transcript(ai: &AiConfig, agent: &AiAgent, combined: &str) -> AppResult<String> {
    let provider = provider_for(agent, ai)?;
    let messages = [ChatMessage {
        role: ChatRole::User,
        content: Some(format!(
            "把下面较早的对话（可能已含摘要）压缩为中文要点，保留：用户偏好与承诺、明确的决策/规则/策略、关键数字、值得长期记住的工具结果。若原文含编号/代号（如 ALPHA-1、事项N），必须逐条保留每个编号及其内容、不要合并或概括成同一句。不要遗漏可能影响后续回答的事实。输出 ≤12 行紧凑要点，不要开头客套。\n\n{combined}"
        )),
        tool_calls: None,
        tool_call_id: None,
    }];
    let request = raisfast_agent::provider::ChatRequest {
        messages: &messages,
        tools: None,
        temperature: Some(0.0),
        max_tokens: None,
        stop: None,
    };
    let response = provider
        .chat(&request, &agent.model)
        .await
        .map_err(|e| AppError::Internal(anyhow::anyhow!("context summarize failed: {e}")))?;
    response
        .text
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| AppError::Internal(anyhow::anyhow!("context summarize returned empty")))
}

/// Context-window decision (opencode compaction semantics adapted to our
/// append-only replay). `trigger_tokens`: fold when estimated history exceeds
/// usable-minus-overhead; `tail_tokens`: keep only the newest suffix that fits
/// `preserve_recent` (opencode `clamp(2k,15k, usable*0.25)`). Returns
/// `(cover_seq, summary_text, folded_now)`: `cover_seq > 0` means older rows
/// are folded, `summary_text` is the durable context block replayed first and
/// `folded_now` signals the caller to persist the `context:summary` row.
///
/// Reference: opencode `session/compaction.ts select()` + zeroclaw
/// consolidation; the canonical fold row (`context:summary`) is host-owned and
/// persisted by the caller (single source, opencode compaction shape).
#[allow(clippy::too_many_arguments)]
async fn ensure_ctx_window(
    ai: &AiConfig,
    agent: &AiAgent,
    existing: &[AiMessage],
    prev: Option<crate::agent::context::CtxState>,
    trigger_tokens: i64,
    tail_tokens: i64,
    force: bool,
) -> AppResult<(i64, Option<String>, bool)> {
    if !force && trigger_tokens <= 0 {
        return Ok((0, None, false));
    }
    use crate::agent::context::{RowMeta, fold_text, select_cover};

    let is_conv = |r: &AiMessage| matches!(r.role.as_str(), "user" | "assistant" | "tool");
    let ctx_chars = prev.as_ref().map_or(0, |p| p.text.len() + 64);

    let trigger_chars = (trigger_tokens.max(0) as usize) * 4;
    let tail_chars = (tail_tokens.max(1_000) as usize) * 4;
    let base: Vec<&AiMessage> = existing
        .iter()
        .filter(|r| r.seq > prev.as_ref().map_or(0, |p| p.cover_seq) && is_conv(r))
        .collect();
    let meta: Vec<RowMeta> = base
        .iter()
        .map(|r| RowMeta {
            seq: r.seq,
            is_user: r.role == "user",
            len: r
                .content
                .len()
                .saturating_add(r.tool_name.as_deref().map_or(0, |s| s.len() + 16))
                .saturating_add(r.tool_error.as_deref().map_or(0, |s| s.len() + 16)),
        })
        .collect();
    let history_chars: usize = meta.iter().map(|r| r.len).sum();

    // No fold needed unless history (+existing ctx) exceeds the trigger.
    if !force && history_chars + ctx_chars <= trigger_chars {
        return Ok((
            prev.as_ref().map_or(0, |p| p.cover_seq),
            prev.filter(|p| p.cover_seq > 0).map(|p| p.text),
            false,
        ));
    }

    // Keep only the newest suffix that fits `preserve_recent` (tail budget).
    let eff_tail = tail_chars.saturating_sub(ctx_chars);
    let cov = match select_cover(&meta, eff_tail) {
        Some(c) => c,
        None if force => {
            // Manual compact: fold everything older than the newest whole turn
            // even when the transcript would otherwise fit the tail budget.
            match meta.iter().rposition(|r| r.is_user) {
                Some(last_user) if last_user > 0 => last_user - 1,
                _ => {
                    return Ok((
                        prev.as_ref().map_or(0, |p| p.cover_seq),
                        prev.filter(|p| p.cover_seq > 0).map(|p| p.text),
                        false,
                    ));
                }
            }
        }
        None => {
            // Everything already fits the tail budget (or a single turn is bigger
            // than the budget — kept whole, engine needs whole turns for tool pairs).
            return Ok((
                prev.as_ref().map_or(0, |p| p.cover_seq),
                prev.filter(|p| p.cover_seq > 0).map(|p| p.text),
                false,
            ));
        }
    };

    // Fold base[0..=cov] (plus the previous summary if any) into one new summary.
    let slice: Vec<(String, String)> = base[..=cov]
        .iter()
        .map(|r| (r.role.clone(), r.content.clone()))
        .collect();
    let slice_text = fold_text(&slice);
    let combined = match &prev {
        Some(p) if !p.text.trim().is_empty() => format!("{}\n---\n{}", p.text, slice_text),
        _ => slice_text,
    };
    let summary = match summarize_transcript(ai, agent, &combined).await {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "context fold summarize failed; keeping full replay");
            return Ok((
                prev.as_ref().map_or(0, |p| p.cover_seq),
                prev.filter(|p| p.cover_seq > 0).map(|p| p.text),
                false,
            ));
        }
    };

    let new_cover = base[cov].seq;
    Ok((new_cover, Some(summary), true))
}

/// Host-managed daily session log (zeroclaw `Daily` tier): one row per agent
/// per day, auto-updated after each turn; never surfaced via model recall and
/// subject to the `daily_max_rows` budget. Not model-initiated.
async fn touch_daily_log(
    pool: &crate::db::Pool,
    agent: &AiAgent,
    tenant: Option<&str>,
    user: Option<SnowflakeId>,
) {
    let now = crate::utils::tz::now_utc();
    let date = now.format("%Y-%m-%d").to_string();
    let key = format!("daily_{date}");
    let content = format!("{date} 当日会话活跃记录（host 自动维护）");
    if let Err(e) = crate::agent::models::ai_memory::store_memory(
        pool, tenant, agent.id, user, &key, &content, "daily", 0.3, false,
    )
    .await
    {
        tracing::warn!(agent = agent.id.0, error = %e, "daily log update failed");
    }
}

/// Daily usage of one agent over the last `days` (default 30, clamped to 1-90).
/// Aggregated from `turn:meta` rows (`usage_total`/`tool_calls_made`), one row
/// per completed or cancelled turn — no schema/JSON-extraction per dialect.
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentUsageDay {
    pub date: String,
    pub turns: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub tool_calls: i64,
    /// Discounted prompt-cache hit input tokens reported by the provider.
    pub cache_read_tokens: i64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentUsageReport {
    pub agent_id: SnowflakeId,
    pub days: i64,
    pub total_turns: i64,
    pub total_input_tokens: i64,
    pub total_output_tokens: i64,
    pub total_tool_calls: i64,
    pub total_cache_read_tokens: i64,
    pub daily: Vec<AgentUsageDay>,
}

pub async fn usage_report(
    pool: &crate::db::Pool,
    tenant: Option<&str>,
    agent_id: SnowflakeId,
    days: i64,
) -> AppResult<AgentUsageReport> {
    let days = days.clamp(1, 90);
    let to = crate::utils::tz::now_utc();
    let from = to - chrono::Duration::days(days);
    let rows =
        ai_message::agent_turn_meta_rows(pool, tenant, agent_id, Some(from), Some(to)).await?;

    let mut buckets: BTreeMap<String, AgentUsageDay> = BTreeMap::new();
    for row in &rows {
        let Ok(meta) = serde_json::from_str::<serde_json::Value>(&row.content) else {
            continue;
        };
        let usage = meta
            .get("usage_total")
            .and_then(serde_json::Value::as_object);
        let input = usage
            .and_then(|u| u.get("input"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let output = usage
            .and_then(|u| u.get("output"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let tool_calls = meta
            .get("tool_calls_made")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let cache_read = usage
            .and_then(|u| u.get("cache_read"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let date = row.created_at.format("%Y-%m-%d").to_string();
        let bucket = buckets.entry(date.clone()).or_insert(AgentUsageDay {
            date,
            turns: 0,
            input_tokens: 0,
            output_tokens: 0,
            tool_calls: 0,
            cache_read_tokens: 0,
        });
        bucket.turns += 1;
        bucket.input_tokens += input;
        bucket.output_tokens += output;
        bucket.tool_calls += tool_calls;
        bucket.cache_read_tokens += cache_read;
    }

    let mut report = AgentUsageReport {
        agent_id,
        days,
        total_turns: 0,
        total_input_tokens: 0,
        total_output_tokens: 0,
        total_tool_calls: 0,
        total_cache_read_tokens: 0,
        daily: buckets.into_values().collect(),
    };
    for day in &report.daily {
        report.total_turns += day.turns;
        report.total_input_tokens += day.input_tokens;
        report.total_output_tokens += day.output_tokens;
        report.total_tool_calls += day.tool_calls;
        report.total_cache_read_tokens += day.cache_read_tokens;
    }
    Ok(report)
}
