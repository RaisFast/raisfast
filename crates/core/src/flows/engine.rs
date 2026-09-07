//! Serial execution core (execution-engine.md, contracts C4-adjacent).
//!
//! P1.2 skeleton: ready-queue + single-point state writes + Join/skip + whole
//! snapshot. Internal semantics for start/end/branch; `script`/`egress` run via
//! an injected [`NodeExecutor`] (wired in P1.5/1.6); await/resume in P2.
//!
//! Execution is serial (one node at a time). Fan-out runs every target that has
//! data; join waits for ALL incoming edges to be decided with at least one taken.

use std::collections::{HashMap, VecDeque};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::errors::app_error::{AppError, AppResult};

use super::graph::{Edge, Graph, GraphNode};
use super::nodes::{self, BranchConfig, EndConfig};

/// variable pool: ns -> name -> value (child fields live inside object values).
pub type Pool = HashMap<String, HashMap<String, Value>>;

pub const S_RUNNING: &str = "running";
pub const S_WAITING: &str = "waiting";
pub const S_SUCCESS: &str = "success";
pub const S_FAILED: &str = "failed";
pub const S_SKIPPED: &str = "skipped";

pub const N_SUCCESS: &str = "success";
pub const N_SKIPPED: &str = "skipped";
pub const N_IN_PROGRESS: &str = "in_progress";
pub const N_WAITING: &str = "waiting";
pub const N_FAILED: &str = "failed";
pub const N_ERROR_OUTPUT: &str = "error_output";

/// One node's per-attempt result (idempotency: success/skipped/error_output
/// never re-run).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NodeState {
    pub status: String,
    #[serde(default)]
    pub input: Option<Value>,
    #[serde(default)]
    pub output: Option<Value>,
    pub error: Option<Value>,
    #[serde(default)]
    pub attempt: i64,
    /// LLM-style usage payload ({"prompt_tokens",...}) for billing; set by the
    /// executor via `ExecOutcome.usage`.
    #[serde(default)]
    pub usage: Option<Value>,
    #[serde(default)]
    pub latency_ms: Option<i64>,
}

/// `modifiers.retry` config.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RetryModifier {
    #[serde(default)]
    pub attempts: Option<i64>,
    #[allow(dead_code)]
    #[serde(default)]
    pub backoff: Option<String>,
}

/// Orthogonal node modifiers (contracts C1.4). Retry is attempted in-process;
/// `continue_on_error` fails the node but lets the run pass through;
/// `on_error_strategy` converts the failure instead (llm-node.md §5):
/// `fail` (default) | `default_value` (write `default_outputs`, take out
/// edges) | `error_output` (write `{"error"}`, take error_out edges).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct NodeModifiers {
    #[serde(default)]
    pub retry: Option<RetryModifier>,
    #[serde(default)]
    pub continue_on_error: bool,
    #[serde(default)]
    pub on_error_strategy: Option<String>,
    #[serde(default)]
    pub default_outputs: Option<Value>,
}

/// Edge verdict for readiness (join = all decided; skip = all skipped).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum EdgeMark {
    Taken,
    Skipped,
}

/// Whole runnable snapshot (durable; one row per instance).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Snapshot {
    pub pool: Pool,
    pub node_states: HashMap<String, NodeState>,
    /// keyed `source|handle->target`.
    pub edge_marks: HashMap<String, EdgeMark>,
    pub status: String,
    #[serde(default)]
    pub error: Option<Value>,
    #[serde(default)]
    pub outputs: Option<Value>,
    /// Nodes parked on `await` (resume completes the head of this list).
    #[serde(default)]
    pub waiting_nodes: Vec<String>,
    /// Node ids in the order this run (re)executed them — observability feed
    /// for `flow_node_run`; not used for scheduling decisions.
    #[serde(default)]
    pub exec_order: Vec<String>,
}

fn edge_key(e: &Edge) -> String {
    format!("{}|{}->{}", e.source, e.source_handle, e.target)
}

impl Snapshot {
    pub fn new() -> Self {
        Self {
            pool: HashMap::new(),
            node_states: HashMap::new(),
            edge_marks: HashMap::new(),
            status: S_RUNNING.to_string(),
            error: None,
            outputs: None,
            waiting_nodes: Vec::new(),
            exec_order: Vec::new(),
        }
    }
}

/// Complete the head waiting node with `payload` and resume the run.
pub fn resume_snapshot(snap: &mut Snapshot, payload: Option<Value>) -> AppResult<()> {
    let Some(node) = snap.waiting_nodes.first().cloned() else {
        return Err(AppError::BadRequest("实例没有等待中的节点".into()));
    };
    snap.waiting_nodes.retain(|n| *n != node);
    let payload = payload.unwrap_or(Value::Null);
    let st = snap.node_states.entry(node.clone()).or_default();
    st.status = N_SUCCESS.to_string();
    st.output = Some(payload.clone());
    st.attempt += 1;
    let ns = snap.pool.entry(node.clone()).or_default();
    ns.insert("resume".to_string(), payload);
    snap.status = S_RUNNING.to_string();
    Ok(())
}

/// What an executable node produced.
#[derive(Debug)]
pub struct ExecOutcome {
    pub output: Value,
    /// Billing usage (llm tokens); persisted to `NodeState.usage` →
    /// `flow_node_run.usage_json`.
    pub usage: Option<Value>,
    pub latency_ms: Option<i64>,
}

