//! HTTP handlers for the AI agent core.
//!
//! Thin layer: parse params/auth, delegate to `agent::service`, shape
//! responses. `/api/v1/ai/*` are user-facing (authed, owner-scoped);
//! `/api/v1/admin/ai/*` are admin-scoped.

use std::convert::Infallible;
use std::pin::Pin;
use std::task::{Context, Poll};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use futures::Stream;
use raisfast_agent::CancellationToken;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::AppState;
use crate::agent::service as ai_service;
use crate::agent::service::AgentTurnResult;
use crate::errors::app_error::{AppError, AppResult};
use crate::errors::response::ApiResponse;
use crate::middleware::auth::AuthUser;
use crate::types::snowflake_id::SnowflakeId;

/// Register routes. Paths are prefixed `/api/v1` by `reg_route!`.
pub fn routes(
    registry: &mut crate::server::RouteRegistry,
    _config: &crate::config::app::AppConfig,
) -> axum::Router<AppState> {
    let r = axum::Router::new();
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/tools",
        get,
        admin_list_domain_tools,
        "system",
        "admin/ai/tools",
        "admin"
    );
    #[cfg(feature = "mcp")]
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/tools/mcp",
        get,
        admin_list_mcp_tools,
        "system",
        "admin/ai/tools",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents",
        post,
        admin_create_agent,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents",
        get,
        admin_list_agents,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}",
        put,
        admin_update_agent,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}",
        get,
        admin_get_agent,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}",
        delete,
        admin_delete_agent,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/agents/{agent_id}/sessions",
        post,
        create_session,
        "system",
        "ai/sessions",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/agents/{agent_id}/sessions",
        get,
        list_my_sessions,
        "system",
        "ai/sessions",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/sessions/{id}/messages",
        get,
        get_messages,
        "system",
        "ai/sessions",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}/usage",
        get,
        admin_agent_usage,
        "system",
        "admin/ai/agents/usage",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/sessions/{id}/compact",
        post,
        compact_session,
        "system",
        "ai/sessions/compact",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/sessions/{id}",
        delete,
        delete_session,
        "system",
        "ai/sessions",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/sessions/{id}/debate",
        post,
        start_anchored_debate,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates",
        post,
        start_cold_debate,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates",
        get,
        list_my_debates,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates/{id}",
        get,
        get_debate,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates/{id}",
        delete,
        cancel_debate,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates/{id}/verdicts",
        post,
        submit_debate_verdicts,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/debates/{id}/events",
        get,
        debate_events,
        "system",
        "ai/debates",
        "authed"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}/memories",
        get,
        admin_list_memories,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}/memories",
        post,
        admin_upsert_memory,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}/memories/{mid}",
        put,
        admin_update_memory,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/agents/{id}/memories/{mid}",
        delete,
        admin_delete_memory,
        "system",
        "admin/ai/agents",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/skills",
        get,
        admin_list_skills,
        "system",
        "admin/ai/skills",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/skills",
        post,
        admin_create_skill,
        "system",
        "admin/ai/skills",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/skills/{name}",
        put,
        admin_update_skill,
        "system",
        "admin/ai/skills",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/skills/{name}",
        delete,
        admin_delete_skill,
        "system",
        "admin/ai/skills",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/sessions",
        get,
        admin_list_sessions,
        "system",
        "admin/ai/sessions",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/sessions/{id}",
        delete,
        admin_delete_session,
        "system",
        "admin/ai/sessions",
        "admin"
    );
    let r = reg_route!(
        r,
        registry,
        _config.api_restful,
        "/admin/ai/sessions/{id}/messages",
        get,
        admin_session_messages,
        "system",
        "admin/ai/sessions",
        "admin"
    );
    reg_route!(
        r,
        registry,
        _config.api_restful,
        "/ai/sessions/{id}/turns",
        post,
        run_turn,
        "system",
        "ai/sessions",
        "authed"
    )
}

#[derive(Deserialize)]
pub struct CreateAgentReq {
    pub name: String,
    pub system_prompt: String,
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub channel_id: Option<SnowflakeId>,
    #[serde(default)]
    pub temperature: Option<f64>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default = "default_true")]
    pub memory_enabled: bool,
    #[serde(default)]
    pub params: Option<serde_json::Value>,
}

fn default_true() -> bool {
    true
}

