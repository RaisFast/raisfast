//! Conversation session model (`ai_sessions`): an agent + owner conversation
//! with a durable cursor (`last_seq`) and transient `status` (`open`/`running`).
//! Multi-tenant (tenant_id filter).

use serde::{Deserialize, Serialize};

use crate::db::DbDriver;
use crate::errors::app_error::{AppError, AppResult};
use crate::types::snowflake_id::SnowflakeId;
use crate::utils::tz::{Timestamp, now_utc};

/// One conversation session.
#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow)]
pub struct AiSession {
    pub id: SnowflakeId,
    pub tenant_id: Option<String>,
    pub agent_id: SnowflakeId,
    pub user_id: SnowflakeId,
    /// Debate/parent session this was spawned from (`NULL` = top-level;
    /// multi-agent §5). Depth governor reads it via the parent chain.
    pub parent_id: Option<SnowflakeId>,
    pub title: String,
    pub status: String,
    pub meta: Option<serde_json::Value>,
    pub last_seq: i64,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub last_active_at: Timestamp,
}

/// Create a session and return it.
#[allow(clippy::too_many_arguments)]
pub async fn create_session(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    agent_id: SnowflakeId,
    user_id: SnowflakeId,
    title: &str,
) -> AppResult<AiSession> {
    let id = crate::utils::id::new_snowflake_id();
    let now = now_utc();
    raisfast_derive::crud_insert!(
        pool,
        "ai_sessions",
        [
            "id" => id,
            "agent_id" => agent_id,
            "user_id" => user_id,
            "title" => title,
            "status" => "open",
            "last_seq" => 0i64,
            "created_at" => &now,
            "updated_at" => &now,
            "last_active_at" => &now
        ],
        tenant: tenant_id
    )?;
    find_session_by_id(pool, id, tenant_id).await
}

/// Create a child session bound to a parent (debate subagent turn,
/// multi-agent §5). `meta` carries `{debate_id, role}` for audit.
#[allow(clippy::too_many_arguments)]
pub async fn create_child_session(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    agent_id: SnowflakeId,
    user_id: SnowflakeId,
    parent_id: SnowflakeId,
    title: &str,
    meta: serde_json::Value,
) -> AppResult<AiSession> {
    let id = crate::utils::id::new_snowflake_id();
    let now = now_utc();
    raisfast_derive::crud_insert!(
        pool,
        "ai_sessions",
        [
            "id" => id,
            "agent_id" => agent_id,
            "user_id" => user_id,
            "parent_id" => parent_id,
            "title" => title,
            "status" => "open",
            "meta" => meta,
            "last_seq" => 0i64,
            "created_at" => &now,
            "updated_at" => &now,
            "last_active_at" => &now
        ],
        tenant: tenant_id
    )?;
    find_session_by_id(pool, id, tenant_id).await
}

/// Find a session by id (tenant-scoped).
pub async fn find_session_by_id(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
) -> AppResult<AiSession> {
    let result: AiSession = raisfast_derive::crud_find_one!(
        pool,
        "ai_sessions",
        AiSession,
        where: ("id", id),
        tenant: tenant_id
    )?;
    Ok(result)
}

/// List sessions of an agent, most recently active first.
pub async fn list_sessions(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    agent_id: SnowflakeId,
) -> AppResult<Vec<AiSession>> {
    let sql = format!(
        "SELECT id, tenant_id, agent_id, user_id, parent_id, title, status, meta, last_seq, \
         created_at, updated_at, last_active_at FROM ai_sessions \
         WHERE agent_id = {}{} ORDER BY last_active_at DESC",
        crate::db::Driver::ph(1),
        tenant_filter(tenant_id, 2)
    );
    let mut q = sqlx::query_as::<_, AiSession>(crate::db::safe_sql(&sql)).bind(agent_id);
    if let Some(tid) = tenant_id {
        q = q.bind(tid);
    }
    Ok(q.fetch_all(pool).await?)
}

/// Set status (e.g. `running` ↔ `open`). Fails if zero rows affected.
pub async fn set_session_status(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    status: &str,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_sessions",
        bind: ["status" => status, "updated_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_session")
}

/// Replace the session `meta` JSON (e.g. durable context-fold state `ctx`).
pub async fn update_session_meta(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    meta: serde_json::Value,
) -> AppResult<()> {
    let now = now_utc();
    let result = raisfast_derive::crud_update!(
        pool,
        "ai_sessions",
        bind: ["meta" => meta, "updated_at" => &now],
        where: ("id", id),
        tenant: tenant_id
    )?;
    AppError::expect_affected(&result, "ai_session")
}