/// Async executor for action nodes (`script`/`egress`/`llm`). The variable
/// pool is passed for template-driven nodes (`llm` reads `{{#…#}}` refs
/// directly instead of a mapped input).
#[async_trait]
pub trait NodeExecutor: Send + Sync {
    async fn exec(&self, node: &GraphNode, input: Value, pool: &Pool) -> AppResult<ExecOutcome>;
}

/// Durability hook: persist the snapshot after each claim / node completion.
#[async_trait]
pub trait Persist: Send + Sync {
    async fn persist(&self, snap: &Snapshot) -> AppResult<()>;
}

/// No-op persistence (pure in-memory runs / tests).
pub struct NoopPersist;
#[async_trait]
impl Persist for NoopPersist {
    async fn persist(&self, _snap: &Snapshot) -> AppResult<()> {
        Ok(())
    }
}

/// Run one full pass of the graph over the snapshot (serial, in-memory).
pub async fn run(graph: &Graph, snap: &mut Snapshot, exec: &dyn NodeExecutor) -> AppResult<()> {
    run_persisted(graph, snap, exec, &NoopPersist).await
}

/// Serial pass with a durability hook: [`Persist::persist`] is invoked right
/// after a node is claimed (`in_progress`, before its side effect runs) and
/// after each node completes — so a crash mid-node leaves a claimed state and
/// completed nodes never re-run on resume (A.3 claim-then-act, at-least-once).
pub async fn run_persisted(
    graph: &Graph,
    snap: &mut Snapshot,
    exec: &dyn NodeExecutor,
    persist: &dyn Persist,
) -> AppResult<()> {
    if snap.status != S_RUNNING {
        return Ok(());
    }
    let mut queue: VecDeque<String> = VecDeque::new();
    queue.push_back(graph.start.clone());

    while let Some(id) = queue.pop_front() {
        if snap.status != S_RUNNING {
            break;
        }
        let Some(node) = graph.nodes.get(&id) else {
            continue;
        };
        // Idempotent resume: already decided nodes never re-run, but their
        // downstream must still be (re)propagated from persisted state.
        if let Some(st) = snap.node_states.get(&id)
            && st.status == N_SUCCESS
        {
            resume_completed(graph, snap, &id, &mut queue)?;
            continue;
        }
        if let Some(st) = snap.node_states.get(&id)
            && st.status == N_SKIPPED
        {
            skip_node(graph, snap, &id, &mut queue)?;
            continue;
        }
        // error_output nodes are decided (llm-node.md §5.2 / H1): re-running
        // would double-bill LLM calls after an await-resume.
        if let Some(st) = snap.node_states.get(&id)
            && st.status == N_ERROR_OUTPUT
        {
            resume_error_output(graph, snap, &id, &mut queue)?;
            continue;
        }
        // Stock-bug sibling of H1: `continue_on_error`-failed nodes already
        // fanned out in a prior pass — replay the fan-out, never re-exec.
        if let Some(st) = snap.node_states.get(&id)
            && st.status == N_FAILED
            && serde_json::from_value::<NodeModifiers>(node.data.modifiers.clone())
                .unwrap_or_default()
                .continue_on_error
        {
            fan_out_after_run(graph, snap, &id, &mut queue)?;
            continue;
        }

        snap.exec_order.push(id.clone());

        let attempt = snap.node_states.get(&id).map_or(1, |s| s.attempt + 1);
        match node.data.kind.as_str() {
            nodes::T_START => {
                set_in_progress(snap, &id, attempt);
                persist.persist(snap).await?;
                mark_node_success(snap, &id, Value::Null);
                fan_out_after_run(graph, snap, &id, &mut queue)?;
            }
            nodes::T_END => {
                set_in_progress(snap, &id, attempt);
                persist.persist(snap).await?;
                let outputs = resolve_end_outputs(node, snap)?;
                finish_success(snap, &id, outputs);
            }
            nodes::T_BRANCH => {
                set_in_progress(snap, &id, attempt);
                persist.persist(snap).await?;
                let cfg: BranchConfig = serde_json::from_value(node.data.config.clone())
                    .map_err(|e| AppError::BadRequest(format!("branch config: {e}")))?;
                let handle = pick_branch(&cfg, &snap.pool)?;
                mark_node_success(snap, &id, json!({"handle": handle}));
                fan_out_after_branch(graph, snap, &id, Some(handle.as_str()), &mut queue)?;
            }
            nodes::T_SCRIPT | nodes::T_EGRESS | nodes::T_LLM => {
                let mods: NodeModifiers =
                    serde_json::from_value(node.data.modifiers.clone()).unwrap_or_default();
                let attempts = mods
                    .retry
                    .as_ref()
                    .and_then(|r| r.attempts)
                    .unwrap_or(1)
                    .max(1);
                // Directly after `start` with no explicit `input` mapping: pass
                // the caller's trigger inputs through by default, so external /
                // manual runs reach the first script without extra wiring.
                // `llm` reads variables through message templates instead and
                // ignores the fed input (llm-node.md W5) — feed it nothing.
                let input = if node.data.kind == nodes::T_LLM {
                    Value::Object(serde_json::Map::new())
                } else {
                    let has_explicit_input = node.data.config.get("input").is_some();
                    let fed_by_start_only = graph
                        .in_edges
                        .get(&id)
                        .map(|idx| {
                            !idx.is_empty()
                                && idx.iter().all(|&ei| graph.edges[ei].source == graph.start)
                        })
                        .unwrap_or(false);
                    if !has_explicit_input && fed_by_start_only {
                        let mut m = serde_json::Map::new();
                        if let Some(ns) = snap.pool.get(&graph.start) {
                            for (k, v) in ns {
                                m.insert(k.clone(), v.clone());
                            }
                        }
                        Value::Object(m)
                    } else {
                        match resolve_inputs(&node.data.config, &snap.pool) {
                            Ok(v) => v,
                            Err(e) => {
                                fail(snap, &id, e.to_string());
                                continue;
                            }
                        }
                    }
                };
                snap.node_states.entry(id.clone()).or_default().input = Some(input.clone());
                let mut last_error: Option<String> = None;
                let mut outcome: Option<ExecOutcome> = None;
                for i in 1..=attempts {
                    // Claim before each attempt: persisted → crash mid-node
                    // re-runs at-least-once from this claim (A.3).
                    set_in_progress(snap, &id, i);
                    persist.persist(snap).await?;
                    match exec.exec(node, input.clone(), &snap.pool).await {
                        Ok(o) => {
                            outcome = Some(o);
                            break;
                        }
                        Err(e) => {
                            last_error = Some(e.to_string());
                            // Config/authoring errors (400) fail fast: the
                            // engine retry is otherwise blind (llm-node.md §4.4).
                            if matches!(e, AppError::BadRequest(_)) {
                                break;
                            }
                        }
                    }
                }
                match outcome {
                    Some(o) => {
                        mark_node_success(snap, &id, o.output);
                        if let Some(st) = snap.node_states.get_mut(&id) {
                            st.usage = o.usage;
                            st.latency_ms = o.latency_ms;
                        }
                        fan_out_exec(graph, snap, &id, &mut queue, false)?;
                    }
                    None => {
                        let msg = last_error.unwrap_or_default();
                        let strategy = mods.on_error_strategy.as_deref().unwrap_or("fail");
                        match strategy {
                            "error_output" => {
                                let out = json!({"error": msg});
                                let st = snap.node_states.entry(id.clone()).or_default();
                                st.status = N_ERROR_OUTPUT.to_string();
                                st.error = Some(json!({"message": msg}));
                                st.output = Some(out.clone());
                                snap.pool
                                    .entry(id.clone())
                                    .or_default()
                                    .insert("output".to_string(), out);
                                fan_out_exec(graph, snap, &id, &mut queue, true)?;
                            }
                            "default_value" => {
                                let out = mods.default_outputs.clone().unwrap_or(Value::Null);
                                let st = snap.node_states.entry(id.clone()).or_default();
                                st.status = N_ERROR_OUTPUT.to_string();
                                st.error = Some(json!({"message": msg}));
                                st.output = Some(out.clone());
                                if out.is_object() || out.is_array() {
                                    snap.pool
                                        .entry(id.clone())
                                        .or_default()
                                        .insert("output".to_string(), out);
                                }
                                fan_out_exec(graph, snap, &id, &mut queue, false)?;
                            }
                            _ if mods.continue_on_error => {
                                // Fail the node but pass the run through (downstream
                                // continues; its inputs still resolve upstream).
                                let st = snap.node_states.entry(id.clone()).or_default();
                                st.status = N_FAILED.to_string();
                                st.error = Some(json!({"message": msg}));
                                fan_out_after_run(graph, snap, &id, &mut queue)?;
                            }
                            _ => {
                                fail(snap, &id, msg);
                            }
                        }
                    }
                }
            }
            nodes::T_AWAIT => {
                // Park the run: node marked waiting + recorded, instance ->
                // waiting. Resume completes it (see `resume_snapshot`), then the
                // engine's success-resume fan-out continues downstream.
                let st = snap.node_states.entry(id.clone()).or_default();
                st.status = N_WAITING.to_string();
                if !snap.waiting_nodes.contains(&id) {
                    snap.waiting_nodes.push(id.clone());
                }
                snap.status = S_WAITING.to_string();
                persist.persist(snap).await?;
                break;
            }
            other => {
                fail(snap, &id, format!("unsupported node type '{other}'"));
            }
        }
        persist.persist(snap).await?;
    }
    if snap.status == S_RUNNING {
        // queue drained without an end → treat as success (no outputs).
        snap.status = S_SUCCESS.to_string();
        persist.persist(snap).await?;
    }
    Ok(())
}

