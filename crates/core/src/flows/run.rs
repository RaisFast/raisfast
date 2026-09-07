//! Instance coordinator: DB-backed execution with per-node durable snapshots
//! (dev-docs/workflow db-schema.md §5, reliability A.3).
//!
//! Pipeline: load instance+version → build/seed snapshot → engine `run_persisted`
//! with a DB [`Persist`] (claim + every node completion → upsert snapshot) →
//! finalize instance status/outputs. Completed nodes never re-run on resume
//! (snapshot carries their `success` state).

use async_trait::async_trait;

use crate::errors::app_error::{AppError, AppResult};
use crate::integration::IntegrationPlane;
use crate::plugins::PluginManager;
use crate::types::snowflake_id::SnowflakeId;
use std::sync::Arc;

use super::exec::FlowsExec;

use super::engine::{self, NodeExecutor, Persist, S_FAILED, S_SUCCESS, S_WAITING, Snapshot};
use super::graph::{self, Graph};
use super::model;

/// Persist the snapshot to `flow_instance_snapshot` (1:1 upsert).
struct DbPersist {
    pool: crate::db::Pool,
    instance_id: SnowflakeId,
}

#[async_trait]
impl Persist for DbPersist {
    async fn persist(&self, snap: &Snapshot) -> AppResult<()> {
        let value = serde_json::to_value(snap)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("snapshot serialize: {e}")))?;
        model::upsert_snapshot(&self.pool, self.instance_id, &value).await
    }
}

/// Load a graph from the instance's locked flow version.
async fn load_graph_for_instance(
    pool: &crate::db::Pool,
    inst: &model::FlowInstance,
) -> AppResult<Graph> {
    let version = model::find_version_by_id(pool, inst.flow_version_id)
        .await?
        .ok_or_else(|| AppError::not_found("flow_version"))?;
    graph::load_definition(&version.definition)
}

/// Seed the start-namespace pool from the instance trigger inputs.
fn seed_pool(
    inputs: Option<&serde_json::Value>,
) -> std::collections::HashMap<String, serde_json::Value> {
    let mut ns = std::collections::HashMap::new();
    if let Some(obj) = inputs.and_then(|v| v.as_object()) {
        for (k, v) in obj {
            ns.insert(k.clone(), v.clone());
        }
    }
    ns
}

pub async fn run_flow_latest(
    pool: &crate::db::Pool,
    plane: Option<Arc<IntegrationPlane>>,
    plugins: Option<Arc<PluginManager>>,
    flow_id: SnowflakeId,
    inputs: Option<serde_json::Value>,
    trigger: &str,
) -> AppResult<model::FlowInstance> {
    let flow = model::find_flow_by_id(pool, flow_id).await?;
    let version = model::latest_version(pool, flow_id)
        .await?
        .ok_or_else(|| AppError::not_found("flow_version"))?;
    graph::load_definition(&version.definition)?;

    let instance_id = crate::utils::id::new_snowflake_id();
    let now = crate::utils::tz::now_utc();
    let instance = model::FlowInstance {
        id: instance_id,
        tenant_id: flow.tenant_id.clone(),
        flow_id,
        flow_version_id: version.id,
        status: "running".into(),
        has_exceptions: false,
        trigger_kind: trigger.to_string(),
        trigger_payload: inputs,
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
    };
    model::insert_flow_instance(pool, &instance).await?;

    let exec = FlowsExec {
        plane,
        plugins,
        llm: None,
    };
    execute_instance(pool, instance_id, &exec).await?;
    model::find_instance_by_id(pool, instance_id).await
}

