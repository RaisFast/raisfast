#![cfg(feature = "db-postgres")]
//! DB round-trip for the debate layer (`dev-docs/agent/multi-agent-debate.md` M-A1).
//!
//! No LLM involved — validates `ai_debates` crud + ledger JSON column and the
//! child-session (`ai_sessions.parent_id`) isolation primitive on PostgreSQL.
//!
//! ```bash
//! RAISFAST_DB=postgres RAISFAST_TEST_DB_URL=postgres://postgres:postgres@localhost:5432/raisfast_test \
//!   cargo test -p raisfast --test ai_debate --no-default-features \
//!     --features "db-postgres plugin-js plugin-rhai search-tantivy payment-all tunnel mcp cron-system integration-stream integration-imap"
//! ```

use raisfast::agent::debate::ledger::{Challenge, Dispute, Ledger, RequirementItem, Severity};
use raisfast::agent::models::{ai_debate, ai_session};
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::time::{SystemTime, UNIX_EPOCH};

use raisfast::types::snowflake_id::SnowflakeId;

fn test_pool() -> PgPool {
    let url = std::env::var("RAISFAST_TEST_DB_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/raisfast_test".into());
    PgPoolOptions::new()
        .max_connections(2)
        .connect_lazy(&url)
        .expect("test pool")
}

fn tenant() -> String {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("t_debate_{n}")
}

fn seed_ledger() -> Ledger {
    Ledger::new(vec![RequirementItem {
        id: "REQ-1".into(),
        story: "As a user, I want export".into(),
        criteria: vec!["WHEN export THE SYSTEM SHALL skip drafts".into()],
        change_note: None,
    }])
}

#[tokio::test]
async fn debate_crud_roundtrip() {
    let pool = test_pool();
    let tenant_id = tenant();

    let ledger = seed_ledger();
    let debate = ai_debate::create_debate(
        &pool,
        Some(&tenant_id),
        SnowflakeId(101),
        SnowflakeId(201),
        SnowflakeId(202),
        None,
        "Build an export feature",
        Some(serde_json::json!({ "max_rounds": 2 })),
        serde_json::to_value(&ledger).unwrap(),
    )
    .await
    .expect("create debate");

    assert_eq!(debate.status, "running");
    assert_eq!(debate.rounds_done, 0);
    assert_eq!(debate.agent_a_id, SnowflakeId(201));
    assert_eq!(debate.origin_session_id, None, "cold start has no origin");
    let decoded: Ledger = serde_json::from_value(debate.ledger).expect("ledger decodes");
    assert_eq!(decoded.items.len(), 1, "ledger JSON round-trips");

    // Tenant isolation: another tenant cannot see the row.
    assert!(
        ai_debate::find_debate_by_id(&pool, debate.id, Some("other_tenant"))
            .await
            .is_err()
    );

    // Ledger + round counter advance.
    let mut ledger2 = decoded;
    ledger2.disputes.push(Dispute {
        id: "D1".into(),
        title: "drafts ambiguity".into(),
        target: "REQ-1".into(),
        kind: raisfast::agent::debate::ledger::ChallengeKind::Ambiguity,
        nature: Some(raisfast::agent::debate::ledger::Nature::Ambiguity),
        severity: Severity::Major,
        status: raisfast::agent::debate::ledger::DisputeStatus::Open,
        resolved_by: None,
        rule_id: None,
        challenges: vec![Challenge {
            round: 1,
            argument: "drafts undefined".into(),
            sources: vec!["REQ-1".into()],
            nature: None,
            proposed_resolution: None,
        }],
        responses: vec![],
        withdraw_round: None,
        brief: None,
        verdict: None,
    });
    ai_debate::update_ledger(
        &pool,
        debate.id,
        Some(&tenant_id),
        serde_json::to_value(&ledger2).unwrap(),
        1,
    )
    .await
    .expect("update ledger");

    // Status transitions + report + heartbeat + failure.
    ai_debate::set_debate_status(&pool, debate.id, Some(&tenant_id), "escalated")
        .await
        .expect("escalate");
    ai_debate::update_report(&pool, debate.id, Some(&tenant_id), "# report\n...")
        .await
        .expect("report");
    ai_debate::update_heartbeat(&pool, debate.id, Some(&tenant_id))
        .await
        .expect("heartbeat");
    ai_debate::set_debate_failed(&pool, debate.id, Some(&tenant_id), "provider down")
        .await
        .expect("fail");

    let final_row = ai_debate::find_debate_by_id(&pool, debate.id, Some(&tenant_id))
        .await
        .expect("final");
    assert_eq!(
        final_row.status, "failed",
        "failure overrides earlier status"
    );
    assert_eq!(final_row.rounds_done, 1);
    assert_eq!(final_row.report.as_deref(), Some("# report\n..."));
    assert_eq!(final_row.error.as_deref(), Some("provider down"));
    let final_ledger: Ledger = serde_json::from_value(final_row.ledger).unwrap();
    assert_eq!(final_ledger.disputes.len(), 1);
    assert_eq!(
        final_ledger.disputes[0].challenges[0].argument,
        "drafts undefined"
    );
}

#[tokio::test]
async fn child_session_isolation_and_meta() {
    let pool = test_pool();
    let tenant_id = tenant();

    let agent = raisfast::agent::models::ai_agent::create_agent(
        &pool,
        Some(&tenant_id),
        None,
        "debater",
        "test prompt",
        "openai_compat",
        "test-model",
        None,
        None,
        vec![],
        true,
        None,
    )
    .await
    .expect("create agent");

    // Origin session (top-level: parent_id NULL).
    let origin = ai_session::create_session(&pool, Some(&tenant_id), agent.id, agent.id, "origin")
        .await
        .expect("origin session");
    assert_eq!(origin.parent_id, None, "top-level session has no parent");

    // Child session with parent + debate meta.
    let meta = serde_json::json!({ "debate_id": SnowflakeId(999), "role": "reviewer" });
    let child = ai_session::create_child_session(
        &pool,
        Some(&tenant_id),
        agent.id,
        agent.id,
        origin.id,
        "review (@reviewer subagent)",
        meta,
    )
    .await
    .expect("child session");
    assert_eq!(child.parent_id, Some(origin.id), "parent_id round-trips");
    assert_eq!(
        child
            .meta
            .as_ref()
            .and_then(|m| m.get("role"))
            .and_then(|r| r.as_str()),
        Some("reviewer"),
        "debate meta round-trips"
    );

    // Isolation: child transcript is empty — nothing from origin leaks in.
    let child_msgs = raisfast::agent::models::ai_message::list_messages_after(
        &pool,
        child.id,
        Some(&tenant_id),
        None,
        100,
    )
    .await
    .expect("child messages");
    assert!(
        child_msgs.is_empty(),
        "child session starts with empty history"
    );

    // Cross-tenant read of the child is refused.
    assert!(
        ai_session::find_session_by_id(&pool, child.id, Some("other_tenant"))
            .await
            .is_err()
    );
}