fn set_in_progress(snap: &mut Snapshot, id: &str, attempt: i64) {
    let st = snap.node_states.entry(id.to_string()).or_default();
    st.status = N_IN_PROGRESS.to_string();
    st.attempt = attempt;
}

fn mark_node_success(snap: &mut Snapshot, id: &str, output: Value) {
    let st = snap.node_states.entry(id.to_string()).or_default();
    st.status = N_SUCCESS.to_string();
    st.output = Some(output.clone());
    // v2 D1 flat addressing: an object output's fields become the namespace
    // directly (`{{#id.field#}}`). Null writes nothing (start keeps its seeded
    // params); any other non-object lands under the single `value` field.
    let ns = snap.pool.entry(id.to_string()).or_default();
    if let Value::Object(map) = output {
        for (k, v) in map {
            ns.insert(k, v);
        }
    } else if !output.is_null() {
        ns.insert("value".to_string(), output);
    }
}

fn finish_success(snap: &mut Snapshot, id: &str, outputs: Value) {
    let st = snap.node_states.entry(id.to_string()).or_default();
    st.status = N_SUCCESS.to_string();
    st.output = Some(outputs.clone());
    snap.outputs = Some(outputs);
    snap.status = S_SUCCESS.to_string();
}

fn fail(snap: &mut Snapshot, id: &str, msg: String) {
    let st = snap.node_states.entry(id.to_string()).or_default();
    st.status = N_FAILED.to_string();
    st.error = Some(json!({"message": msg}));
    snap.error = Some(json!({"node_id": id, "message": msg}));
    snap.status = S_FAILED.to_string();
}