/// Idempotent cursor advance: only ever moves forward.
pub async fn advance_last_seq(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
    new_seq: i64,
) -> AppResult<()> {
    let sql = format!(
        "UPDATE ai_sessions SET last_seq = {}, updated_at = {}, last_active_at = {} \
         WHERE id = {} AND last_seq < {}{}",
        crate::db::Driver::ph(1),
        crate::db::Driver::now_fn(),
        crate::db::Driver::now_fn(),
        crate::db::Driver::ph(2),
        crate::db::Driver::ph(3),
        tenant_filter(tenant_id, 4)
    );
    let mut q = sqlx::query(crate::db::safe_sql(&sql))
        .bind(new_seq) // $1 last_seq = ?
        .bind(id) // $2 id = ?
        .bind(new_seq); // $3 last_seq < ?
    if let Some(tid) = tenant_id {
        q = q.bind(tid); // $4 tenant_id = ?
    }
    let _ = q.execute(pool).await?;
    Ok(())
}

/// `" AND tenant_id = {ph}"` for `Some`, `""` for `None`.
fn tenant_filter(tenant_id: Option<&str>, start_index: usize) -> String {
    tenant_id
        .map(|_| format!(" AND tenant_id = {}", crate::db::Driver::ph(start_index)))
        .unwrap_or_default()
}

/// Admin listing across agents and users of a tenant. Returns
/// `(items, total)`; most recently active first.
pub async fn admin_list_sessions(
    pool: &crate::db::Pool,
    tenant_id: Option<&str>,
    agent_id: Option<SnowflakeId>,
    user_id: Option<SnowflakeId>,
    status: Option<&str>,
    limit: i64,
    offset: i64,
) -> AppResult<(Vec<AiSession>, i64)> {
    let mut where_clause = String::from(" WHERE 1=1");
    let mut n = 1;
    if tenant_id.is_some() {
        where_clause.push_str(&format!(" AND tenant_id = {}", crate::db::Driver::ph(n)));
        n += 1;
    }
    if agent_id.is_some() {
        where_clause.push_str(&format!(" AND agent_id = {}", crate::db::Driver::ph(n)));
        n += 1;
    }
    if user_id.is_some() {
        where_clause.push_str(&format!(" AND user_id = {}", crate::db::Driver::ph(n)));
        n += 1;
    }
    if status.is_some() {
        where_clause.push_str(&format!(" AND status = {}", crate::db::Driver::ph(n)));
        n += 1;
    }

    let count_sql = format!(
        "SELECT {} FROM ai_sessions{where_clause}",
        crate::db::Driver::cast_int("COUNT(*)")
    );
    let mut q = sqlx::query_scalar::<_, i64>(crate::db::safe_sql(&count_sql));
    if let Some(tid) = tenant_id {
        q = q.bind(tid);
    }
    if let Some(a) = agent_id {
        q = q.bind(a);
    }
    if let Some(u) = user_id {
        q = q.bind(u);
    }
    if let Some(s) = status {
        q = q.bind(s);
    }
    let total = q.fetch_one(pool).await?;

    let list_sql = format!(
        "SELECT id, tenant_id, agent_id, user_id, parent_id, title, status, meta, last_seq, \
         created_at, updated_at, last_active_at FROM ai_sessions{where_clause} \
         ORDER BY last_active_at DESC LIMIT {} OFFSET {}",
        crate::db::Driver::ph(n),
        crate::db::Driver::ph(n + 1)
    );
    let mut q = sqlx::query_as::<_, AiSession>(crate::db::safe_sql(&list_sql));
    if let Some(tid) = tenant_id {
        q = q.bind(tid);
    }
    if let Some(a) = agent_id {
        q = q.bind(a);
    }
    if let Some(u) = user_id {
        q = q.bind(u);
    }
    if let Some(s) = status {
        q = q.bind(s);
    }
    let items = q.bind(limit).bind(offset).fetch_all(pool).await?;
    Ok((items, total))
}

/// Delete a session and its messages in one transaction (service-level
/// cascade, db-schema §8). Memories are agent-scoped and stay.
pub async fn delete_session_cascade(
    pool: &crate::db::Pool,
    id: SnowflakeId,
    tenant_id: Option<&str>,
) -> AppResult<()> {
    crate::in_transaction!(pool, tx, {
        let sql = format!(
            "DELETE FROM ai_messages WHERE session_id = {}{}",
            crate::db::Driver::ph(1),
            tenant_filter(tenant_id, 2)
        );
        let mut q = sqlx::query(crate::db::safe_sql(&sql)).bind(id);
        if let Some(tid) = tenant_id {
            q = q.bind(tid);
        }
        q.execute(&mut *tx).await?;

        if tenant_id.is_some() {
            raisfast_derive::crud_delete!(
                &mut *tx,
                "ai_sessions",
                where: ("id", id),
                tenant: tenant_id
            )?;
        } else {
            raisfast_derive::crud_delete!(&mut *tx, "ai_sessions", where: ("id", id))?;
        }

        Ok::<_, AppError>(())
    })?;
    Ok(())
}