/// `GET /admin/ai/tools` — the domain-tool catalog for the admin agents
/// form and the tools page.
///
/// Built-in/conditional tools only — pure construction, zero IO, no MCP:
/// this endpoint never touches the network and answers instantly.
/// Code/env-level tool changes surface after the mandatory
/// compile/restart with no cache to invalidate. MCP tools live on
/// [`admin_list_mcp_tools`].
///
/// `read_skill` is registered per-turn in compact mode only, but is listed
/// here so admins can allowlist it up front.
pub async fn admin_list_domain_tools(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    auth.ensure_admin()?;
    // Tool *specs* (name/description/category) are identical for every
    // actor; the admin caller's `auth` only matters for execution, which
    // never happens here.
    let mut registry = crate::agent::tools::build_static_tools(&state, &auth, None, None).await;
    // knowledge_search: listed whenever the KB subsystem is enabled
    // (mounting is a per-agent binding configured separately).
    crate::agent::tools::kb::register_catalog(&mut registry, &state);
    // read_skill: per-turn tool (compact mode), listed for allowlisting.
    registry.register(crate::agent::tools::skills::ReadSkillTool::new(
        crate::agent::skills::skills_root(),
        auth.tenant_id().map(str::to_string),
        Vec::new(),
    ));
    let items: Vec<serde_json::Value> = registry
        .specs()
        .into_iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "category": s.category,
            })
        })
        .collect();
    Ok(ApiResponse::success(json!({ "items": items })))
}

/// `GET /admin/ai/tools/mcp` — MCP tools only. This is the only catalog
/// path that talks to MCP servers; specs come from a TTL cache
/// ([`mcp::cached_catalog_specs`]) so a dead/hanging server costs at most
/// one bounded attempt per TTL, and server-side tool changes surface
/// within one TTL.
#[cfg(feature = "mcp")]
pub async fn admin_list_mcp_tools(
    auth: AuthUser,
    State(state): State<AppState>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    auth.ensure_admin()?;
    let items: Vec<serde_json::Value> =
        crate::agent::tools::mcp::cached_catalog_specs(&state.config.ai.mcp_servers)
            .await
            .iter()
            .map(|(name, description)| {
                json!({ "name": name, "description": description, "category": "mcp" })
            })
            .collect();
    Ok(ApiResponse::success(json!({ "items": items })))
}

pub async fn admin_create_agent(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<CreateAgentReq>,
) -> AppResult<ApiResponse<crate::agent::models::ai_agent::AiAgent>> {
    auth.ensure_admin()?;
    let agent = ai_service::create_agent(
        &state.pool,
        auth.tenant_id().map(str::to_string),
        auth.user_id().map(SnowflakeId),
        body.name,
        body.system_prompt,
        body.provider,
        body.model,
        body.channel_id,
        body.temperature,
        body.tools,
        body.memory_enabled,
        body.params,
    )
    .await?;
    Ok(ApiResponse::success(agent))
}

pub async fn admin_update_agent(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(patch): Json<crate::agent::service::AgentPatch>,
) -> AppResult<ApiResponse<crate::agent::models::ai_agent::AiAgent>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let agent =
        crate::agent::service::update_agent(&state.pool, auth.tenant_id(), id, &patch).await?;
    Ok(ApiResponse::success(agent))
}

/// `GET /admin/ai/agents/{id}` — single agent detail (tenant-scoped).
pub async fn admin_get_agent(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<crate::agent::models::ai_agent::AiAgent>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let agent = ai_service::find_agent(&state.pool, id, auth.tenant_id()).await?;
    Ok(ApiResponse::success(agent))
}

/// `DELETE /admin/ai/agents/{id}` — delete an agent, cascading sessions,
/// messages and memories (service-level cascade).
pub async fn admin_delete_agent(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    crate::agent::service::delete_agent(&state.pool, auth.tenant_id(), id).await?;
    Ok(ApiResponse::success(()))
}

pub async fn admin_list_agents(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(mut params): Query<crate::utils::pagination::PaginationParams>,
) -> AppResult<
    ApiResponse<crate::errors::response::PaginatedData<crate::agent::models::ai_agent::AiAgent>>,
> {
    auth.ensure_admin()?;
    params.sanitize();
    let agents = ai_service::list_agents(&state.pool, auth.tenant_id()).await?;
    Ok(params.paginate_in_memory(agents))
}

#[derive(Deserialize)]
pub struct UsageQuery {
    /// Days of history to aggregate (default 30, clamped 1-90).
    #[serde(default = "default_usage_days")]
    pub days: i64,
}

fn default_usage_days() -> i64 {
    30
}