/// Non-branch node: every outgoing edge is taken; then targets become ready.
fn fan_out_after_run(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let Some(idx) = graph.out_edges.get(id) else {
        return Ok(());
    };
    for &ei in idx {
        mark(graph, snap, ei, EdgeMark::Taken);
    }
    for &ei in idx {
        consider_target(graph, snap, &graph.edges[ei].target, queue)?;
    }
    Ok(())
}

/// EXEC-node fan-out by verdict (llm-node.md §5.2): success → every non-
/// `error_out` edge is taken (error branch skipped); error_output → only the
/// `error_out` edges are taken. Without `error_out` edges this equals
/// [`fan_out_after_run`].
fn fan_out_exec(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    queue: &mut VecDeque<String>,
    error_path: bool,
) -> AppResult<()> {
    let Some(idx) = graph.out_edges.get(id) else {
        return Ok(());
    };
    for &ei in idx {
        let is_err_edge = graph.edges[ei].source_handle == super::nodes::H_ERROR_OUT;
        let taken = if error_path {
            is_err_edge
        } else {
            !is_err_edge
        };
        mark(
            graph,
            snap,
            ei,
            if taken {
                EdgeMark::Taken
            } else {
                EdgeMark::Skipped
            },
        );
    }
    for &ei in idx {
        consider_target(graph, snap, &graph.edges[ei].target, queue)?;
    }
    Ok(())
}

/// Branch node: only the chosen handle edges are taken; others skipped.
fn fan_out_after_branch(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    handle: Option<&str>,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let Some(idx) = graph.out_edges.get(id) else {
        return Ok(());
    };
    for &ei in idx {
        let edge = &graph.edges[ei];
        let taken = Some(edge.source_handle.as_str()) == handle;
        mark(
            graph,
            snap,
            ei,
            if taken {
                EdgeMark::Taken
            } else {
                EdgeMark::Skipped
            },
        );
    }
    for &ei in idx {
        consider_target(graph, snap, &graph.edges[ei].target, queue)?;
    }
    Ok(())
}

fn mark(graph: &Graph, snap: &mut Snapshot, ei: usize, mark: EdgeMark) {
    snap.edge_marks.insert(edge_key(&graph.edges[ei]), mark);
}

/// After an incoming edge of `target` was decided: if all decided → ready
/// (any taken) or skipped (all skipped, propagated).
fn consider_target(
    graph: &Graph,
    snap: &mut Snapshot,
    target: &str,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let Some(in_edges) = graph.in_edges.get(target) else {
        return Ok(());
    };
    let decided = in_edges
        .iter()
        .filter(|ei| snap.edge_marks.contains_key(&edge_key(&graph.edges[**ei])))
        .count();
    if decided < in_edges.len() {
        return Ok(()); // still waiting on other branches (join)
    }
    let any_taken = in_edges.iter().any(|ei| {
        matches!(
            snap.edge_marks.get(&edge_key(&graph.edges[*ei])),
            Some(EdgeMark::Taken)
        )
    });
    if any_taken {
        if !queue.contains(&target.to_string()) {
            queue.push_back(target.to_string());
        }
    } else {
        skip_node(graph, snap, target, queue)?;
    }
    Ok(())
}

/// Mark a node skipped and propagate down (skip = its edges are Skipped).
fn skip_node(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let declared = graph
        .nodes
        .get(id)
        .map(|n| nodes::declared_output_fields(&n.data.kind, &n.data.config))
        .unwrap_or_default();
    {
        let st = snap.node_states.entry(id.to_string()).or_default();
        st.status = N_SKIPPED.to_string();
    }
    // v2 D6 skip-nulls: downstream refs to a skipped node resolve to explicit
    // nulls instead of 400-ing — the reference laws stay closed at runtime.
    if !declared.is_empty() {
        let ns = snap.pool.entry(id.to_string()).or_default();
        for field in declared {
            ns.entry(field).or_insert(Value::Null);
        }
    }
    let Some(idx) = graph.out_edges.get(id) else {
        return Ok(());
    };
    for &ei in idx {
        mark(graph, snap, ei, EdgeMark::Skipped);
    }
    for &ei in idx {
        consider_target(graph, snap, &graph.edges[ei].target, queue)?;
    }
    Ok(())
}

