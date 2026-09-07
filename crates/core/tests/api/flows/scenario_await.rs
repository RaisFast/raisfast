//! Await/resume (HITL) scenarios against the DB-backed coordinator.

use super::helpers::*;
use raisfast::flows::engine::Snapshot;
use raisfast::flows::model::{self, Flow, FlowInstance, FlowVersion};
use raisfast::flows::run;
use raisfast::utils::tz::now_utc;
use serde_json::json;

async fn seed_waiting_instance(pool: &raisfast::db::Pool) -> i64 {
    let now = now_utc();
    let flow_id = raisfast::utils::id::new_snowflake_id();
    model::insert_flow(
        pool,
        &Flow {
            id: flow_id,
            tenant_id: "default".into(),
            name: format!("await-{}", raisfast::utils::id::new_id()),
            description: None,
            enabled: true,
            current_version: None,
            extra: None,
            created_at: now,
            updated_at: now,
        },
    )
    .await
    .unwrap();
    let def = def_of(
        json!([
            node("start", "start", json!({})),
            node("gate", "await", json!({"kind": "human"})),
            node(
                "end",
                "end",
                json!({
                    "outputs": [{"key": "ok", "value": {"ref": ["gate", "resume", "approved"]}}]
                })
            )
        ]),
        json!([edge("start", "out", "gate"), edge("gate", "out", "end")]),
    );
    let vid = raisfast::utils::id::new_snowflake_id();
    model::insert_flow_version(
        pool,
        &FlowVersion {
            id: vid,
            flow_id,
            version_number: 1,
            definition: def,
            created_by: None,
            created_at: now,
        },
    )
    .await
    .unwrap();
    model::set_flow_current_version(pool, flow_id, vid)
        .await
        .unwrap();

    let iid = raisfast::utils::id::new_snowflake_id();
    model::insert_flow_instance(
        pool,
        &FlowInstance {
            id: iid,
            tenant_id: "default".into(),
            flow_id,
            flow_version_id: vid,
            status: "running".into(),
            has_exceptions: false,
            trigger_kind: "api".into(),
            trigger_payload: Some(json!({})),
            inputs_summary: None,
            outputs: None,
            error: None,
            started_by: None,
            started_at: Some(now),
            finished_at: None,
            waiting_kind: None,
            waiting_needed: None,
            waiting_received: 0,
            resume_until: None,
            created_at: now,
        },
    )
    .await
    .unwrap();
    iid.0
}

#[tokio::test]
async fn await_parks_instance_then_resume_approves() {
    let pool = super::super::test_pool().await;
    let det = Deterministic::new();
    let iid = seed_waiting_instance(&pool).await;
    let iid = raisfast::types::snowflake_id::SnowflakeId(iid);

    run::execute_instance(&pool, iid, &det).await.unwrap();
    let inst = model::find_instance_by_id(&pool, iid).await.unwrap();
    assert_eq!(inst.status, "waiting", "parks on await: {inst:?}");
    let snap: Snapshot =
        serde_json::from_value(model::find_snapshot(&pool, iid).await.unwrap().unwrap()).unwrap();
    assert!(snap.waiting_nodes.contains(&"gate".to_string()));

    // Resume with approval.
    run::resume_instance(&pool, iid, Some(json!({"approved": true})))
        .await
        .unwrap();
    let done = model::find_instance_by_id(&pool, iid).await.unwrap();
    assert_eq!(done.status, "success");
    assert_eq!(done.outputs.unwrap()["ok"], true);
}