/// `GET /admin/ai/agents/{id}/usage` — daily LLM usage aggregation.
pub async fn admin_agent_usage(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<UsageQuery>,
) -> AppResult<ApiResponse<crate::agent::service::AgentUsageReport>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    // Agent must exist in this tenant before aggregating its usage.
    let _agent = ai_service::find_agent(&state.pool, id, auth.tenant_id()).await?;
    let report = ai_service::usage_report(&state.pool, auth.tenant_id(), id, q.days).await?;
    Ok(ApiResponse::success(report))
}

#[derive(Deserialize)]
pub struct CreateSessionReq {
    #[serde(default)]
    pub title: Option<String>,
}

// ── admin memory management ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct MemoryListQuery {
    #[serde(default)]
    pub q: Option<String>,
    #[serde(default)]
    pub category: Option<String>,
    /// Optional user filter (encoded id string); omit to list cross-user.
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// `GET /admin/ai/agents/{id}/memories` — list live memories of an agent
/// (cross-user by default, optional filters).
pub async fn admin_list_memories(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MemoryListQuery>,
) -> AppResult<ApiResponse<Vec<crate::agent::models::ai_memory::AiMemory>>> {
    auth.ensure_admin()?;
    let agent_id = crate::types::snowflake_id::parse_id(&id)?;
    let _agent = ai_service::find_agent(&state.pool, agent_id, auth.tenant_id()).await?;
    let user_id = match q.user_id.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(crate::types::snowflake_id::parse_id(raw)?),
    };
    let memories = ai_service::list_agent_memories(
        &state.pool,
        auth.tenant_id(),
        agent_id,
        user_id,
        q.category.as_deref().filter(|c| !c.is_empty()),
        q.q.as_deref(),
        q.limit.unwrap_or(200).clamp(1, 1000),
    )
    .await?;
    Ok(ApiResponse::success(memories))
}

#[derive(Deserialize)]
pub struct UpsertMemoryReq {
    pub key: String,
    pub content: String,
    #[serde(default = "default_memory_category")]
    pub category: String,
    #[serde(default = "default_memory_importance")]
    pub importance: f64,
    /// Optional target user (encoded id string); omit for platform-level row.
    #[serde(default)]
    pub user_id: Option<String>,
}

fn default_memory_category() -> String {
    "core".to_string()
}

fn default_memory_importance() -> f64 {
    0.5
}

/// `POST /admin/ai/agents/{id}/memories` — upsert by (agent, user, key).
pub async fn admin_upsert_memory(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<UpsertMemoryReq>,
) -> AppResult<ApiResponse<crate::agent::models::ai_memory::AiMemory>> {
    auth.ensure_admin()?;
    let agent_id = crate::types::snowflake_id::parse_id(&id)?;
    let _agent = ai_service::find_agent(&state.pool, agent_id, auth.tenant_id()).await?;
    let key = body.key.trim();
    if key.is_empty() || body.content.trim().is_empty() {
        return Err(AppError::BadRequest(
            "memory key and content must not be empty".to_string(),
        ));
    }
    let user_id = match body.user_id.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(crate::types::snowflake_id::parse_id(raw)?),
    };
    let memory = ai_service::upsert_agent_memory(
        &state.pool,
        auth.tenant_id(),
        agent_id,
        user_id,
        key,
        body.content.trim(),
        body.category.trim(),
        body.importance,
    )
    .await?;
    Ok(ApiResponse::success(memory))
}

#[derive(Deserialize)]
pub struct UpdateMemoryReq {
    pub content: String,
    pub category: String,
    pub importance: f64,
    pub pinned: bool,
}

/// `PUT /admin/ai/agents/{id}/memories/{mid}` — edit one row by id.
pub async fn admin_update_memory(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((id, mid)): Path<(String, String)>,
    Json(body): Json<UpdateMemoryReq>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let agent_id = crate::types::snowflake_id::parse_id(&id)?;
    let memory_id = crate::types::snowflake_id::parse_id(&mid)?;
    if body.content.trim().is_empty() {
        return Err(AppError::BadRequest(
            "memory content must not be empty".to_string(),
        ));
    }
    ai_service::update_agent_memory(
        &state.pool,
        auth.tenant_id(),
        agent_id,
        memory_id,
        body.content.trim(),
        body.category.trim(),
        body.importance,
        body.pinned,
    )
    .await?;
    Ok(ApiResponse::success(()))
}

/// `DELETE /admin/ai/agents/{id}/memories/{mid}` — delete one row by id.
pub async fn admin_delete_memory(
    auth: AuthUser,
    State(state): State<AppState>,
    Path((id, mid)): Path<(String, String)>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let agent_id = crate::types::snowflake_id::parse_id(&id)?;
    let memory_id = crate::types::snowflake_id::parse_id(&mid)?;
    ai_service::delete_agent_memory(&state.pool, auth.tenant_id(), agent_id, memory_id).await?;
    Ok(ApiResponse::success(()))
}

