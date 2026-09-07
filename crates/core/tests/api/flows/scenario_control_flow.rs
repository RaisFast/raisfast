//! Control-flow scenarios: linear, branch (structured + expr), fan-out join,
//! skip propagation.

use super::helpers::*;
use raisfast::flows::engine::{N_SKIPPED, N_SUCCESS, S_SUCCESS};
use serde_json::json;

#[tokio::test]
async fn linear_chain_with_template_and_refs() {
    let g = def_graph(def_of(
        json!([
            node("start", "start", json!({})),
            node("s1", "egress", json!({"client_key": "k", "op": "o"})),
            node(
                "end",
                "end",
                json!({
                    "outputs": [
                        {"key": "echo", "value": {"ref": ["start", "msg"]}},
                        {"key": "n", "value": {"literal": 5}}
                    ]
                })
            )
        ]),
        json!([edge("start", "out", "s1"), edge("s1", "out", "end")]),
    ));
    let mut snap = seeded_snapshot(json!({"msg": "hello"}));
    let exec = Deterministic::new().with("s1", json!({"ok": true}));
    run_pure(&g, &mut snap, &exec).await;
    assert_eq!(snap.status, S_SUCCESS);
    let out = snap.outputs.clone().unwrap();
    assert_eq!(out["echo"], "hello");
    assert_eq!(out["n"], 5);
}

#[tokio::test]
async fn branch_takes_true_path_and_skips_false() {
    let g = def_graph(def_of(
        json!([
            node("start", "start", json!({})),
            node_m(
                "br",
                "branch",
                json!({
                    "branches": [{"id": "b1", "when": "{{#start.level#}} >= 3", "handle": "true"}],
                    "else_handle": "false"
                }),
                json!({})
            ),
            node("na", "egress", json!({"client_key": "k", "op": "o"})),
            node("nb", "egress", json!({"client_key": "k", "op": "o"}))
        ]),
        json!([
            edge("start", "out", "br"),
            edge("br", "true", "na"),
            edge("br", "false", "nb")
        ]),
    ));
    let mut snap = seeded_snapshot(json!({"level": 5}));
    let exec = Deterministic::new();
    run_pure(&g, &mut snap, &exec).await;
    assert_eq!(snap.node_states["na"].status, N_SUCCESS);
    assert_eq!(snap.node_states["nb"].status, N_SKIPPED);
}

#[tokio::test]
async fn join_waits_for_both_fanout_branches() {
    let g = def_graph(def_of(
        json!([
            node("start", "start", json!({})),
            node("a", "egress", json!({"client_key": "k", "op": "o"})),
            node("b", "egress", json!({"client_key": "k", "op": "o"})),
            node("join", "egress", json!({"client_key": "k", "op": "o"})),
            node("end", "end", json!({"outputs": []}))
        ]),
        json!([
            edge("start", "out", "a"),
            edge("start", "out", "b"),
            edge("a", "out", "join"),
            edge("b", "out", "join"),
            edge("join", "out", "end")
        ]),
    ));
    let mut snap = seeded_snapshot(json!({}));
    let exec = Deterministic::new();
    run_pure(&g, &mut snap, &exec).await;
    assert_eq!(snap.status, S_SUCCESS);
    assert_eq!(snap.node_states["a"].status, N_SUCCESS);
    assert_eq!(snap.node_states["b"].status, N_SUCCESS);
    assert_eq!(
        snap.node_states["join"].status, N_SUCCESS,
        "join waits both"
    );
}
