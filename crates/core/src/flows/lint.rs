//! Publish-time reference lint (design D4, dev-docs/workflow v2 proposal).
//!
//! Laws enforced today:
//! 1. **存在律** — every reference namespace must be a node in the graph.
//! 2. **上游律** — the namespace must be a transitive upstream ancestor of the
//!    referencing node (execution order ≡ topological order in the serial DAG).
//!
//! The declaration law (fields ∈ the target node's declared outputs) lands with
//! the v2 declared-output registry. Reserved namespaces (`sys`) are skipped
//! until the engine implements them. Dynamic selectors built inside script
//! code are outside static reach — documented escape hatch.

use std::collections::HashSet;

use serde_json::Value;

use crate::errors::app_error::{AppError, AppResult};

use super::expr;
use super::graph::Graph;

/// Namespaces that are reserved by contract but not (yet) engine-backed.
const RESERVED_NS: &[&str] = &["sys"];

/// One extracted reference: where it appeared (config path for diagnostics)
/// and its selector (`ns.field.child`).
struct FoundRef {
    path: String,
    selector: String,
}

/// Recursively walk a JSON value collecting `{{#sel#}}` selectors from strings
/// and `{"ref": [...]}` arrays from ValueExpr objects.
fn extract_refs(path: &str, v: &Value, out: &mut Vec<FoundRef>) {
    match v {
        Value::String(s) => {
            for sel in expr::selectors_in_text(s) {
                out.push(FoundRef {
                    path: path.to_string(),
                    selector: sel,
                });
            }
        }
        Value::Array(items) => {
            for (i, item) in items.iter().enumerate() {
                extract_refs(&format!("{path}[{i}]"), item, out);
            }
        }
        Value::Object(map) => {
            if let Some(arr) = map.get("ref").and_then(Value::as_array)
                && !arr.is_empty()
                && arr.iter().all(Value::is_string)
            {
                let sel = arr
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(".");
                out.push(FoundRef {
                    path: format!("{path}.ref"),
                    selector: sel,
                });
                return; // a ref object carries no further nested refs
            }
            for (k, child) in map {
                extract_refs(&format!("{path}.{k}"), child, out);
            }
        }
        _ => {}
    }
}

/// Transitive upstream ancestors of `id` (via in-edges closure), `id` excluded.
fn ancestors_of(graph: &Graph, id: &str) -> HashSet<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut queue: Vec<String> = Vec::new();
    if let Some(idx) = graph.in_edges.get(id) {
        for &ei in idx {
            queue.push(graph.edges[ei].source.clone());
        }
    }
    while let Some(cur) = queue.pop() {
        if !seen.insert(cur.clone()) {
            continue;
        }
        if let Some(idx) = graph.in_edges.get(&cur) {
            for &ei in idx {
                let src = graph.edges[ei].source.clone();
                if !seen.contains(&src) {
                    queue.push(src);
                }
            }
        }
    }
    seen
}