/// Replay fan-out for a node already completed in a persisted snapshot (resume).
fn resume_completed(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let Some(node) = graph.nodes.get(id) else {
        return Ok(());
    };
    match node.data.kind.as_str() {
        nodes::T_BRANCH => {
            let handle = snap
                .node_states
                .get(id)
                .and_then(|s| s.output.as_ref())
                .and_then(|o| o.get("handle"))
                .and_then(Value::as_str)
                .map(str::to_string);
            fan_out_after_branch(graph, snap, id, handle.as_deref(), queue)?;
        }
        nodes::T_END => {}
        nodes::T_SCRIPT | nodes::T_EGRESS | nodes::T_LLM => {
            // Same verdict fan-out as the live path: a succeeded exec node
            // skips its error_out edges (they were Skipped in the prior pass).
            fan_out_exec(graph, snap, id, queue, false)?;
        }
        _ => {
            fan_out_after_run(graph, snap, id, queue)?;
        }
    }
    Ok(())
}

/// Replay fan-out for an `error_output`-decided node on resume (H1 fix):
/// never re-exec (would double-bill LLM calls); re-route by the node's
/// declared strategy — `error_output` → error_out edges, else normal out.
fn resume_error_output(
    graph: &Graph,
    snap: &mut Snapshot,
    id: &str,
    queue: &mut VecDeque<String>,
) -> AppResult<()> {
    let is_error_branch = graph
        .nodes
        .get(id)
        .and_then(|n| n.data.modifiers.get("on_error_strategy"))
        .and_then(Value::as_str)
        .is_some_and(|s| s == "error_output");
    fan_out_exec(graph, snap, id, queue, is_error_branch)
}

// ── value resolution (literal/ref; expr → P1.7) ──────────────────────────

fn resolve(raw: &Value, pool: &Pool) -> AppResult<Value> {
    if let Some(arr) = raw.get("ref").and_then(Value::as_array) {
        return resolve_ref(arr, pool);
    }
    if let Some(v) = raw.get("literal") {
        return Ok(v.clone());
    }
    if raw.get("expr").is_some() {
        return Err(AppError::BadRequest("expr 求值尚未接线（P1.7）".into()));
    }
    Ok(raw.clone())
}

fn resolve_ref(sel: &[Value], pool: &Pool) -> AppResult<Value> {
    // v2 D7: single-segment `[ns]` = whole namespace object.
    if sel.len() == 1 {
        let ns = sel[0].as_str().unwrap_or_default();
        let m = pool
            .get(ns)
            .ok_or_else(|| AppError::BadRequest(format!("ref 引用不存在: {ns}")))?;
        let map: serde_json::Map<String, Value> = m.clone().into_iter().collect();
        return Ok(Value::Object(map));
    }
    if sel.is_empty() {
        return Err(AppError::BadRequest("ref 不能为空".into()));
    }
    let ns = sel[0].as_str().unwrap_or_default();
    let name = sel[1].as_str().unwrap_or_default();
    let mut v = pool
        .get(ns)
        .and_then(|m| m.get(name))
        .cloned()
        .ok_or_else(|| AppError::BadRequest(format!("ref 引用不存在: {ns}.{name}")))?;
    for part in sel.iter().skip(2) {
        let key = part
            .as_str()
            .ok_or_else(|| AppError::BadRequest("ref 子路径元素必须是字符串".into()))?;
        v = v
            .get(key)
            .cloned()
            .ok_or_else(|| AppError::BadRequest(format!("ref 子路径不存在: {key}")))?;
    }
    Ok(v)
}

fn resolve_inputs(config: &Value, pool: &Pool) -> AppResult<Value> {
    let mut out = serde_json::Map::new();
    if let Some(input) = config.get("input").and_then(Value::as_object) {
        for (k, v) in input {
            out.insert(k.clone(), resolve(v, pool)?);
        }
    }
    Ok(Value::Object(out))
}

fn resolve_end_outputs(node: &GraphNode, snap: &Snapshot) -> AppResult<Value> {
    let cfg: EndConfig = serde_json::from_value(node.data.config.clone())
        .map_err(|e| AppError::BadRequest(format!("end config: {e}")))?;
    let mut out = serde_json::Map::new();
    for o in &cfg.outputs {
        out.insert(o.key.clone(), resolve(&o.value, &snap.pool)?);
    }
    Ok(Value::Object(out))
}

fn pick_branch(cfg: &BranchConfig, pool: &Pool) -> AppResult<String> {
    for rule in &cfg.branches {
        let matched = eval_condition(&rule.when, pool)?;
        if matched {
            return Ok(rule.handle.clone().unwrap_or_default());
        }
    }
    Ok(cfg.else_handle.clone().unwrap_or_default())
}

/// Structured condition `{op, var, value}` or a literal bool. Expression
/// strings (`{{#…#}} >= 3`) require the P1.7 evaluator.
fn eval_condition(when: &Value, pool: &Pool) -> AppResult<bool> {
    if let Some(op) = when.get("op").and_then(Value::as_str) {
        let left = resolve(when.get("var").unwrap_or(&Value::Null), pool)?;
        let right = when.get("value").cloned().unwrap_or(Value::Null);
        return eval_op(op, &left, &right);
    }
    if let Some(b) = when.as_bool() {
        return Ok(b);
    }
    if let Some(s) = when.as_str() {
        return super::expr::eval_bool(s, pool);
    }
    Ok(false)
}

