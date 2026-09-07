//! Graph loading & structural validation (flow-graph.md).
//!
//! Parses a flow definition's `graph` (ReactFlow canvas JSON) into a `Graph`:
//! unique node ids, edge endpoints exist, exactly one start (no incoming edge),
//! per-node `data.type/version` validated through the node registry.
//! Builds out-edges + in-count indexes ready for the P1.2 scheduler.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::errors::app_error::{AppError, AppResult};

use super::nodes;

/// Per-node engine data (`nodes[].data`): engine reads type/version/config.
#[derive(Debug, Clone, Deserialize)]
pub struct NodeData {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default = "default_version")]
    pub version: i64,
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub desc: Option<String>,
    #[serde(default)]
    pub config: Value,
    #[serde(default)]
    pub modifiers: Value,
}

fn default_version() -> i64 {
    1
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub data: NodeData,
}

/// Connection (edge) — ports default to `out` / `in`.
#[derive(Debug, Clone)]
pub struct Edge {
    pub source: String,
    pub source_handle: String,
    pub target: String,
    pub target_handle: String,
}

/// Loaded, validated graph ready for execution.
#[derive(Debug, Clone)]
pub struct Graph {
    pub nodes: HashMap<String, GraphNode>,
    pub edges: Vec<Edge>,
    /// Edges by source node id (fan-out; branch handles among them).
    pub out_edges: HashMap<String, Vec<usize>>,
    /// Edge indices by target node id (join = all resolved).
    pub in_edges: HashMap<String, Vec<usize>>,
    /// Incoming edge count by target node id (join = all resolved).
    pub in_count: HashMap<String, usize>,
    pub start: String,
}

fn read_node(n: &Value) -> AppResult<(String, NodeData)> {
    let id = n
        .get("id")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("node missing 'id'".into()))?;
    let data: NodeData = serde_json::from_value(
        n.get("data")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({})),
    )
    .map_err(|e| AppError::BadRequest(format!("node '{id}' data invalid: {e}")))?;
    Ok((id.to_string(), data))
}

fn read_edge(e: &Value) -> AppResult<Edge> {
    let source = e
        .get("source")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("edge missing 'source'".into()))?;
    let target = e
        .get("target")
        .and_then(Value::as_str)
        .ok_or_else(|| AppError::BadRequest("edge missing 'target'".into()))?;
    Ok(Edge {
        source: source.to_string(),
        source_handle: e
            .get("sourceHandle")
            .and_then(Value::as_str)
            .unwrap_or(nodes::H_OUT)
            .to_string(),
        target: target.to_string(),
        target_handle: e
            .get("targetHandle")
            .and_then(Value::as_str)
            .unwrap_or(nodes::H_IN)
            .to_string(),
    })
}