/// Lint every node's config + modifiers. Returns BadRequest with the first
/// violation (deterministic: nodes iterate in insertion order).
pub fn lint_graph(graph: &Graph) -> AppResult<()> {
    for (id, node) in &graph.nodes {
        if node.data.kind == super::nodes::T_START {
            continue; // start has no in-edges; refs here would be self/invalid anyway
        }
        let mut refs: Vec<FoundRef> = Vec::new();
        extract_refs("config", &node.data.config, &mut refs);
        extract_refs("modifiers", &node.data.modifiers, &mut refs);
        if refs.is_empty() {
            continue;
        }
        let ancestors = ancestors_of(graph, id);
        for r in refs {
            let Some((ns, _rest)) = r.selector.split_once('.') else {
                continue; // bare `{{##}}`-ish degenerate token — runtime concern
            };
            if RESERVED_NS.contains(&ns) || ns.is_empty() {
                continue;
            }
            if !graph.nodes.contains_key(ns) {
                return Err(AppError::BadRequest(format!(
                    "lint: 节点 '{id}' 引用了不存在的节点 '{ns}'（{}）",
                    r.path
                )));
            }
            if ns == id {
                return Err(AppError::BadRequest(format!(
                    "lint: 节点 '{id}' 不能引用自身输出（{}）",
                    r.path
                )));
            }
            if !ancestors.contains(ns) {
                return Err(AppError::BadRequest(format!(
                    "lint: 节点 '{id}' 引用了非上游节点 '{ns}'（{}）— 引用目标必须是已执行的上游",
                    r.path
                )));
            }
            // Law 3 声明律 (v2 D4): a field reference must hit the target node's
            // declared outputs. Scripts without output_schema declare nothing —
            // documented escape (dynamic shapes).
            if let Some((_, rest)) = r.selector.split_once('.') {
                let field = rest.split('.').next().unwrap_or_default();
                if !field.is_empty() {
                    let target = &graph
                        .nodes
                        .get(ns)
                        .map(|n| (n.data.kind.clone(), n.data.config.clone()));
                    if let Some((kind, config)) = target {
                        let declared = super::nodes::declared_output_fields(kind, config);
                        if !declared.is_empty() && !declared.iter().any(|f| f == field) {
                            return Err(AppError::BadRequest(format!(
                                "lint: 节点 '{id}' 引用了 '{ns}.{field}'，但目标只声明输出 {}（{}）",
                                declared.join("/"),
                                r.path
                            )));
                        }
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn graph_of(nodes: Value, edges: Value) -> AppResult<Graph> {
        super::super::graph::load_definition(&json!({
            "name": "t",
            "graph": { "nodes": nodes, "edges": edges }
        }))
    }

    fn llm(id: &str, text: &str) -> Value {
        json!({"id": id, "data": {"type": "llm", "config": {
            "messages": [{"role": "user", "text": text}]
        }}})
    }
    fn end_ref(id: &str, sel: Vec<&str>) -> Value {
        json!({"id": id, "data": {"type": "end", "config": {
            "outputs": [{"key": "o", "value": {"ref": sel}}]
        }}})
    }
    fn edge(s: &str, t: &str) -> Value {
        json!({"source": s, "sourceHandle": "out", "target": t})
    }

    #[test]
    fn upstream_ref_passes() {
        let g = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {"params": [
                    {"variable": "q", "label": "Q", "type": "text", "required": true}
                ]}}}),
                llm("a", "{{#start.q#}}"),
                end_ref("e", vec!["a", "text"]),
            ]),
            json!([edge("start", "a"), edge("a", "e")]),
        )
        .unwrap();
        assert!(lint_graph(&g).is_ok());
    }

    #[test]
    fn missing_node_rejected() {
        // load_definition lints (publish/test entry) — construction itself 400s.
        let err = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                llm("a", "{{#ghost.x#}}"),
                end_ref("e", vec!["a", "text"]),
            ]),
            json!([edge("start", "a"), edge("a", "e")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
    }

    #[test]
    fn non_upstream_rejected() {
        // e references b, but b is a sibling branch (not an ancestor of e).
        let err = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                llm("a", "x"),
                llm("b", "y"),
                end_ref("e", vec!["b", "text"]),
            ]),
            json!([edge("start", "a"), edge("a", "e"), edge("start", "b")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("非上游"), "{err}");
    }

    #[test]
    fn self_reference_rejected() {
        let err = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                llm("a", "{{#a.text#}}"),
                end_ref("e", vec!["a", "text"]),
            ]),
            json!([edge("start", "a"), edge("a", "e")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("自身"), "{err}");
    }

    #[test]
    fn duplicate_titles_rejected() {
        // v2 D3: non-empty node titles must be unique at publish time.
        let err = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                json!({"id": "a", "data": {"type": "script", "title": "T", "config": {"language": "js", "code": "1"}}}),
                json!({"id": "b", "data": {"type": "script", "title": "T", "config": {"language": "js", "code": "2"}}}),
                end_ref("e", vec!["a"]),
            ]),
            json!([edge("start", "a"), edge("a", "e"), edge("start", "b")]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("标题重复"), "{err}");
    }

    #[test]
    fn reserved_sys_skipped() {
        let g = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                llm("a", "{{#sys.flow_run_id#}}"),
                end_ref("e", vec!["a", "text"]),
            ]),
            json!([edge("start", "a"), edge("a", "e")]),
        )
        .unwrap();
        assert!(lint_graph(&g).is_ok());
    }

    #[test]
    fn ref_array_and_expr_string_caught() {
        // branch structured condition carries a {"ref": [...]} to a ghost node
        let err = graph_of(
            json!([
                json!({"id": "start", "data": {"type": "start", "config": {}}}),
                json!({"id": "br", "data": {"type": "branch", "config": {
                    "branches": [{"handle": "yes", "label": "Y", "when": {"op": "==", "var": {"ref": ["ghost", "x"]}, "value": 1}}]
                }}}),
                end_ref("e", vec!["br", "handle"]),
            ]),
            json!([
                {"source": "start", "sourceHandle": "out", "target": "br"},
                {"source": "br", "sourceHandle": "yes", "target": "e"}
            ]),
        )
        .unwrap_err();
        assert!(err.to_string().contains("不存在"), "{err}");
    }
}