fn eval_op(op: &str, left: &Value, right: &Value) -> AppResult<bool> {
    let num_cmp = |o: &str| -> Option<bool> {
        let l = left.as_f64()?;
        let r = right.as_f64()?;
        Some(match o {
            ">" => l > r,
            ">=" => l >= r,
            "<" => l < r,
            "<=" => l <= r,
            "==" => l == r,
            "!=" => l != r,
            _ => return None,
        })
    };
    let r = match op {
        "==" => equalish(left, right),
        "!=" => !equalish(left, right),
        "in" => right
            .as_array()
            .map(|a| a.iter().any(|x| equalish(x, left)))
            .unwrap_or(false),
        "contains" => left
            .as_str()
            .map(|s| s.contains(right.as_str().unwrap_or_default()))
            .unwrap_or(false),
        "starts_with" => left
            .as_str()
            .map(|s| s.starts_with(right.as_str().unwrap_or_default()))
            .unwrap_or(false),
        "ends_with" => left
            .as_str()
            .map(|s| s.ends_with(right.as_str().unwrap_or_default()))
            .unwrap_or(false),
        "and" | "or" | "not" => {
            return Err(AppError::BadRequest(format!(
                "组合条件 '{op}' 待 P1.7 递归支持"
            )));
        }
        _ => num_cmp(op).unwrap_or(false),
    };
    Ok(r)
}