// ── admin skills management ─────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct SkillScopeQuery {
    /// `platform` (default `tenant` = current admin tenant).
    #[serde(default)]
    pub scope: Option<String>,
}

fn skill_scope(raw: Option<&str>) -> String {
    match raw {
        None | Some("") => "tenant".to_string(),
        Some(s) => s.to_string(),
    }
}

/// `GET /admin/ai/skills` — list skills of both layers for this tenant.
pub async fn admin_list_skills(
    auth: AuthUser,
) -> AppResult<ApiResponse<Vec<crate::agent::skills::admin::AdminSkill>>> {
    auth.ensure_admin()?;
    let skills = crate::agent::skills::admin::list_skills(
        &crate::agent::skills::skills_root(),
        auth.tenant_id(),
    )?;
    Ok(ApiResponse::success(skills))
}

#[derive(Deserialize)]
pub struct CreateSkillReq {
    /// Directory name (slug); also written as frontmatter `name`.
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
    #[serde(default)]
    pub scope: Option<String>,
}

/// `POST /admin/ai/skills` — create a skill directory + SKILL.md.
pub async fn admin_create_skill(
    auth: AuthUser,
    Json(body): Json<CreateSkillReq>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let write = crate::agent::skills::admin::SkillWrite {
        description: body.description,
        instructions: body.instructions,
        always: body.always,
        tools: body.tools,
        disallowed_tools: body.disallowed_tools,
    };
    crate::agent::skills::admin::create_skill(
        &crate::agent::skills::skills_root(),
        auth.tenant_id(),
        &skill_scope(body.scope.as_deref()),
        body.name.trim(),
        &write,
    )?;
    Ok(ApiResponse::success(()))
}

#[derive(Deserialize)]
pub struct UpdateSkillReq {
    pub description: String,
    #[serde(default)]
    pub instructions: String,
    #[serde(default)]
    pub always: bool,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default)]
    pub disallowed_tools: Vec<String>,
}

/// `PUT /admin/ai/skills/{name}?scope=` — overwrite SKILL.md (extra
/// frontmatter fields are preserved).
pub async fn admin_update_skill(
    auth: AuthUser,
    Path(name): Path<String>,
    Query(q): Query<SkillScopeQuery>,
    Json(body): Json<UpdateSkillReq>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let write = crate::agent::skills::admin::SkillWrite {
        description: body.description,
        instructions: body.instructions,
        always: body.always,
        tools: body.tools,
        disallowed_tools: body.disallowed_tools,
    };
    crate::agent::skills::admin::update_skill(
        &crate::agent::skills::skills_root(),
        auth.tenant_id(),
        &skill_scope(q.scope.as_deref()),
        &name,
        &write,
    )?;
    Ok(ApiResponse::success(()))
}

/// `DELETE /admin/ai/skills/{name}?scope=` — remove a skill directory.
pub async fn admin_delete_skill(
    auth: AuthUser,
    Path(name): Path<String>,
    Query(q): Query<SkillScopeQuery>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    crate::agent::skills::admin::delete_skill(
        &crate::agent::skills::skills_root(),
        auth.tenant_id(),
        &skill_scope(q.scope.as_deref()),
        &name,
    )?;
    Ok(ApiResponse::success(()))
}

pub async fn create_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
    Json(body): Json<CreateSessionReq>,
) -> AppResult<ApiResponse<crate::agent::models::ai_session::AiSession>> {
    let owner = current_owner(&auth)?;
    let agent_id = crate::types::snowflake_id::parse_id(&agent_id)?;
    let tenant = auth.tenant_id().map(str::to_string);
    // Agent must exist in this tenant.
    let _agent = ai_service::find_agent(&state.pool, agent_id, tenant.as_deref()).await?;
    let session = ai_service::create_session(
        &state.pool,
        tenant,
        agent_id,
        owner,
        body.title.as_deref().unwrap_or(""),
    )
    .await?;
    Ok(ApiResponse::success(session))
}

pub async fn list_my_sessions(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(agent_id): Path<String>,
) -> AppResult<ApiResponse<Vec<crate::agent::models::ai_session::AiSession>>> {
    let owner = current_owner(&auth)?;
    let agent_id = crate::types::snowflake_id::parse_id(&agent_id)?;
    let sessions =
        ai_service::list_my_sessions(&state.pool, auth.tenant_id(), agent_id, owner).await?;
    Ok(ApiResponse::success(sessions))
}