/// Load + validate a graph object `{nodes: [...], edges: [...]}`.
///
/// # Errors
///
/// `BadRequest` on unknown node type/version, malformed config, duplicate ids,
/// dangling edge, or not exactly one start with no incoming edges.
pub fn load_graph(graph_value: &Value) -> AppResult<Graph> {
    let nodes_val = graph_value
        .get("nodes")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("graph missing 'nodes[]'".into()))?;
    let edges_val = graph_value
        .get("edges")
        .and_then(Value::as_array)
        .ok_or_else(|| AppError::BadRequest("graph missing 'edges[]'".into()))?;

    let mut nodes = HashMap::new();
    let mut start: Option<String> = None;
    for n in nodes_val {
        let (id, data) = read_node(n)?;
        if data.kind == "custom-note" {
            continue; // canvas-only note nodes are ignored by the engine.
        }
        if nodes
            .insert(
                id.clone(),
                GraphNode {
                    id: id.clone(),
                    data: data.clone(),
                },
            )
            .is_some()
        {
            return Err(AppError::BadRequest(format!("duplicate node id '{id}'")));
        }
        nodes::validate_node(&data.kind, data.version, &data.config)?;
        if data.kind == nodes::T_START {
            if start.is_some() {
                return Err(AppError::BadRequest("v1 只允许一个 start 节点".into()));
            }
            start = Some(id.clone());
        }
    }
    let Some(start_id) = start else {
        return Err(AppError::BadRequest("graph 缺少 start 节点".into()));
    };

    // v2 D3: non-empty titles are unique (EndOutput keys default to titles;
    // picker display must be unambiguous). Unnamed nodes are exempt — the
    // canvas falls back to the type label.
    {
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for node in nodes.values() {
            let title = node.data.title.trim();
            if title.is_empty() {
                continue;
            }
            if !seen.insert(title) {
                return Err(AppError::BadRequest(format!(
                    "节点标题重复: '{title}'（v2 要求非空标题唯一）"
                )));
            }
        }
    }

    let mut edges = Vec::new();
    let mut out_edges: HashMap<String, Vec<usize>> = HashMap::new();
    let mut in_edges: HashMap<String, Vec<usize>> = HashMap::new();
    let mut in_count: HashMap<String, usize> = HashMap::new();
    for (i, e) in edges_val.iter().enumerate() {
        let edge = read_edge(e)?;
        if !nodes.contains_key(&edge.source) || !nodes.contains_key(&edge.target) {
            return Err(AppError::BadRequest(format!(
                "edge #{i} 引用不存在节点: {} -> {}",
                edge.source, edge.target
            )));
        }
        if edge.target == start_id {
            return Err(AppError::BadRequest("start 节点不允许入边".into()));
        }
        let edge_idx = edges.len();
        out_edges
            .entry(edge.source.clone())
            .or_default()
            .push(edge_idx);
        in_edges
            .entry(edge.target.clone())
            .or_default()
            .push(edge_idx);
        *in_count.entry(edge.target.clone()).or_insert(0) += 1;
        edges.push(edge);
    }

    // on_error_strategy=error_output nodes must own at least one error_out
    // edge (llm-node.md §5.3): publish-time 400, never a runtime dead branch.
    for node in nodes.values() {
        if node
            .data
            .modifiers
            .get("on_error_strategy")
            .and_then(Value::as_str)
            == Some("error_output")
            && !out_edges.get(&node.id).is_some_and(|idx| {
                idx.iter()
                    .any(|&ei| edges[ei].source_handle == nodes::H_ERROR_OUT)
            })
        {
            return Err(AppError::BadRequest(format!(
                "节点 '{}' 声明 on_error_strategy=error_output 但缺少 error_out 出边",
                node.id
            )));
        }
    }

    let graph = Graph {
        nodes,
        edges,
        out_edges,
        in_edges,
        in_count,
        start: start_id,
    };

    // Reference lint (design D4): existence + upstream laws on every
    // template / ValueExpr ref in configs and modifiers.
    super::lint::lint_graph(&graph)?;

    Ok(graph)
}

/// Convenience: load from a full flow definition (`{... graph: {...}}`).
pub fn load_definition(def: &Value) -> AppResult<Graph> {
    let graph = def
        .get("graph")
        .ok_or_else(|| AppError::BadRequest("flow definition missing 'graph'".into()))?;
    load_graph(graph)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn sample() -> Value {
        json!({
            "name": "test",
            "graph": {
                "nodes": [
                    { "id": "start", "data": { "type": "start", "config": {} } },
                    { "id": "s1", "data": { "type": "script", "config": {"language": "js", "code": "return {}"} } },
                    { "id": "e1", "data": { "type": "egress", "config": {"client_key": "llm", "op": "chat"} } },
                    { "id": "end", "data": { "type": "end", "config": {} } }
                ],
                "edges": [
                    { "source": "start", "target": "s1" },
                    { "source": "s1", "target": "e1" },
                    { "source": "e1", "target": "end" }
                ]
            }
        })
    }

    #[test]
    fn loads_valid_graph() {
        let g = load_definition(&sample()).unwrap();
        assert_eq!(g.start, "start");
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.edges.len(), 3);
        assert_eq!(g.in_count["end"], 1);
        assert_eq!(g.out_edges["start"].len(), 1);
    }

    #[test]
    fn rejects_dangling_edge() {
        let mut v = sample();
        v["graph"]["edges"][0]["target"] = json!("missing");
        assert!(load_definition(&v).is_err());
    }

    #[test]
    fn rejects_unknown_node_type() {
        let mut v = sample();
        v["graph"]["nodes"][1]["data"]["type"] = json!("nope");
        assert!(load_definition(&v).is_err());
    }

    #[test]
    fn rejects_two_starts() {
        let mut v = sample();
        v["graph"]["nodes"][3]["data"]["type"] = json!("start");
        assert!(load_definition(&v).is_err());
    }
}