fn equalish(a: &Value, b: &Value) -> bool {
    match (a.as_f64(), b.as_f64()) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct StubExec;
    #[async_trait]
    impl NodeExecutor for StubExec {
        async fn exec(
            &self,
            _node: &GraphNode,
            _input: Value,
            _pool: &Pool,
        ) -> AppResult<ExecOutcome> {
            Ok(ExecOutcome {
                output: json!({"stub": true}),
                usage: None,
                latency_ms: None,
            })
        }
    }

    /// Executor that fails the first `fail_until` calls, then succeeds.
    struct FlakyExec {
        calls: std::sync::atomic::AtomicUsize,
        fail_until: usize,
    }
    #[async_trait]
    impl NodeExecutor for FlakyExec {
        async fn exec(
            &self,
            _node: &GraphNode,
            _input: Value,
            _pool: &Pool,
        ) -> AppResult<ExecOutcome> {
            let n = self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            if n < self.fail_until {
                // Internal (retryable): BadRequest now short-circuits retries.
                Err(AppError::Internal(anyhow::anyhow!("flaky boom")))
            } else {
                Ok(ExecOutcome {
                    output: json!({"stub": true}),
                    usage: None,
                    latency_ms: None,
                })
            }
        }
    }

    fn graph_of(def: Value) -> Graph {
        super::super::graph::load_definition(&def).unwrap()
    }
    fn def(nodes: Value, edges: Value) -> Value {
        json!({"name":"t","graph":{"nodes":nodes,"edges":edges}})
    }

    fn node(id: &str, kind: &str, config: Value) -> Value {
        json!({"id": id, "data": {"type": kind, "config": config}})
    }
    fn edge(s: &str, h: &str, t: &str) -> Value {
        json!({"source": s, "sourceHandle": h, "target": t})
    }

    #[tokio::test]
    async fn linear_end_outputs_resolved() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node(
                    "end",
                    "end",
                    json!({"outputs": [{"key": "answer", "value": {"ref": ["start", "msg"]}}]})
                )
            ]),
            json!([edge("start", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        let mut start_input = HashMap::new();
        start_input.insert("msg".into(), json!("hi"));
        snap.pool.insert("start".into(), start_input);
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.outputs.unwrap()["answer"], "hi");
        assert_eq!(snap.node_states["end"].status, N_SUCCESS);
    }

    #[tokio::test]
    async fn branch_false_skips_true_path() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node(
                    "br",
                    "branch",
                    json!({
                        "branches": [{"id": "b1", "when": {"op": "==", "var": ["start", "msg"], "value": "hi"}, "handle": "true"}],
                        "else_handle": "false"
                    })
                ),
                node("na", "end", json!({"outputs": []})),
                node("nb", "end", json!({"outputs": []}))
            ]),
            json!([
                edge("start", "out", "br"),
                edge("br", "true", "na"),
                edge("br", "false", "nb")
            ]),
        ));
        let mut snap = Snapshot::new();
        let mut start_input = HashMap::new();
        start_input.insert("msg".into(), json!("bye"));
        snap.pool.insert("start".into(), start_input);
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["na"].status, N_SKIPPED);
        assert_eq!(snap.node_states["nb"].status, N_SUCCESS);
    }

    #[tokio::test]
    async fn branch_expr_string_when() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node(
                    "br",
                    "branch",
                    json!({
                        "branches": [{"id": "b1", "when": "{{#start.msg#}} == \"hi\"", "handle": "true"}],
                        "else_handle": "false"
                    })
                ),
                node("na", "end", json!({"outputs": []})),
                node("nb", "end", json!({"outputs": []}))
            ]),
            json!([
                edge("start", "out", "br"),
                edge("br", "true", "na"),
                edge("br", "false", "nb")
            ]),
        ));
        let mut snap = Snapshot::new();
        let mut start_input = HashMap::new();
        start_input.insert("msg".into(), json!("hi"));
        snap.pool.insert("start".into(), start_input);
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["na"].status, N_SUCCESS);
        assert_eq!(snap.node_states["nb"].status, N_SKIPPED);
    }

    #[tokio::test]
    async fn join_waits_for_both_branches() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node("e1", "egress", json!({"client_key": "k", "op": "o"})),
                node("e2", "egress", json!({"client_key": "k", "op": "o"})),
                node(
                    "end",
                    "end",
                    json!({"outputs": [{"key": "v", "value": {"ref": ["e2"]}}]})
                )
            ]),
            json!([
                edge("start", "out", "e1"),
                edge("start", "out", "e2"),
                edge("e1", "out", "end"),
                edge("e2", "out", "end")
            ]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["e1"].status, N_SUCCESS);
        assert_eq!(snap.node_states["e2"].status, N_SUCCESS);
        assert_eq!(snap.outputs.unwrap()["v"]["stub"], true);
    }

    fn node_m(id: &str, kind: &str, config: Value, mods: Value) -> Value {
        json!({"id": id, "data": {"type": kind, "config": config, "modifiers": mods}})
    }

    #[tokio::test]
    async fn retry_recovers_after_failure() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"retry": {"attempts": 3}})
                ),
                node("end", "end", json!({}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let flaky = FlakyExec {
            calls: Default::default(),
            fail_until: 1,
        };
        run(&g, &mut snap, &flaky).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["e1"].status, N_SUCCESS);
        assert_eq!(snap.node_states["e1"].attempt, 2, "one fail + one success");
    }

    #[tokio::test]
    async fn continue_on_error_passes_through() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"continue_on_error": true})
                ),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let always_fail = FlakyExec {
            calls: Default::default(),
            fail_until: usize::MAX,
        };
        run(&g, &mut snap, &always_fail).await.unwrap();
        assert_eq!(
            snap.status, S_SUCCESS,
            "run passes through on continue_on_error"
        );
        assert_eq!(snap.node_states["e1"].status, N_FAILED);
        assert_eq!(snap.node_states["end"].status, N_SUCCESS);
    }

    #[tokio::test]
    async fn error_output_routes_error_branch() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"on_error_strategy": "error_output"})
                ),
                node("ok_end", "end", json!({"outputs": []})),
                node("err_end", "end", json!({"outputs": []}))
            ]),
            json!([
                edge("start", "out", "e1"),
                edge("e1", "out", "ok_end"),
                edge("e1", "error_out", "err_end")
            ]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let always_fail = FlakyExec {
            calls: Default::default(),
            fail_until: usize::MAX,
        };
        run(&g, &mut snap, &always_fail).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS, "error_output 不终止 run");
        assert_eq!(snap.node_states["e1"].status, N_ERROR_OUTPUT);
        assert_eq!(snap.node_states["ok_end"].status, N_SKIPPED);
        assert_eq!(snap.node_states["err_end"].status, N_SUCCESS);
        assert!(snap.pool["e1"]["output"]["error"].is_string());
    }

    #[tokio::test]
    async fn default_value_fabricates_output() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({
                        "on_error_strategy": "default_value",
                        "default_outputs": {"score": 0}
                    })
                ),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let always_fail = FlakyExec {
            calls: Default::default(),
            fail_until: usize::MAX,
        };
        run(&g, &mut snap, &always_fail).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["e1"].status, N_ERROR_OUTPUT);
        assert_eq!(snap.pool["e1"]["output"]["score"], 0);
        assert_eq!(snap.node_states["end"].status, N_SUCCESS);
    }

    #[tokio::test]
    async fn bad_request_fails_fast_without_retry_burn() {
        struct BadReqExec {
            calls: std::sync::atomic::AtomicUsize,
        }
        #[async_trait]
        impl NodeExecutor for BadReqExec {
            async fn exec(
                &self,
                _node: &GraphNode,
                _input: Value,
                _pool: &Pool,
            ) -> AppResult<ExecOutcome> {
                self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                Err(AppError::BadRequest("config bad".into()))
            }
        }
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"retry": {"attempts": 3}})
                ),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let exec = BadReqExec {
            calls: Default::default(),
        };
        run(&g, &mut snap, &exec).await.unwrap();
        assert_eq!(snap.status, S_FAILED);
        assert_eq!(
            exec.calls.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "BadRequest 短路：不烧 attempts"
        );
        assert_eq!(snap.node_states["e1"].attempt, 1);
    }

    #[tokio::test]
    async fn error_output_node_not_rerun_on_resume() {
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"on_error_strategy": "error_output"})
                ),
                node("gate", "await", json!({"kind": "human"})),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([
                edge("start", "out", "e1"),
                edge("e1", "error_out", "gate"),
                edge("gate", "out", "end")
            ]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        let always_fail = FlakyExec {
            calls: Default::default(),
            fail_until: usize::MAX,
        };
        run(&g, &mut snap, &always_fail).await.unwrap();
        assert_eq!(snap.status, S_WAITING);
        assert_eq!(snap.node_states["e1"].status, N_ERROR_OUTPUT);
        let calls_after_first = always_fail.calls.load(std::sync::atomic::Ordering::SeqCst);

        resume_snapshot(&mut snap, Some(json!({"approved": true}))).unwrap();
        run(&g, &mut snap, &always_fail).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(
            always_fail.calls.load(std::sync::atomic::Ordering::SeqCst),
            calls_after_first,
            "resume 不重跑 error_output 节点（H1：避免二次计费）"
        );
        assert_eq!(snap.node_states["end"].status, N_SUCCESS);
    }

    #[tokio::test]
    async fn flat_addressing_downstream_ref() {
        // v2 D1: object output fields are the namespace directly — no `output` segment.
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node("s1", "script", json!({"language": "js", "code": "1"})),
                node(
                    "end",
                    "end",
                    json!({"outputs": [{"key": "v", "value": {"ref": ["s1", "stub"]}}]})
                )
            ]),
            json!([edge("start", "out", "s1"), edge("s1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(
            snap.outputs.unwrap()["v"],
            json!(true),
            "字段引用直接取值（平铺无壳）"
        );
    }

    #[tokio::test]
    async fn single_segment_ref_returns_whole_namespace() {
        // v2 D7: [ns] alone resolves to the whole field map.
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node("e1", "egress", json!({"client_key": "k", "op": "o"})),
                node(
                    "end",
                    "end",
                    json!({"outputs": [{"key": "all", "value": {"ref": ["e1"]}}]})
                )
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.outputs.unwrap()["all"], json!({"stub": true}));
    }

    #[tokio::test]
    async fn skipped_node_declared_fields_resolve_null() {
        // v2 D6: branch not taken → skipped node's declared fields are null,
        // downstream refs resolve (null) instead of 400.
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node(
                    "br",
                    "branch",
                    json!({
                        "branches": [{"handle": "yes", "when": {"op": "==", "var": {"ref": ["start", "go"]}, "value": true}}],
                        "else_handle": "no"
                    })
                ),
                node("e1", "egress", json!({"client_key": "k", "op": "o"})),
                node(
                    "end",
                    "end",
                    json!({"outputs": [{"key": "v", "value": {"ref": ["e1", "response"]}}]})
                )
            ]),
            json!([
                edge("start", "out", "br"),
                edge("br", "yes", "e1"),
                edge("e1", "out", "end"),
                edge("br", "no", "end")
            ]),
        ));
        let mut snap = Snapshot::new();
        let mut si = HashMap::new();
        si.insert("go".into(), json!(false));
        snap.pool.insert("start".into(), si);
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["e1"].status, N_SKIPPED);
        assert_eq!(
            snap.outputs.unwrap()["v"],
            Value::Null,
            "跳过节点的声明字段解析为 null"
        );
    }

    #[tokio::test]
    async fn usage_and_latency_ride_node_state() {
        struct UsageExec;
        #[async_trait]
        impl NodeExecutor for UsageExec {
            async fn exec(
                &self,
                _node: &GraphNode,
                _input: Value,
                _pool: &Pool,
            ) -> AppResult<ExecOutcome> {
                Ok(ExecOutcome {
                    output: json!({"text": "hi"}),
                    usage: Some(json!({"total_tokens": 42})),
                    latency_ms: Some(7),
                })
            }
        }
        let g = graph_of(def(
            json!([
                node("start", "start", json!({})),
                node("e1", "egress", json!({"client_key": "k", "op": "o"})),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        run(&g, &mut snap, &UsageExec).await.unwrap();
        assert_eq!(
            snap.node_states["e1"].usage.as_ref().unwrap()["total_tokens"],
            42
        );
        assert_eq!(snap.node_states["e1"].latency_ms, Some(7));
    }

    #[test]
    fn error_output_without_edge_rejected_at_publish() {
        let bad = def(
            json!([
                node("start", "start", json!({})),
                node_m(
                    "e1",
                    "egress",
                    json!({"client_key": "k", "op": "o"}),
                    json!({"on_error_strategy": "error_output"})
                ),
                node("end", "end", json!({"outputs": []}))
            ]),
            json!([edge("start", "out", "e1"), edge("e1", "out", "end")]),
        );
        let err = match super::super::graph::load_definition(&bad) {
            Ok(_) => panic!("error_output 无 error_out 边应被发布校验拒绝"),
            Err(e) => e,
        };
        assert!(err.to_string().contains("error_out"), "{err}");
    }

    #[tokio::test]
    async fn await_parks_then_resume_continues() {
        let g = graph_of(def(
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
        ));
        let mut snap = Snapshot::new();
        snap.pool.insert("start".into(), HashMap::new());
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_WAITING);
        assert_eq!(snap.node_states["gate"].status, N_WAITING);
        assert!(snap.waiting_nodes.contains(&"gate".to_string()));

        // Resume: complete the gate with an approval payload.
        resume_snapshot(&mut snap, Some(json!({"approved": true}))).unwrap();
        assert_eq!(snap.status, S_RUNNING);
        run(&g, &mut snap, &StubExec).await.unwrap();
        assert_eq!(snap.status, S_SUCCESS);
        assert_eq!(snap.node_states["gate"].status, N_SUCCESS);
        assert_eq!(snap.node_states["end"].status, N_SUCCESS);
        assert_eq!(snap.outputs.unwrap()["ok"], true);
    }
}
