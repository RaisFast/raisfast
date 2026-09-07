//! Shared builders for flow-engine scenario tests (pure/in-memory by default;
//! DB-backed durable scenarios call `super::super::test_pool`).

use raisfast::flows::engine::{self, ExecOutcome, NodeExecutor, Snapshot};
use raisfast::flows::graph::{self, Graph};
use serde_json::{Value, json};
use std::collections::HashMap;

/// Build a `Graph` from a raw flow definition object.
pub fn def_graph(def: Value) -> Graph {
    graph::load_definition(&def).expect("valid definition")
}

/// Minimal flow-definition helper: nodes + edges wrapped in `graph`.
pub fn def_of(nodes: Value, edges: Value) -> Value {
    json!({"name": "scenario", "graph": {"nodes": nodes, "edges": edges}})
}

pub fn node(id: &str, kind: &str, config: Value) -> Value {
    json!({"id": id, "data": {"type": kind, "config": config}})
}

pub fn node_m(id: &str, kind: &str, config: Value, mods: Value) -> Value {
    json!({"id": id, "data": {"type": kind, "config": config, "modifiers": mods}})
}

pub fn edge(s: &str, h: &str, t: &str) -> Value {
    json!({"source": s, "sourceHandle": h, "target": t})
}

/// Deterministic action executor: script/egress nodes return the configured
/// per-node output (fallback: echoes the input). Lets scenario tests drive the
/// engine without network.
pub struct Deterministic {
    outputs: HashMap<String, Value>,
}

impl Deterministic {
    pub fn new() -> Self {
        Self {
            outputs: HashMap::new(),
        }
    }
    pub fn with(mut self, node_id: &str, output: Value) -> Self {
        self.outputs.insert(node_id.to_string(), output);
        self
    }
}

#[async_trait::async_trait]
impl NodeExecutor for Deterministic {
    async fn exec(
        &self,
        node: &raisfast::flows::graph::GraphNode,
        input: Value,
        _pool: &raisfast::flows::engine::Pool,
    ) -> raisfast::errors::app_error::AppResult<ExecOutcome> {
        let out = self.outputs.get(&node.id).cloned().unwrap_or(input);
        Ok(ExecOutcome {
            output: out,
            usage: None,
            latency_ms: None,
        })
    }
}

/// Seed a snapshot with the given start-namespace inputs.
pub fn seeded_snapshot(start_inputs: Value) -> Snapshot {
    let mut snap = Snapshot::new();
    let mut ns = HashMap::new();
    if let Some(obj) = start_inputs.as_object() {
        for (k, v) in obj {
            ns.insert(k.clone(), v.clone());
        }
    }
    snap.pool.insert("start".to_string(), ns);
    snap
}

/// Run an in-memory scenario to completion and return the snapshot.
pub async fn run_pure(graph: &Graph, snap: &mut Snapshot, exec: &dyn NodeExecutor) {
    engine::run(graph, snap, exec).await.expect("run ok");
}
