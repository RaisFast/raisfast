//! Resilience scenarios: retry, continue_on_error, fail-stop, durable resume.

use super::helpers::*;
use raisfast::errors::app_error::AppError;
use raisfast::flows::engine::{N_FAILED, N_SUCCESS, S_SUCCESS};
use serde_json::{Value, json};

use std::sync::atomic::{AtomicUsize, Ordering};

/// Executor failing the first `fail_until` calls.
struct Flaky {
    calls: AtomicUsize,
    fail_until: usize,
}
#[async_trait::async_trait]
impl raisfast::flows::engine::NodeExecutor for Flaky {
    async fn exec(
        &self,
        _n: &raisfast::flows::graph::GraphNode,
        _i: Value,
        _pool: &raisfast::flows::engine::Pool,
    ) -> raisfast::errors::app_error::AppResult<raisfast::flows::engine::ExecOutcome> {
        let c = self.calls.fetch_add(1, Ordering::SeqCst);
        if c < self.fail_until {
            // Internal (retryable): BadRequest now short-circuits retries.
            Err(AppError::Internal(anyhow::anyhow!("boom")))
        } else {
            Ok(raisfast::flows::engine::ExecOutcome {
                output: json!({"ok": true}),
                usage: None,
                latency_ms: None,
            })
        }
    }
}

fn linear_through(e1_cfg: Value, e1_mods: Value) -> raisfast::flows::graph::Graph {
    def_graph(def_of(
        json!([
            node("start", "start", json!({})),
            node_m("e1", "egress", e1_cfg, e1_mods),
            node("end", "end", json!({"outputs": []}))
        ]),
        json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
    ))
}

#[tokio::test]
async fn retry_recovers_after_transient_failure() {
    let g = linear_through(
        json!({"client_key": "k", "op": "o"}),
        json!({"retry": {"attempts": 3}}),
    );
    let mut snap = seeded_snapshot(json!({}));
    let flaky = Flaky {
        calls: AtomicUsize::new(0),
        fail_until: 1,
    };
    run_pure(&g, &mut snap, &flaky).await;
    assert_eq!(snap.status, S_SUCCESS);
    assert_eq!(snap.node_states["e1"].status, N_SUCCESS);
    assert_eq!(snap.node_states["e1"].attempt, 2, "fail then retry");
}

#[tokio::test]
async fn continue_on_error_lets_flow_finish() {
    let g = linear_through(
        json!({"client_key": "k", "op": "o"}),
        json!({"continue_on_error": true}),
    );
    let mut snap = seeded_snapshot(json!({}));
    let always = Flaky {
        calls: AtomicUsize::new(0),
        fail_until: usize::MAX,
    };
    run_pure(&g, &mut snap, &always).await;
    assert_eq!(snap.status, S_SUCCESS);
    assert_eq!(snap.node_states["e1"].status, N_FAILED);
    assert_eq!(snap.node_states["end"].status, N_SUCCESS);
}

#[tokio::test]
async fn unhandled_failure_stops_flow() {
    let g = linear_through(json!({"client_key": "k", "op": "o"}), json!({}));
    let mut snap = seeded_snapshot(json!({}));
    let always = Flaky {
        calls: AtomicUsize::new(0),
        fail_until: usize::MAX,
    };
    run_pure(&g, &mut snap, &always).await;
    assert_eq!(snap.status, raisfast::flows::engine::S_FAILED);
}

#[tokio::test]
async fn durable_resume_does_not_rerun_completed_nodes() {
    let pool = super::super::test_pool().await;
    use raisfast::flows::model::{self, Flow, FlowInstance, FlowVersion};
    use raisfast::flows::run;
    use raisfast::utils::tz::now_utc;

    let now = now_utc();
    let flow_id = raisfast::utils::id::new_snowflake_id();
    model::insert_flow(
        &pool,
        &Flow {
            id: flow_id,
            tenant_id: "default".into(),
            name: format!("dup-{}", raisfast::utils::id::new_id()),
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
            node("e1", "egress", json!({"client_key": "k", "op": "o"})),
            node("end", "end", json!({"outputs": []}))
        ]),
        json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
    );
    let vid = raisfast::utils::id::new_snowflake_id();
    model::insert_flow_version(
        &pool,
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

    // Crash simulation: snapshot already has `start` success (completed).
    let mut snap = raisfast::flows::engine::Snapshot::new();
    snap.node_states.insert(
        "start".into(),
        raisfast::flows::engine::NodeState {
            input: None,
            status: N_SUCCESS.into(),
            output: Some(json!(null)),
            error: None,
            attempt: 1,
            usage: None,
            latency_ms: None,
        },
    );
    model::upsert_snapshot(&pool, flow_id, &serde_json::to_value(&snap).unwrap())
        .await
        .unwrap();

    let iid = raisfast::utils::id::new_snowflake_id();
    model::insert_flow_instance(
        &pool,
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

    let det = Deterministic::new();
    run::execute_instance(&pool, iid, &det).await.unwrap();
    let done = model::find_instance_by_id(&pool, iid).await.unwrap();
    assert_eq!(done.status, "success");
    let s2: raisfast::flows::engine::Snapshot =
        serde_json::from_value(model::find_snapshot(&pool, iid).await.unwrap().unwrap()).unwrap();
    // start completed before the "crash" and must not re-run.
    assert_eq!(s2.node_states["start"].attempt, 1);
    assert_eq!(s2.node_states["e1"].status, N_SUCCESS);
}