#[derive(Deserialize)]
pub struct MessagesQuery {
    #[serde(default)]
    pub after_seq: Option<i64>,
    #[serde(default)]
    pub limit: Option<i64>,
}

pub async fn get_messages(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> AppResult<ApiResponse<Vec<crate::agent::models::ai_message::AiMessage>>> {
    let owner = current_owner(&auth)?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let session = ai_service::find_session(&state.pool, id, auth.tenant_id()).await?;
    if session.user_id != owner {
        return Err(AppError::ForbiddenOwnership);
    }
    let messages = ai_service::list_messages(
        &state.pool,
        session.id,
        auth.tenant_id(),
        q.after_seq,
        q.limit.unwrap_or(200).clamp(1, 1000),
    )
    .await?;
    Ok(ApiResponse::success(messages))
}

// ── admin sessions management ───────────────────────────────────────────────

#[derive(Deserialize)]
pub struct AdminSessionQuery {
    #[serde(default)]
    pub page: Option<i64>,
    #[serde(default)]
    pub page_size: Option<i64>,
    /// Encoded agent id filter.
    #[serde(default)]
    pub agent_id: Option<String>,
    /// Encoded user id filter.
    #[serde(default)]
    pub user_id: Option<String>,
    /// `open` / `running` / `closed` / `archived`.
    #[serde(default)]
    pub status: Option<String>,
}

/// `GET /admin/ai/sessions` — paginated listing across agents and users.
pub async fn admin_list_sessions(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(q): Query<AdminSessionQuery>,
) -> AppResult<
    ApiResponse<
        crate::errors::response::PaginatedData<crate::agent::models::ai_session::AiSession>,
    >,
> {
    auth.ensure_admin()?;
    let params = crate::utils::pagination::PaginationParams::from_options(q.page, q.page_size);
    let agent_id = match q.agent_id.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(crate::types::snowflake_id::parse_id(raw)?),
    };
    let user_id = match q.user_id.as_deref() {
        None | Some("") => None,
        Some(raw) => Some(crate::types::snowflake_id::parse_id(raw)?),
    };
    let (items, total) = ai_service::admin_list_sessions(
        &state.pool,
        auth.tenant_id(),
        agent_id,
        user_id,
        q.status.as_deref().filter(|s| !s.is_empty()),
        params.page_size,
        params.offset(),
    )
    .await?;
    Ok(params.paginate(items, total))
}

/// `DELETE /admin/ai/sessions/{id}` — delete a session and its messages.
pub async fn admin_delete_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<()>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    ai_service::delete_session(&state.pool, auth.tenant_id(), id).await?;
    Ok(ApiResponse::success(()))
}

/// `GET /admin/ai/sessions/{id}/messages` — read-only replay for admins
/// (no owner check; tenant-scoped).
pub async fn admin_session_messages(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Query(q): Query<MessagesQuery>,
) -> AppResult<ApiResponse<Vec<crate::agent::models::ai_message::AiMessage>>> {
    auth.ensure_admin()?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let _session = ai_service::find_session(&state.pool, id, auth.tenant_id()).await?;
    let messages = ai_service::list_messages(
        &state.pool,
        id,
        auth.tenant_id(),
        q.after_seq,
        q.limit.unwrap_or(200).clamp(1, 1000),
    )
    .await?;
    Ok(ApiResponse::success(messages))
}

#[derive(Deserialize)]
pub struct TurnReq {
    pub content: String,
}

/// `POST /api/v1/ai/sessions/{id}/compact` — manual LLM compaction (opencode
/// `/compact` analog). Folds the oldest turns into a durable summary now.
pub async fn compact_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let owner = current_owner(&auth)?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let session = ai_service::find_session(&state.pool, id, auth.tenant_id()).await?;
    if session.user_id != owner {
        return Err(AppError::ForbiddenOwnership);
    }
    let agent = ai_service::find_agent(&state.pool, session.agent_id, auth.tenant_id()).await?;
    let result = ai_service::compact_session(
        &state.pool,
        &state.config.ai,
        &state.llm_router,
        &agent,
        session.id,
        auth.tenant_id(),
    )
    .await?;
    Ok(ApiResponse::success(json!({
        "compacted": result.is_some(),
        "cover_seq": result.as_ref().map(|(c, _)| c),
        "summary": result.map(|(_, s)| s),
    })))
}

