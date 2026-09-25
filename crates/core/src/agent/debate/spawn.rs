//! Delegation primitive: run one full agent turn in a fresh child session
//! (multi-agent §5, `dev-docs/agent/multi-agent-debate.md`).
//!
//! `[照抄 opencode tool/task.ts]` shape — new session per spawn, context
//! isolation (child history = this input only), result contract = final
//! assistant text only — combined with `[照抄 claw-code tools/lib.rs
//! execute_agent]`'s manifest discipline (audit via the child session's own
//! two-phase persistence, free from `ai_messages`).
//!
//! The debate orchestrator is the only caller in MVP; the model never sees a
//! spawn tool, so the tree depth is structurally 1 and no recursion guard is
//! needed yet (`parent_id` fields are already in place for phase-2 tooling).

use serde_json::json;

use crate::agent::models::{ai_agent, ai_session};
use crate::agent::service::{self, AgentTurnResult};
use crate::agent::tools::build_domain_tools;
use crate::errors::app_error::{AppError, AppResult};
use crate::middleware::auth::AuthUser;
use crate::types::snowflake_id::SnowflakeId;

/// Result of one delegated subagent turn.
pub struct AgentTurnOutput {
    /// Child session (full transcript = audit trail, multi-agent §12).
    pub session_id: SnowflakeId,
    /// Final assistant text (the only thing the caller sees — intermediate
    /// iterations never leak into the parent context).
    pub text: String,
    pub turns: AgentTurnResult,
}

/// One delegated-turn request (multi-agent §5).
pub struct AgentTurnRequest<'a> {
    pub agent_id: SnowflakeId,
    /// `Some` = debate child session (`parent_id` link); `None` = top-level.
    pub parent_session_id: Option<SnowflakeId>,
    pub title: &'a str,
    /// Audit role in `meta` (`proposer` / `reviewer`).
    pub role: &'a str,
    pub debate_id: Option<SnowflakeId>,
    pub input: &'a str,
}

/// Run one turn for the request's agent in a new child session.
///
/// - **Fail-closed validation**: the target agent must exist in the same
///   tenant; reviewer-component agents (`debate_role = "reviewer"`) are
///   spawnable only by the orchestrator, which is the only caller today
///   (multi-agent D-A14).
/// - **Actor snapshot**: `auth` is the debate owner's identity, captured at
///   debate creation and reused for every subagent turn so domain tools
///   (KB/file reads) hit the service layer with correct ownership — this is
///   the headless-tool-surface fix (multi-agent README gap note).
/// - **Isolation**: the child session's history starts with this `input`
///   only; nothing from the origin session leaks in (§6.4 ledger-as-carrier).
pub async fn run_agent_turn(
    state: &crate::AppState,
    auth: &AuthUser,
    req: &AgentTurnRequest<'_>,
) -> AppResult<AgentTurnOutput> {
    let tenant = auth.tenant_id();
    let agent = ai_agent::find_agent_by_id(&state.pool, req.agent_id, tenant).await?;
    let owner = actor_user_id(auth)?;

    let meta = json!({ "debate_id": req.debate_id, "role": req.role });
    let session = match req.parent_session_id {
        Some(parent) => {
            ai_session::create_child_session(
                &state.pool,
                tenant,
                agent.id,
                owner,
                parent,
                req.title,
                meta,
            )
            .await?
        }
        None => ai_session::create_session(&state.pool, tenant, agent.id, owner, req.title).await?,
    };

    // Domain tools bound to the debate owner's actor snapshot + the child
    // session (read attribution). The per-agent `tools` allowlist is applied
    // inside `run_turn_streamed` — the reviewer's read-only surface is pure
    // `ai_agents.tools` config (multi-agent D-A4).
    let extra_tools = build_domain_tools(state, auth, Some(&agent), Some(session.id)).await;

    let result = service::run_turn_streamed(
        &state.pool,
        &state.config.ai,
        &state.llm_router,
        &agent,
        session.id,
        req.input,
        extra_tools,
        None,
        &mut |_| {},
    )
    .await?;

    Ok(AgentTurnOutput {
        session_id: session.id,
        text: result.text.clone(),
        turns: result,
    })
}

/// Memory scope owner for the child session: the debate owner (the actor
/// snapshot). Agents are always driven on behalf of an authenticated user.
fn actor_user_id(auth: &AuthUser) -> AppResult<SnowflakeId> {
    auth.user_id()
        .map(SnowflakeId)
        .ok_or_else(|| AppError::Unauthorized)
}

/// Run one more turn on an **existing** session (single repair retry when a
/// round's JSON output fails validation, multi-agent §6.2). Rebuilds the
/// domain tools from the same actor snapshot; the transcript continuity
/// comes from the session log, so the model sees its previous answer.
pub async fn continue_agent_turn(
    state: &crate::AppState,
    auth: &AuthUser,
    session_id: SnowflakeId,
    input: &str,
) -> AppResult<AgentTurnResult> {
    let tenant = auth.tenant_id();
    let session = ai_session::find_session_by_id(&state.pool, session_id, tenant).await?;
    let agent = ai_agent::find_agent_by_id(&state.pool, session.agent_id, tenant).await?;
    let extra_tools = build_domain_tools(state, auth, Some(&agent), Some(session.id)).await;
    service::run_turn_streamed(
        &state.pool,
        &state.config.ai,
        &state.llm_router,
        &agent,
        session.id,
        input,
        extra_tools,
        None,
        &mut |_| {},
    )
    .await
}
