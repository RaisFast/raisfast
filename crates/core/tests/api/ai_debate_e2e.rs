//! Debate orchestrator e2e (multi-agent §6.5, M-A2 acceptance):
//! full R0 → challenge → defense loop against a scripted mock OpenAI-compatible
//! SSE provider — consensus path and max-rounds escalation path.

use super::*;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use raisfast::agent::debate::ledger::DisputeStatus;
use raisfast::agent::debate::ledger::Ledger;
use raisfast::agent::models::ai_debate as debate_model;
use raisfast::errors::app_error::AppResult;
use raisfast::middleware::auth::AuthUser;
use raisfast::models::user::UserRole;
use raisfast::types::snowflake_id::SnowflakeId;

/// Scripted mock: each `/v1/chat/completions` call pops the next payload and
/// returns it as one SSE content delta + usage frame.
async fn mock_llm(script: Vec<serde_json::Value>) -> String {
    let counter = Arc::new(AtomicUsize::new(0));
    let script = Arc::new(script);
    let app = axum::Router::new().route(
        "/v1/chat/completions",
        axum::routing::post(move |body: Option<axum::Json<serde_json::Value>>| {
            let counter = counter.clone();
            let script = script.clone();
            async move {
                let _ = body;
                let i = counter.fetch_add(1, Ordering::SeqCst);
                let content = script
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| serde_json::json!("{}"))
                    .to_string();
                let frame = serde_json::json!({
                    "choices": [{ "delta": { "role": "assistant", "content": content } }],
                    "usage": { "prompt_tokens": 100, "completion_tokens": 40 }
                });
                let body = format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    serde_json::to_string(&frame).unwrap()
                );
                axum::http::Response::builder()
                    .header("content-type", "text/event-stream")
                    .body(axum::body::Body::from(body))
                    .unwrap()
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let addr = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    format!("http://{addr}/v1")
}

async fn create_mock_channel(
    state: &AppState,
    tenant: &str,
    base_url: &str,
) -> AppResult<(SnowflakeId, AppState)> {
    use raisfast::llm::models::channel::{LlmCostMode, LlmKeyMode, NewChannel};
    let ch = raisfast::llm::models::channel::create_channel(
        &state.pool,
        Some(tenant),
        NewChannel {
            name: uniq("mock-debate"),
            provider: "generic".into(),
            base_url: base_url.to_string(),
            api_keys: serde_json::json!([{ "key": "test-key", "status": "active" }]),
            key_mode: LlmKeyMode::Polling,
            models: "gpt-4o-mini".into(),
            model_mapping: None,
            priority: 0,
            weight: 0,
            channel_groups: "default".into(),
            auto_ban: false,
            param_override: None,
            header_override: None,
            config: None,
            cost_mode: LlmCostMode::Usage,
            cost_discount: 1.0,
            monthly_cost: None,
            test_model: None,
        },
    )
    .await?;
    // The harness router is persistence-less (empty cache, no pool) — build
    // a DB-backed router that sees this channel and clone the state over it.
    let router = raisfast::llm::service::LlmRouter::new(state.pool.clone()).await;
    Ok((
        ch.id,
        AppState {
            llm_router: router,
            ..state.clone()
        },
    ))
}

#[allow(clippy::too_many_arguments)]
async fn create_actor(
    state: &AppState,
    tenant: &str,
    user_id: i64,
    name: &str,
    system: &str,
    tools: Vec<String>,
    channel_id: SnowflakeId,
) -> SnowflakeId {
    let agent = raisfast::agent::models::ai_agent::create_agent(
        &state.pool,
        Some(tenant),
        Some(SnowflakeId(user_id)),
        name,
        system,
        "openai_compat",
        "gpt-4o-mini",
        Some(channel_id),
        None,
        tools,
        true,
        None,
    )
    .await
    .expect("create debate actor");
    agent.id
}

/// Poll until the debate leaves `running` (background task), then return it.
async fn wait_terminal(
    pool: &raisfast::db::Pool,
    id: SnowflakeId,
    tenant: &str,
) -> debate_model::AiDebate {
    for _ in 0..150 {
        if let Ok(row) = debate_model::find_debate_by_id(pool, id, Some(tenant)).await
            && row.status != "running"
        {
            return row;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("debate did not reach a terminal state in time");
}

#[tokio::test]
async fn debate_consensus_path() {
    let (_app, state) = test_app().await;
    let tenant = uniq("t_debate");

    let base_url = mock_llm(vec![
        // R0 (proposer itemization)
        serde_json::json!({
            "summary": "Export feature",
            "items": [{ "id": "REQ-1", "story": "As a user, I want export",
                        "criteria": ["WHEN export THE SYSTEM SHALL skip drafts"] }]
        }),
        // Round 1 reviewer challenge
        serde_json::json!({
            "coverage": ["range", "edge cases"],
            "new_challenges": [{
                "target": "REQ-1", "type": "ambiguity", "severity": "major",
                "nature": "ambiguity",
                "evidence": "草稿口径未定义，实现会分叉",
                "sources": ["REQ-1"]
            }],
            "re_visits": []
        }),
        // Round 1 proposer defense: accept + revise
        serde_json::json!({
            "responses": [{ "dispute_id": "D1", "stance": "accept",
                            "reasoning": "确实含糊，采纳", "sources": [],
                            "revision_targets": ["REQ-1"] }],
            "revised_items": [{ "id": "REQ-1", "story": "As a user, I want export",
                                "criteria": ["WHEN export THE SYSTEM SHALL exclude drafts"],
                                "change_note": "D1：明确导出排除草稿" }]
        }),
    ])
    .await;

    let (channel_id, state) = create_mock_channel(&state, &tenant, &base_url)
        .await
        .unwrap();
    let a = create_actor(
        &state,
        &tenant,
        9001,
        &uniq("proposer"),
        "propose",
        vec!["*".into()],
        channel_id,
    )
    .await;
    let b = create_actor(
        &state,
        &tenant,
        9001,
        &uniq("reviewer"),
        "review",
        vec![
            "knowledge_search".into(),
            "memory_store".into(),
            "memory_recall".into(),
            "memory_forget".into(),
        ],
        channel_id,
    )
    .await;

    let auth = AuthUser::from_parts(Some(9001), UserRole::Author, Some(tenant.clone()));
    let debate = raisfast::agent::debate::orchestrator::start_debate(
        &state,
        &auth,
        a,
        b,
        None,
        "给内容系统加导出功能".into(),
        Some(3),
    )
    .await
    .expect("start debate");

    let final_row = wait_terminal(&state.pool, debate.id, &tenant).await;
    assert_eq!(
        final_row.status, "consensus",
        "A accepts → all resolved → consensus; error={:?}",
        final_row.error
    );
    let ledger: Ledger = serde_json::from_value(final_row.ledger.clone()).expect("ledger");
    assert_eq!(ledger.disputes.len(), 1);
    assert_eq!(ledger.disputes[0].status, DisputeStatus::Improved);
    assert_eq!(
        ledger.items[0].criteria[0], "WHEN export THE SYSTEM SHALL exclude drafts",
        "revision applied to the item"
    );
    assert_eq!(final_row.rounds_done, 1);
    assert!(
        final_row
            .usage_total
            .as_ref()
            .and_then(|u| u.get("input_tokens"))
            .is_some(),
        "usage aggregated into the debate row"
    );
}

#[tokio::test]
async fn debate_escalates_on_max_rounds() {
    let (_app, state) = test_app().await;
    let tenant = uniq("t_debate");

    let base_url = mock_llm(vec![
        serde_json::json!({
            "summary": "Export feature",
            "items": [{ "id": "REQ-1", "story": "As a user, I want export",
                        "criteria": ["WHEN export THE SYSTEM SHALL skip drafts"] }]
        }),
        serde_json::json!({
            "coverage": ["range"],
            "new_challenges": [{
                "target": "REQ-1", "type": "ambiguity", "severity": "blocker",
                "evidence": "草稿口径冲突", "sources": ["REQ-1 §2"]
            }],
            "re_visits": []
        }),
        // Proposer rejects with sources — dispute stays open.
        serde_json::json!({
            "responses": [{ "dispute_id": "D1", "stance": "reject",
                            "reasoning": "口径已由需求原文锁定", "sources": ["需求原文 §2"] }],
            "revised_items": []
        }),
        // Escalation brief (reviewer fills recommendation/cost/impact)
        serde_json::json!({
            "briefs": [{ "dispute_id": "D1", "nature": "value",
                         "recommendation": "倾向 B：口径含糊会导致两种实现",
                         "cost_if_a": "两种实现分叉，返工一轮",
                         "cost_if_b": "多一天分析",
                         "impact": "阻塞导出功能交付" }]
        }),
        // Final round: complete item list after the human verdict
        serde_json::json!({
            "summary": "Export feature (final)",
            "items": [{ "id": "REQ-1", "story": "As a user, I want export",
                        "criteria": ["WHEN export THE SYSTEM SHALL exclude drafts and template pages"] }]
        }),
    ])
    .await;

    let (channel_id, state) = create_mock_channel(&state, &tenant, &base_url)
        .await
        .unwrap();
    let a = create_actor(
        &state,
        &tenant,
        9002,
        &uniq("proposer"),
        "propose",
        vec!["*".into()],
        channel_id,
    )
    .await;
    let b = create_actor(
        &state,
        &tenant,
        9002,
        &uniq("reviewer"),
        "review",
        vec!["knowledge_search".into()],
        channel_id,
    )
    .await;

    let auth = AuthUser::from_parts(Some(9002), UserRole::Author, Some(tenant.clone()));
    let debate = raisfast::agent::debate::orchestrator::start_debate(
        &state,
        &auth,
        a,
        b,
        None,
        "给内容系统加导出功能".into(),
        Some(1),
    )
    .await
    .expect("start debate");

    let final_row = wait_terminal(&state.pool, debate.id, &tenant).await;
    assert_eq!(
        final_row.status, "escalated",
        "escalated; error={:?}",
        final_row.error
    );
    let ledger: Ledger = serde_json::from_value(final_row.ledger.clone()).expect("ledger");
    assert_eq!(
        ledger.escalated_ids().len(),
        1,
        "open dispute queued for verdicts"
    );
    let report = final_row.report.as_deref().unwrap_or("");
    assert!(report.contains("需你拍板"), "four-section report persisted");
    assert!(
        report.contains("倾向 B"),
        "brief recommendation lands in report"
    );
    assert!(
        !report.contains("证据冲突"),
        "value-nature dispute must not be labeled factual conflict"
    );

    // Human verdict: side B wins → finalizing → final round → concluded.
    let verdicts = vec![raisfast::agent::debate::orchestrator::VerdictInput {
        dispute_id: "D1".into(),
        verdict: "side_b".into(),
        resolution: None,
        note: Some("口径以评审员为准".into()),
    }];
    let row =
        raisfast::agent::debate::orchestrator::submit_verdicts(&state, &auth, debate.id, verdicts)
            .await
            .expect("submit verdicts");

    let mut concluded = row;
    for _ in 0..150 {
        if concluded.status == "concluded" {
            break;
        }
        concluded = debate_model::find_debate_by_id(&state.pool, debate.id, Some(&tenant))
            .await
            .expect("debate row");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(
        concluded.status, "concluded",
        "verdicts → final round → concluded"
    );
    let ledger: Ledger = serde_json::from_value(concluded.ledger).expect("ledger");
    assert_eq!(
        ledger.disputes[0].status,
        DisputeStatus::Upheld,
        "side_b verdict upheld"
    );
    assert_eq!(
        ledger.items[0].criteria[0],
        "WHEN export THE SYSTEM SHALL exclude drafts and template pages",
        "final round replaced the item list"
    );
}

/// HTTP layer: cold-start endpoint + status polling via the real handlers
/// (route registration + auth shaping; router swapped for the mock channel).
#[tokio::test]
async fn debate_http_cold_start_end_to_end() {
    let (_app, state) = test_app().await;
    let tenant = uniq("t_debate");

    let base_url = mock_llm(vec![
        serde_json::json!({
            "summary": "Export feature",
            "items": [{ "id": "REQ-1", "story": "As a user, I want export",
                        "criteria": ["WHEN export THE SYSTEM SHALL skip drafts"] }]
        }),
        serde_json::json!({
            "coverage": ["range"],
            "new_challenges": [{
                "target": "REQ-1", "type": "ambiguity", "severity": "minor",
                "evidence": "草稿口径未定义", "sources": ["REQ-1"]
            }],
            "re_visits": []
        }),
        // A rejects? no — accept would conclude; minor is auto-resolved
        // before the terminal check, so defense content is irrelevant. Keep
        // a minimal valid response (reject path also fine): auto-pass closes
        // the minor regardless.
        serde_json::json!({
            "responses": [{ "dispute_id": "D1", "stance": "reject",
                            "reasoning": "口径由原文锁定", "sources": ["原文 §2"] }],
            "revised_items": []
        }),
    ])
    .await;
    let (channel_id, state) = create_mock_channel(&state, &tenant, &base_url)
        .await
        .unwrap();
    let a = create_actor(
        &state,
        &tenant,
        9003,
        &uniq("proposer"),
        "propose",
        vec!["*".into()],
        channel_id,
    )
    .await;
    let b = create_actor(
        &state,
        &tenant,
        9003,
        &uniq("reviewer"),
        "review",
        vec!["knowledge_search".into()],
        channel_id,
    )
    .await;

    // Agents live in a custom tenant; the token carries the default tenant.
    // Round-trip the cold-start handler directly with a matching identity.
    let auth = AuthUser::from_parts(Some(9003), UserRole::Author, Some(tenant.clone()));
    let res = raisfast::agent::handler::start_cold_debate(
        auth.clone(),
        axum::extract::State(state.clone()),
        axum::Json(raisfast::agent::handler::ColdDebateReq {
            agent_a_id: a.0.to_string(),
            agent_b_id: b.0.to_string(),
            requirement: "导出功能".into(),
            max_rounds: Some(3),
        }),
    )
    .await
    .expect("cold start handler");
    let body = &res;
    assert_eq!(
        body.code, 0,
        "start_cold_debate responds ok: {}",
        body.message
    );

    // Poll via the status handler.
    let debate_id = {
        let raw = serde_json::to_value(&res).unwrap();
        raw.pointer("/data/debate/id")
            .and_then(|x| x.as_str())
            .expect("debate id in response")
            .to_string()
    };
    for _ in 0..150 {
        let res = raisfast::agent::handler::get_debate(
            auth.clone(),
            axum::extract::State(state.clone()),
            axum::extract::Path(debate_id.clone()),
        )
        .await
        .expect("get_debate handler");
        let raw = serde_json::to_value(&res).unwrap();
        let status = raw
            .pointer("/data/debate/status")
            .and_then(|x| x.as_str())
            .expect("status")
            .to_string();
        if status != "running" {
            assert_eq!(status, "consensus", "minor auto-resolved → consensus");
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("debate did not reach consensus via HTTP path");
}