/// Run an ad-hoc definition (current canvas / draft) against a flow without
/// publishing a version. The instance references the latest published version
/// id; execution + node-runs reflect the provided definition. trigger='test'.
pub async fn run_definition_latest(
    pool: &crate::db::Pool,
    plane: Option<Arc<IntegrationPlane>>,
    plugins: Option<Arc<PluginManager>>,
    flow_id: SnowflakeId,
    definition: serde_json::Value,
    inputs: Option<serde_json::Value>,
) -> AppResult<model::FlowInstance> {
    graph::load_definition(&definition)?;
    let flow = model::find_flow_by_id(pool, flow_id).await?;
    let version = model::latest_version(pool, flow_id)
        .await?
        .ok_or_else(|| AppError::not_found("flow_version"))?;
    let instance_id = crate::utils::id::new_snowflake_id();
    let now = crate::utils::tz::now_utc();
    let instance = model::FlowInstance {
        id: instance_id,
        tenant_id: flow.tenant_id.clone(),
        flow_id,
        flow_version_id: version.id,
        status: "running".into(),
        has_exceptions: false,
        trigger_kind: "test".into(),
        trigger_payload: inputs,
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
    };
    model::insert_flow_instance(pool, &instance).await?;

    // Seed a fresh snapshot from the trigger payload + start params, then run
    // the engine against the provided (unpublished) definition.
    let graph = graph::load_definition(&definition)?;
    let mut ns = seed_pool(instance.trigger_payload.as_ref());
    if let Some(start_node) = graph.nodes.get(&graph.start) {
        let cfg: super::nodes::StartConfig = serde_json::from_value(start_node.data.config.clone())
            .map_err(|e| AppError::BadRequest(format!("start config: {e}")))?;
        super::params::apply(&cfg.params, &mut ns)?;
    }
    let mut snap = Snapshot::new();
    snap.pool.insert(graph.start.clone(), ns);
    let persist = DbPersist {
        pool: pool.clone(),
        instance_id,
    };
    let exec = FlowsExec {
        plane,
        plugins,
        llm: None,
    };
    engine::run_persisted(&graph, &mut snap, &exec, &persist).await?;
    record_node_runs(pool, instance_id, &graph, &snap).await?;

    if snap.status == S_WAITING {
        model::update_instance_status(pool, instance_id, "waiting", false, None, None).await?;
    } else if snap.status == S_SUCCESS {
        model::finalize_instance(
            pool,
            instance_id,
            S_SUCCESS,
            false,
            snap.outputs.as_ref(),
            None,
        )
        .await?;
    } else if snap.status == S_FAILED {
        model::finalize_instance(
            pool,
            instance_id,
            S_FAILED,
            false,
            None,
            snap.error.as_ref(),
        )
        .await?;
    }
    model::find_instance_by_id(pool, instance_id).await
}

/// Run an instance to completion (idempotent: terminals return early; completed
/// nodes in a persisted snapshot are skipped).
///
/// # Errors
///
/// `AppError` on load/persist failures; node failures are reflected in the
/// instance status (`failed`), not bubbled up.
pub async fn execute_instance(
    pool: &crate::db::Pool,
    instance_id: SnowflakeId,
    exec: &dyn NodeExecutor,
) -> AppResult<()> {
    let inst = model::find_instance_by_id(pool, instance_id).await?;
    if matches!(inst.status.as_str(), S_SUCCESS | S_FAILED | "canceled") {
        return Ok(()); // terminal
    }
    let graph = load_graph_for_instance(pool, &inst).await?;

    // Load persisted snapshot, or seed a fresh one.
    let mut snap: Snapshot = match model::find_snapshot(pool, instance_id).await? {
        Some(v) => serde_json::from_value(v)
            .map_err(|e| AppError::Internal(anyhow::anyhow!("snapshot parse: {e}")))?,
        None => {
            let mut ns = seed_pool(inst.trigger_payload.as_ref());
            if let Some(start_node) = graph.nodes.get(&graph.start) {
                let cfg: super::nodes::StartConfig =
                    serde_json::from_value(start_node.data.config.clone())
                        .map_err(|e| AppError::BadRequest(format!("start config: {e}")))?;
                super::params::apply(&cfg.params, &mut ns)?;
            }
            let mut s = Snapshot::new();
            s.pool.insert(graph.start.clone(), ns);
            s
        }
    };

    let persist = DbPersist {
        pool: pool.clone(),
        instance_id,
    };
    engine::run_persisted(&graph, &mut snap, exec, &persist).await?;

    record_node_runs(pool, instance_id, &graph, &snap).await?;

    if snap.status == S_WAITING {
        // Parked on an await node: keep the snapshot; a resume call continues.
        model::update_instance_status(pool, instance_id, "waiting", false, None, None).await?;
        return Ok(());
    }
    if snap.status == S_SUCCESS {
        model::finalize_instance(
            pool,
            instance_id,
            S_SUCCESS,
            false,
            snap.outputs.as_ref(),
            None,
        )
        .await?;
        // Keep the snapshot? Terminal success can drop it; keep for replay (P2).
    } else if snap.status == S_FAILED {
        model::finalize_instance(
            pool,
            instance_id,
            S_FAILED,
            false,
            None,
            snap.error.as_ref(),
        )
        .await?;
    }
    Ok(())
}