/// `DELETE /api/v1/ai/sessions/{id}` — owner-scoped session deletion
/// (cascade: session + its messages).
pub async fn delete_session(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let owner = current_owner(&auth)?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let session = ai_service::find_session(&state.pool, id, auth.tenant_id()).await?;
    if session.user_id != owner {
        return Err(AppError::ForbiddenOwnership);
    }
    ai_service::delete_session(&state.pool, auth.tenant_id(), session.id).await?;
    Ok(ApiResponse::success(json!({ "deleted": true })))
}

/// `POST /api/v1/ai/sessions/{id}/turns` — streamed SSE of one turn.
pub async fn run_turn(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<TurnReq>,
) -> AppResult<Sse<CancelOnDrop<ReceiverStream<Result<SseEvent, Infallible>>>>> {
    let owner = current_owner(&auth)?;
    let id = crate::types::snowflake_id::parse_id(&id)?;
    let session = ai_service::find_session(&state.pool, id, auth.tenant_id()).await?;
    if session.user_id != owner {
        return Err(AppError::ForbiddenOwnership);
    }
    let agent = ai_service::find_agent(&state.pool, session.agent_id, auth.tenant_id()).await?;
    // Reviewer-component agents are debate-internal (multi-agent D-A14):
    // their prompt is ledger-shaped, so direct chat is undefined behavior.
    // Admin playground stays exempt for prompt debugging.
    let is_reviewer = agent
        .params
        .as_ref()
        .and_then(|p| p.get("debate_role"))
        .and_then(serde_json::Value::as_str)
        .is_some_and(|r| r == "reviewer");
    if is_reviewer && !auth.is_super_admin() {
        return Err(AppError::Conflict(
            "reviewer component agent — use /ai/debates (multi-agent D-A14)".into(),
        ));
    }
    let extra_tools =
        crate::agent::tools::build_domain_tools(&state, &auth, Some(&agent), Some(session.id))
            .await;

    let pool = state.pool.clone();
    let ai_cfg = state.config.ai.clone();
    let router = state.llm_router.clone();
    let emitter = state.emitter.clone();
    let broadcast = state.config.ai.broadcast_events;
    let content = body.content;

    let cancel = CancellationToken::new();
    let task_cancel = cancel.clone();
    let (tx, rx) = mpsc::channel::<Result<SseEvent, Infallible>>(64);
    tracing::info!(session = session.id.0, "ai turn: starting (streamed)");
    tokio::spawn(async move {
        let mut emit = |ev: raisfast_agent::TurnEvent| {
            let _ = tx.try_send(Ok(agent_event(ev)));
        };
        let result = ai_service::run_turn_streamed(
            &pool,
            &ai_cfg,
            &router,
            &agent,
            session.id,
            &content,
            extra_tools,
            Some(task_cancel),
            &mut emit,
        )
        .await;

        match result {
            Ok(outcome) => {
                tracing::info!(
                    session = session.id.0,
                    text_len = outcome.text.len(),
                    "ai turn: done"
                );
                if broadcast {
                    emitter.emit(crate::event::Event::Custom {
                        source: "ai".to_string(),
                        event_type: "ai.turn.done".to_string(),
                        data: json!({
                            "session_id": session.id.0,
                            "agent_id": agent.id.0,
                            "text": outcome.text,
                            "iterations": outcome.iterations,
                            "tool_calls_made": outcome.tool_calls_made,
                        }),
                    });
                }
                let _ = tx.send(Ok(done_event(&outcome))).await;
            }
            Err(e) => {
                tracing::warn!(session = session.id.0, error = %e, "ai turn: failed");
                if broadcast {
                    emitter.emit(crate::event::Event::Custom {
                        source: "ai".to_string(),
                        event_type: "ai.turn.error".to_string(),
                        data: json!({
                            "session_id": session.id.0,
                            "agent_id": agent.id.0,
                            "message": e.to_string(),
                        }),
                    });
                }
                let _ = tx
                    .send(Ok(SseEvent::default().event("error").data(
                        json!({
                            "code": "turn_failed",
                            "message": e.to_string(),
                            "fatal": true,
                        })
                        .to_string(),
                    )))
                    .await;
            }
        }
    });

    Ok(Sse::new(CancelOnDrop {
        inner: ReceiverStream::new(rx),
        cancel,
    }))
}

/// Stream wrapper that cancels the running turn when the SSE response is
/// dropped (client disconnected). The engine then stops at the next checkpoint
/// and the service persists the partial transcript.
pub struct CancelOnDrop<S> {
    inner: S,
    cancel: CancellationToken,
}

