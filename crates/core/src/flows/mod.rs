//! Flow orchestration engine v2 (dev-docs/workflow).
//!
//! RaisFast 内置动态逻辑编排引擎：图 DAG + 串行执行 + 整棵快照 durable。
//! 深度集成自家资产：host 脚本（四引擎裁剪 host）/ egress / vault+租户 / worker 队列 /
//! eventbus / CT。数据自建独立命名空间（不写 itg_* 表）。
//!
//! 规划子模块（随 P0.5+ 依次落地，此处按文档地图预留）：
//! - `model`  —— flow/flow_version/flow_instance/flow_instance_snapshot/flow_node_run
//!   模型 + repo（db-schema.md）
//! - `graph`  —— 图定义加载/校验（flow-graph.md / contracts.md C1-C3）
//! - `nodes`  —— 节点注册表 + 各节点实现（start/end/script/egress/branch/await）
//! - `engine` —— 串行执行器 + Join/skip + 快照推进 + 错误分层（execution-engine.md）
//! - `handler`—— admin/run/resume/SSE API（contracts.md C4）
//!
//! 设计文档：`dev-docs/workflow/`（README 为地图，contracts.md 为冻结契约，db-schema.md 为表）。

pub mod engine;
pub mod exec;
pub mod expr;
pub mod graph;
pub mod handler;
pub mod lint;
pub mod llm;
pub mod model;
pub mod nodes;
pub mod params;
pub mod run;
pub mod trigger;

#[cfg(feature = "export-types")]
crate::export_types!(
    model::Flow,
    model::FlowVersion,
    model::FlowInstance,
    model::FlowNodeRun,
    model::FlowTrigger,
    nodes::StartParam,
    nodes::StartConfig,
    nodes::EndOutput,
    nodes::EndConfig,
    nodes::SandboxLimits,
    nodes::HostPermissions,
    nodes::ScriptConfig,
    nodes::EgressConfig,
    nodes::BranchRule,
    nodes::BranchConfig,
    nodes::AwaitConfig,
    nodes::LlmMessage,
    nodes::LlmConfig,
    nodes::NodeConfigVariant,
    nodes::ValueExpr,
    nodes::NodeKind,
    handler::CreateFlowReq,
    handler::RunFlowReq,
    handler::ResumeReq,
    handler::UpdateFlowReq,
    handler::PublishFlowReq,
    handler::FlowDetail,
    handler::TriggerCreateReq,
);