/// Mirror terminal node states onto `flow_node_run` (upsert per node). Ran for
/// every pass (initial + after resume), so a node parked as `waiting` gets its
/// row flipped to `success` once resumed.
async fn record_node_runs(
    pool: &crate::db::Pool,
    instance_id: SnowflakeId,
    graph: &Graph,
    snap: &Snapshot,
) -> AppResult<()> {
    let mut ordered: Vec<&String> = snap.exec_order.iter().collect();
    for id in snap.node_states.keys() {
        if !snap.exec_order.contains(id) {
            ordered.push(id);
        }
    }
    for node_id in ordered {
        let Some(st) = snap.node_states.get(node_id) else {
            continue;
        };
        let status = st.status.as_str();
        if !matches!(
            status,
            "success" | "failed" | "skipped" | "waiting" | "error_output"
        ) {
            continue;
        }
        let Some(node) = graph.nodes.get(node_id) else {
            continue;
        };
        let error = st.error.as_ref().map(|v| {
            if let Some(s) = v.as_str() {
                s.to_string()
            } else {
                v.to_string()
            }
        });
        let input = st.input.as_ref().map(|v| v.to_string());
        let output = st.output.as_ref().map(|v| v.to_string());
        let usage = st.usage.as_ref().map(|v| v.to_string());
        model::record_node_run(
            pool,
            instance_id,
            node_id,
            node.data.kind.as_str(),
            status,
            st.attempt,
            input.as_deref(),
            output.as_deref(),
            error.as_deref(),
            usage.as_deref(),
            st.latency_ms,
        )
        .await?;
    }
    Ok(())
}

/// Complete a parked `await` node with `payload` and continue the instance.
///
/// # Errors
///
/// `BadRequest` when the instance is not waiting on any node.
pub async fn resume_instance(
    pool: &crate::db::Pool,
    instance_id: SnowflakeId,
    payload: Option<serde_json::Value>,
) -> AppResult<()> {
    let inst = model::find_instance_by_id(pool, instance_id).await?;
    if inst.status != "waiting" {
        return Err(AppError::BadRequest(format!(
            "实例状态不是 waiting: {}",
            inst.status
        )));
    }
    let Some(value) = model::find_snapshot(pool, instance_id).await? else {
        return Err(AppError::BadRequest("实例无快照".into()));
    };
    let mut snap: Snapshot = serde_json::from_value(value)
        .map_err(|e| AppError::Internal(anyhow::anyhow!("snapshot parse: {e}")))?;
    engine::resume_snapshot(&mut snap, payload)?;
    let persist = DbPersist {
        pool: pool.clone(),
        instance_id,
    };
    persist.persist(&snap).await?;
    execute_instance(pool, instance_id, &NoopExec).await?;
    Ok(())
}