impl<S: Stream + Unpin> Stream for CancelOnDrop<S> {
    type Item = S::Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

impl<S> Drop for CancelOnDrop<S> {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

// ── helpers ────────────────────────────────────────────────────────────────

fn current_owner(auth: &AuthUser) -> AppResult<SnowflakeId> {
    auth.user_id()
        .map(SnowflakeId)
        .ok_or(AppError::Unauthorized)
}

fn agent_event(ev: raisfast_agent::TurnEvent) -> SseEvent {
    match ev {
        raisfast_agent::TurnEvent::Chunk { delta } => SseEvent::default()
            .event("chunk")
            .data(json!({ "delta": delta }).to_string()),
        raisfast_agent::TurnEvent::Thinking { delta } => SseEvent::default()
            .event("thinking")
            .data(json!({ "delta": delta }).to_string()),
        raisfast_agent::TurnEvent::Text { text } => SseEvent::default()
            .event("text")
            .data(json!({ "text": text }).to_string()),
        raisfast_agent::TurnEvent::ToolCall { name, arguments } => SseEvent::default()
            .event("tool_call")
            .data(json!({ "name": name, "args": arguments }).to_string()),
        raisfast_agent::TurnEvent::ToolResult { name, output } => {
            SseEvent::default().event("tool_result").data(
                json!({
                    "name": name,
                    "output": output,
                    "success": !raisfast_agent::tool_output_failed(&output),
                })
                .to_string(),
            )
        }
    }
}

fn done_event(outcome: &AgentTurnResult) -> SseEvent {
    let data = json!({
        "outcome": {
            "text": outcome.text,
            "iterations": outcome.iterations,
            "tool_calls_made": outcome.tool_calls_made,
            "usage": outcome.usage.as_ref().map(|u| json!({
                "input": u.input_tokens,
                "output": u.output_tokens,
            })),
        },
    });
    SseEvent::default().event("done").data(data.to_string())
}

// ── Debates (multi-agent M-A4; dev-docs/agent/multi-agent-debate.md §11) ──────

use crate::agent::debate::orchestrator::{self, VerdictInput};
use crate::agent::models::ai_debate as debate_model;

#[derive(Deserialize)]
pub struct AnchoredDebateReq {
    pub reviewer_agent_id: String,
    pub max_rounds: Option<u32>,
}

#[derive(Deserialize)]
pub struct ColdDebateReq {
    pub agent_a_id: String,
    pub agent_b_id: String,
    pub requirement: String,
    pub max_rounds: Option<u32>,
}

#[derive(Deserialize)]
pub struct VerdictsReq {
    pub verdicts: Vec<VerdictBody>,
}

#[derive(Deserialize)]
pub struct VerdictBody {
    pub dispute_id: String,
    pub verdict: String,
    pub resolution: Option<String>,
    pub note: Option<String>,
}

/// Owner-or-admin policy shared by debate read/verdict/cancel endpoints.
fn debate_access(auth: &AuthUser, debate: &debate_model::AiDebate) -> AppResult<()> {
    let owner = current_owner(auth)?;
    if debate.user_id == owner || auth.is_super_admin() {
        Ok(())
    } else {
        Err(AppError::ForbiddenOwnership)
    }
}

/// `POST /ai/sessions/{id}/debate` — session-anchored primary entry
/// (multi-agent §6.2): the proposer is the session's own agent and R0 runs
/// in the origin session itself.
pub async fn start_anchored_debate(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<AnchoredDebateReq>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let owner = current_owner(&auth)?;
    let session_id = crate::types::snowflake_id::parse_id(&id)?;
    let session = ai_service::find_session(&state.pool, session_id, auth.tenant_id()).await?;
    if session.user_id != owner {
        return Err(AppError::ForbiddenOwnership);
    }
    let reviewer = crate::types::snowflake_id::parse_id(&body.reviewer_agent_id)?;
    let debate = orchestrator::start_debate(
        &state,
        &auth,
        session.agent_id,
        reviewer,
        Some(session.id),
        if session.title.is_empty() {
            format!("session {}", session.id.0)
        } else {
            session.title.clone()
        },
        body.max_rounds,
    )
    .await?;
    Ok(ApiResponse::success(json!({ "debate": debate })))
}

/// `POST /ai/debates` — cold start (headless/API): explicit requirement text.
pub async fn start_cold_debate(
    auth: AuthUser,
    State(state): State<AppState>,
    Json(body): Json<ColdDebateReq>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let a = crate::types::snowflake_id::parse_id(&body.agent_a_id)?;
    let b = crate::types::snowflake_id::parse_id(&body.agent_b_id)?;
    let debate =
        orchestrator::start_debate(&state, &auth, a, b, None, body.requirement, body.max_rounds)
            .await?;
    Ok(ApiResponse::success(json!({ "debate": debate })))
}

/// `GET /ai/debates/{id}` — status + ledger + report.
pub async fn get_debate(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let debate_id = crate::types::snowflake_id::parse_id(&id)?;
    let debate = debate_model::find_debate_by_id(&state.pool, debate_id, auth.tenant_id()).await?;
    debate_access(&auth, &debate)?;
    Ok(ApiResponse::success(json!({ "debate": debate })))
}

/// `POST /ai/debates/{id}/verdicts` — human judgments (§8); when all
/// escalated disputes are consumed the final round runs in the background.
pub async fn submit_debate_verdicts(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<VerdictsReq>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let debate_id = crate::types::snowflake_id::parse_id(&id)?;
    let debate = debate_model::find_debate_by_id(&state.pool, debate_id, auth.tenant_id()).await?;
    debate_access(&auth, &debate)?;
    let verdicts: Vec<VerdictInput> = body
        .verdicts
        .into_iter()
        .map(|v| VerdictInput {
            dispute_id: v.dispute_id,
            verdict: v.verdict,
            resolution: v.resolution,
            note: v.note,
        })
        .collect();
    let updated = orchestrator::submit_verdicts(&state, &auth, debate_id, verdicts).await?;
    Ok(ApiResponse::success(json!({ "debate": updated })))
}

/// `DELETE /ai/debates/{id}` — cancel a `running` debate. The orchestrator
/// checks the row status before each round and bails; an in-flight engine
/// turn finishes naturally (bounded by its own max_iterations).
pub async fn cancel_debate(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let debate_id = crate::types::snowflake_id::parse_id(&id)?;
    let debate = debate_model::find_debate_by_id(&state.pool, debate_id, auth.tenant_id()).await?;
    debate_access(&auth, &debate)?;
    if debate.status != orchestrator::status::RUNNING {
        return Err(AppError::BadRequest(format!(
            "debate is {} — only running debates can be cancelled",
            debate.status
        )));
    }
    debate_model::set_debate_status(&state.pool, debate_id, auth.tenant_id(), "cancelled").await?;
    Ok(ApiResponse::success(json!({ "cancelled": true })))
}

/// `GET /ai/debates/{id}/events` — SSE forwarding of `ai.debate.*` events
/// for this debate (round-level progress; no token deltas by D-A9).
pub async fn debate_events(
    auth: AuthUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> AppResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    let debate_id = crate::types::snowflake_id::parse_id(&id)?;
    let debate = debate_model::find_debate_by_id(&state.pool, debate_id, auth.tenant_id()).await?;
    debate_access(&auth, &debate)?;

    let rx = state.eventbus.subscribe();
    let stream = tokio_stream::StreamExt::filter_map(
        tokio_stream::wrappers::BroadcastStream::new(rx),
        move |result| match result {
            Ok(arc_event) => match arc_event.as_ref() {
                crate::event::Event::Custom {
                    source: _,
                    event_type,
                    data,
                } if event_type.starts_with("ai.debate.") => {
                    let matches = data
                        .get("debate_id")
                        .and_then(serde_json::Value::as_i64)
                        .is_some_and(|v| v == debate_id.0);
                    if matches {
                        Some(Ok(SseEvent::default()
                            .event(event_type.clone())
                            .data(data.to_string())))
                    } else {
                        None
                    }
                }
                _ => None,
            },
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!("debate SSE lagged, skipped {n} events");
                None
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(std::time::Duration::from_secs(30))
            .text("ping"),
    ))
}

/// `GET /ai/debates` — the current user's debates (most recent first).
#[derive(Deserialize)]
pub struct ListDebatesQuery {
    pub status: Option<String>,
    pub page: Option<i64>,
    pub page_size: Option<i64>,
}
pub async fn list_my_debates(
    auth: AuthUser,
    State(state): State<AppState>,
    Query(q): Query<ListDebatesQuery>,
) -> AppResult<ApiResponse<serde_json::Value>> {
    let owner = current_owner(&auth)?;
    let page = q.page.unwrap_or(1).max(1);
    let page_size = q.page_size.unwrap_or(20).clamp(1, 100);
    let (items, total) = debate_model::list_my_debates(
        &state.pool,
        auth.tenant_id(),
        owner,
        q.status.as_deref(),
        page_size,
        (page - 1) * page_size,
    )
    .await?;
    Ok(ApiResponse::success(
        json!({ "items": items, "total": total, "page": page, "page_size": page_size }),
    ))
}
