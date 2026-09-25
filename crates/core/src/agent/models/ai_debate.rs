//! Debate run model (`ai_debates`): one A/B review debate execution
//! (multi-agent §6-§7, `dev-docs/agent/multi-agent-debate.md`).
//!
//! The `ledger` JSON column is the single source of state for the debate
//! (requirement items + disputes + rounds log); `status` is the coarse
//! orchestration state machine (`running` → `consensus`/`escalated`/
//! `failed`/`cancelled` → `finalizing`/`concluded`). Multi-tenant
//! (tenant_id filter), one row per debate run.

use serde::{Deserialize, Serialize};

use crate::db::DbDriver;
use crate::errors::app_error::{AppError, AppResult};
use crate::types::snowflake_id::SnowflakeId;
use crate::utils::tz::{Timestamp, now_utc};

/// One debate run.
#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct AiDebate {
    pub id: SnowflakeId,
    pub tenant_id: Option<String>,
    /// Debate owner — authorized to submit verdicts (multi-agent §8).
    pub user_id: SnowflakeId,
    pub status: String,
    pub agent_a_id: SnowflakeId,
    pub agent_b_id: SnowflakeId,
    /// Origin session for session-anchored debates (`NULL` = cold start).
    pub origin_session_id: Option<SnowflakeId>,
    /// Cold start = raw requirement text; anchored = R0 prompt contract.
    pub requirement: String,
    /// Request-level overrides (`max_rounds` etc.), narrowed by global config.
    pub params: Option<serde_json::Value>,
    /// Dispute-ledger JSON — the debate's single source of state.
    pub ledger: serde_json::Value,
    pub rounds_done: i32,
    /// Escalation report (four-section projection, multi-agent §6.6).
    pub report: Option<String>,
    /// Aggregated usage across all debate turns.
    pub usage_total: Option<serde_json::Value>,
    pub error: Option<String>,
    pub heartbeat_at: Timestamp,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

/// Create a debate row in `running` state with an empty-ledger seed.
#[allow(clippy::too_many_arguments)]
pub async fn create_debate(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    user_id: SnowflakeId,
    agent_a_id: SnowflakeId,
    agent_b_id: SnowflakeId,
    origin_session_id: Option<SnowflakeId>,
    requirement: &str,
    params: Option<serde_json::Value>,
    ledger: serde_json::Value,
) -> AppResult<AiDebate> {
    let id = crate::utils::id::new_snowflake_id();
    let now = now_utc();
    raisfast_derive::crud_insert!(
        pool,
        "ai_debates",
        [
            "id" => id,
            "user_id" => user_id,
            "status" => "running",
            "agent_a_id" => agent_a_id,
            "agent_b_id" => agent_b_id,
            "origin_session_id" => origin_session_id,
            "requirement" => requirement,
            "params" => params,
            "ledger" => ledger,
            "rounds_done" => 0i32,
            "heartbeat_at" => &now,
            "created_at" => &now,
            "updated_at" => &now
        ],
        tenant: tenant_id
    )?;
    find_debate_by_id(pool, id, tenant_id).await
}

/// Find a debate by id (tenant-scoped).
pub async fn find_debate_by_id(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
) -> AppResult<AiDebate> {
    let result: AiDebate = raisfast_derive::crud_find_one!(
        pool,
        "ai_debates",
        AiDebate,
        where: ("id", id),
        tenant: tenant_id
    )?;
    Ok(result)
}

/// Replace the ledger + round counter in one write (single source of state).
pub async fn update_ledger(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    ledger: serde_json::Value,
    rounds_done: i32,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: [
            "ledger" => ledger,
            "rounds_done" => rounds_done,
            "updated_at" => &now
        ],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// Transition `status` (e.g. `running` → `escalated`). Fails if zero rows
/// affected — the caller owns transition legality (orchestrator state machine).
pub async fn set_debate_status(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    status: &str,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: ["status" => status, "updated_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// Persist the escalation report projection (multi-agent §6.6).
pub async fn update_report(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    report: &str,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: ["report" => report, "updated_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// Liveness bump for the reaper (crash recovery, multi-agent §7).
pub async fn update_heartbeat(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: ["heartbeat_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// Record a terminal failure with the reason (`status='failed'`).
pub async fn set_debate_failed(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    error: &str,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: [
            "status" => "failed",
            "error" => error,
            "updated_at" => &now
        ],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// Merge per-turn usage into the aggregate column (read-modify-write through
/// the orchestrator, which is single-writer per debate by construction).
pub async fn update_usage_total(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    usage_total: serde_json::Value,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_debates",
        bind: ["usage_total" => usage_total, "updated_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_debate")
}

/// List the current user's debates, most recent first. Returns
/// `(items, total)` for pagination (multi-agent §11).
pub async fn list_my_debates(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    user_id: SnowflakeId,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<AiDebate>, i64)> {
    let mut where_clause = String::from(" WHERE user_id = ");
    where_clause.push_str(&crate::db::Driver::ph(1));
    let mut n = 2;
    if tenant_id.is_some() {
        where_clause.push_str(&format!(" AND tenant_id = {}", crate::db::Driver::ph(n)));
        n += 1;
    }
    if status.is_some() {
        where_clause.push_str(&format!(" AND status = {}", crate::db::Driver::ph(n)));
        n += 1;
    }
    let count_sql = format!(
        "SELECT {} FROM ai_debates{where_clause}",
        crate::db::Driver::cast_int("COUNT(*)")
    );
    let mut q = sqlx::query_scalar::<_, i64>(crate::db::safe_sql(&count_sql)).bind(user_id);
    if let Some(tid) = tenant_id {
        q = q.bind(tid);
    }
    if let Some(s) = status {
        q = q.bind(s);
    }
    let total = q.fetch_one(pool).await?;

    let list_sql = format!(
        "SELECT id, tenant_id, user_id, status, agent_a_id, agent_b_id, \
         origin_session_id, requirement, params, ledger, rounds_done, report, \
         usage_total, error, heartbeat_at, created_at, updated_at \
         FROM ai_debates{where_clause} ORDER BY created_at DESC LIMIT {} OFFSET {}",
        crate::db::Driver::ph(n),
        crate::db::Driver::ph(n + 1)
    );
    let mut q = sqlx::query_as::<_, AiDebate>(crate::db::safe_sql(&list_sql)).bind(user_id);
    if let Some(tid) = tenant_id {
        q = q.bind(tid);
    }
    if let Some(s) = status {
        q = q.bind(s);
    }
    let items = q.bind(limit).bind(offset).fetch_all(pool).await?;
    Ok((items, total))
}