struct NoopExec;
#[async_trait]
impl NodeExecutor for NoopExec {
    async fn exec(
        &self,
        _node: &super::graph::GraphNode,
        _input: serde_json::Value,
        _pool: &engine::Pool,
    ) -> AppResult<super::engine::ExecOutcome> {
        Ok(super::engine::ExecOutcome {
            output: serde_json::Value::Null,
            usage: None,
            latency_ms: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    struct StubExec;
    #[async_trait]
    impl NodeExecutor for StubExec {
        async fn exec(
            &self,
            _node: &crate::flows::graph::GraphNode,
            _input: Value,
            _pool: &engine::Pool,
        ) -> AppResult<engine::ExecOutcome> {
            Ok(engine::ExecOutcome {
                output: json!({"stub": true}),
                usage: None,
                latency_ms: None,
            })
        }
    }

    fn now() -> crate::utils::tz::Timestamp {
        crate::utils::tz::now_utc()
    }

    async fn seed_linear_flow(pool: &crate::db::Pool) -> SnowflakeId {
        let flow_id = crate::utils::id::new_snowflake_id();
        let flow = model::Flow {
            id: flow_id,
            tenant_id: "default".into(),
            name: "durable".into(),
            description: None,
            enabled: true,
            current_version: None,
            extra: None,
            created_at: now(),
            updated_at: now(),
        };
        model::insert_flow(pool, &flow).await.unwrap();

        let def = json!({
            "name": "durable",
            "graph": {
                "nodes": [
                    {"id": "start", "data": {"type": "start", "config": {}}},
                    {"id": "e1", "data": {"type": "egress", "config": {"client_key": "k", "op": "o"}}},
                    {"id": "end", "data": {"type": "end", "config": {"outputs": [{"key": "v", "value": {"ref": ["e1"]}}]}}}
                ],
                "edges": [
                    {"source": "start", "target": "e1"},
                    {"source": "e1", "target": "end"}
                ]
            }
        });
        let version = model::FlowVersion {
            id: crate::utils::id::new_snowflake_id(),
            flow_id,
            version_number: 1,
            definition: def,
            created_by: None,
            created_at: now(),
        };
        model::insert_flow_version(pool, &version).await.unwrap();
        model::set_flow_current_version(pool, flow_id, version.id)
            .await
            .unwrap();

        let instance_id = crate::utils::id::new_snowflake_id();
        let inst = model::FlowInstance {
            id: instance_id,
            tenant_id: "default".into(),
            flow_id,
            flow_version_id: version.id,
            status: "running".into(),
            has_exceptions: false,
            trigger_kind: "api".into(),
            trigger_payload: Some(json!({"msg": "hi"})),
            inputs_summary: None,
            outputs: None,
            error: None,
            started_by: None,
            started_at: Some(now()),
            finished_at: None,
            waiting_kind: None,
            waiting_needed: None,
            waiting_received: 0,
            resume_until: None,
            created_at: now(),
        };
        model::insert_flow_instance(pool, &inst).await.unwrap();
        instance_id
    }

    #[tokio::test]
    async fn durable_run_finishes_and_resume_skips_completed() {
        let pool = crate::test_pool!();
        let instance_id = seed_linear_flow(&pool).await;

        // First pass to completion.
        execute_instance(&pool, instance_id, &StubExec)
            .await
            .unwrap();
        let inst = model::find_instance_by_id(&pool, instance_id)
            .await
            .unwrap();
        assert_eq!(inst.status, "success");
        assert_eq!(inst.outputs.unwrap()["v"]["stub"], true);
        let snap_val = model::find_snapshot(&pool, instance_id)
            .await
            .unwrap()
            .unwrap();
        let snap: Snapshot = serde_json::from_value(snap_val).unwrap();
        assert_eq!(snap.node_states["start"].status, engine::N_SUCCESS);
        assert_eq!(snap.node_states["e1"].status, engine::N_SUCCESS);

        // Terminal → no-op on second call.
        execute_instance(&pool, instance_id, &StubExec)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn resume_does_not_rerun_completed_start() {
        let pool = crate::test_pool!();
        let instance_id = seed_linear_flow(&pool).await;

        // Simulate crash after start completed: snapshot with start success,
        // e1 undecided/untouched.
        let mut snap = Snapshot::new();
        snap.node_states.insert(
            "start".into(),
            engine::NodeState {
                input: None,
                status: engine::N_SUCCESS.into(),
                output: Some(Value::Null),
                error: None,
                attempt: 1,
                usage: None,
                latency_ms: None,
            },
        );
        let snap_json = serde_json::to_value(&snap).unwrap();
        model::upsert_snapshot(&pool, instance_id, &snap_json)
            .await
            .unwrap();

        execute_instance(&pool, instance_id, &StubExec)
            .await
            .unwrap();
        let inst = model::find_instance_by_id(&pool, instance_id)
            .await
            .unwrap();
        assert_eq!(inst.status, "success");
        let snap_val = model::find_snapshot(&pool, instance_id)
            .await
            .unwrap()
            .unwrap();
        let snap2: Snapshot = serde_json::from_value(snap_val).unwrap();
        // start not re-run (attempt stays 1), e1 ran once.
        assert_eq!(snap2.node_states["start"].attempt, 1);
        assert_eq!(snap2.node_states["e1"].status, engine::N_SUCCESS);
    }
}
