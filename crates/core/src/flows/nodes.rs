//! Node registry + config schemas (contracts.md C1).
//!
//! v1 types: start/end/script/egress/branch (+ await config reserved for P2).
//! `validate_node(type, version, config)` deserializes config into a strong Rust
//! struct — shape errors surface as 400 here, not at runtime. Unknown keys are
//! tolerated (extra=allow) so frontend can carry display fields; required keys
//! and value shapes are enforced.

use serde::Deserialize;
use serde_json::Value;

use crate::errors::app_error::{AppError, AppResult};

/// Control types for start inputs (v2: value-oriented names + five-way file
/// family; `file` is the catch-all "other" bucket).
pub const START_PARAM_TYPES: &[&str] = &[
    "text",
    "paragraph",
    "select",
    "number",
    "boolean",
    "json",
    "file",
    "file-array",
];

/// Allowed file categories for `accept` (no "any" — other is the catch-all).
pub const FILE_ACCEPT_TYPES: &[&str] = &["document", "image", "audio", "video", "other"];

/// File-family control types (`accept` applies to these only).
#[must_use]
pub fn is_file_kind(kind: &str) -> bool {
    matches!(kind, "file" | "file-array")
}

/// Reserved handle names (contracts.md C1.3).
pub const H_IN: &str = "in";
pub const H_OUT: &str = "out";
pub const H_ERROR_OUT: &str = "error_out";

/// Known node types for v1.
pub const T_START: &str = "start";
pub const T_END: &str = "end";
pub const T_SCRIPT: &str = "script";
pub const T_EGRESS: &str = "egress";
pub const T_BRANCH: &str = "branch";
pub const T_AWAIT: &str = "await";
pub const T_LLM: &str = "llm";

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct StartParam {
    /// Variable key of the start namespace (v2: was `name` — one word, one job).
    pub variable: String,
    #[serde(default)]
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub default: Option<Value>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub max_length: Option<i64>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub options: Option<Vec<Value>>,
    /// File-category constraint, required for `file` / `file-array` params:
    /// a non-empty subset of document|image|audio|video|other — multi-select
    /// (e.g. documents AND images) is allowed (v2 — no "any" bucket).
    #[serde(default)]
    pub accept: Option<Vec<String>>,
    /// Max number of files; `file-array` only, integer ≥ 1 when present.
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub max_count: Option<i64>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct StartConfig {
    #[serde(default)]
    pub params: Vec<StartParam>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct EndOutput {
    /// Result key in the final run output (v2: was `name`).
    pub key: String,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub value: Value,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct EndConfig {
    #[serde(default)]
    pub outputs: Vec<EndOutput>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct SandboxLimits {
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub memory_mb: Option<i64>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HostPermissions {
    /// Outbound api-client keys the script may call via `egress.call` / `host.callApi`.
    #[serde(default)]
    pub call_api: Option<Vec<String>>,
    /// Content types (plural names) the script may touch via `ct.*` host APIs
    /// (`*` = all; empty/absent = denied).
    #[serde(default)]
    pub content_types: Option<Vec<String>>,
    /// Raw SQL tables (read-only / read-write forms) via the `db` host API.
    #[serde(default)]
    pub database: Option<Vec<String>>,
    /// Raw HTTP domain whitelist (`*.example.com`, `api.example.com/*`).
    #[serde(default)]
    pub http: Option<Vec<String>>,
    /// Session-token actions (`issue`/`verify`).
    #[serde(default)]
    pub session: Option<Vec<String>>,
    /// Presence actions (`available`/`status`/`report`).
    #[serde(default)]
    pub presence: Option<Vec<String>>,
    #[serde(default)]
    pub data: Option<bool>,
    #[serde(default)]
    pub emit: Option<bool>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct ScriptConfig {
    pub language: String,
    #[serde(default)]
    pub code: String,
    #[serde(default)]
    pub plugin_id: Option<String>,
    #[serde(default)]
    pub fn_name: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub input: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub output_schema: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    pub sandbox: Option<SandboxLimits>,
    #[serde(default)]
    pub host_permissions: Option<HostPermissions>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct EgressConfig {
    pub client_key: String,
    #[serde(default)]
    pub op: String,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub input: Option<serde_json::Map<String, Value>>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub output_schema: Option<serde_json::Map<String, Value>>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct BranchRule {
    #[serde(default)]
    pub label: String,
    /// Structured condition or expression string.
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub when: Value,
    #[serde(default)]
    pub handle: Option<String>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct BranchConfig {
    #[serde(default)]
    pub branches: Vec<BranchRule>,
    #[serde(default)]
    pub else_handle: Option<String>,
}

#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct AwaitConfig {
    pub kind: String,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub form: Option<Value>,
    #[serde(default)]
    pub approvers: Option<String>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub timeout_secs: Option<i64>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub events: Option<Vec<Value>>,
}

/// One chat message of an `llm` node: `text` is a C3.1 template (`{{#ns.name#}}`).
#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct LlmMessage {
    pub role: String,
    pub text: String,
}

/// `llm` node config (llm-node.md §2). Error handling stays orthogonal via
/// `modifiers.on_error_strategy` (C1.4); the node ignores engine-fed `input`
/// (variables are read from the pool through message templates).
#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[derive(Debug, Clone, Deserialize)]
pub struct LlmConfig {
    pub model: Option<String>,
    #[serde(default)]
    pub messages: Vec<LlmMessage>,
    pub temperature: Option<f64>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub max_tokens: Option<i64>,
    #[serde(default)]
    pub stop: Option<Vec<String>>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "number"))]
    pub timeout_ms: Option<i64>,
    #[serde(default)]
    #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
    pub json_schema: Option<Value>,
}

/// Declared output fields of a node (v2 D2): what the node writes into its
/// pool namespace. Drives skip-null semantics (D6) and lint law 3 (D4).
///
/// - `start`  → declared start variables
/// - `script` → `output_schema` properties (empty when undeclared → law-3 skip)
/// - `egress` → fixed `response`
/// - `llm`    → fixed `text`/`structured`/`usage`/`latency_ms`
/// - `await`  → fixed `resume`
/// - `branch` → fixed `handle`
#[must_use]
pub fn declared_output_fields(kind: &str, config: &Value) -> Vec<String> {
    match kind {
        T_START => {
            let Ok(c) = serde_json::from_value::<StartConfig>(config.clone()) else {
                return Vec::new();
            };
            c.params.iter().map(|p| p.variable.clone()).collect()
        }
        T_SCRIPT => {
            let Ok(c) = serde_json::from_value::<ScriptConfig>(config.clone()) else {
                return Vec::new();
            };
            c.output_schema
                .as_ref()
                .map(|m| m.keys().cloned().collect())
                .unwrap_or_default()
        }
        T_EGRESS => vec!["response".into()],
        T_LLM => vec![
            "text".into(),
            "structured".into(),
            "usage".into(),
            "latency_ms".into(),
        ],
        T_AWAIT => vec!["resume".into()],
        T_BRANCH => vec!["handle".into()],
        _ => Vec::new(),
    }
}

/// Shallow JSON-Schema check shared by script output validation and the llm
/// structured output path: only `type` and `required` (recursive for nested
/// objects); other keywords ignored (v2 D8 — one validator, no second dialect).
pub fn shallow_schema_check(value: &Value, schema: &Value) -> Result<(), String> {
    if let Some(want) = schema.get("type").and_then(Value::as_str) {
        let ok = match (want, value) {
            ("object", Value::Object(_))
            | ("array", Value::Array(_))
            | ("string", Value::String(_))
            | ("boolean", Value::Bool(_))
            | ("null", Value::Null) => true,
            ("number", Value::Number(_)) => true,
            ("integer", Value::Number(n)) => n.is_i64() || n.is_u64(),
            _ => false,
        };
        if !ok {
            return Err(format!("类型不匹配: 期望 {want}"));
        }
    }
    if let (Value::Object(map), Some(required)) =
        (value, schema.get("required").and_then(Value::as_array))
    {
        for key in required {
            if let Some(k) = key.as_str()
                && !map.contains_key(k)
            {
                return Err(format!("缺少必填字段: {k}"));
            }
        }
    }
    if let (Value::Object(map), Some(props)) =
        (value, schema.get("properties").and_then(Value::as_object))
    {
        for (k, sub) in props {
            if let Some(v) = map.get(k)
                && let Err(e) = shallow_schema_check(v, sub)
            {
                return Err(format!("字段 {k}: {e}"));
            }
        }
    }
    Ok(())
}

/// Validate an input value: scalar (literal shorthand) OR exactly one of
/// `{literal|ref|expr}`. `ref` must be an array of strings.
pub fn validate_value_expr(where_: &str, v: &Value) -> AppResult<()> {
    if v.is_object() {
        let obj = v.as_object().unwrap();
        let keys: Vec<&String> = obj.keys().collect();
        let variant = keys
            .iter()
            .find(|k| k.as_str() == "literal" || k.as_str() == "ref" || k.as_str() == "expr");
        if let Some(k) = variant {
            if keys.len() > 1 {
                return Err(AppError::BadRequest(format!(
                    "{where_}: ValueExpr {v} 只能含一个键 (literal|ref|expr)"
                )));
            }
            if k.as_str() == "ref" {
                let arr = obj.get("ref").and_then(Value::as_array).ok_or_else(|| {
                    AppError::BadRequest(format!("{where_}: ref 必须是字符串数组"))
                })?;
                if arr.iter().any(|s| !s.is_string()) {
                    return Err(AppError::BadRequest(format!(
                        "{where_}: ref 元素必须是字符串"
                    )));
                }
            }
        }
    }
    Ok(())
}

fn validate_input_map(
    where_: &str,
    input: Option<&serde_json::Map<String, Value>>,
) -> AppResult<()> {
    if let Some(map) = input {
        for (k, v) in map {
            validate_value_expr(&format!("{where_}.input.{k}"), v)?;
        }
    }
    Ok(())
}

/// Deserialize + validate a node config against its known schema. Unknown type
/// or version → `BadRequest`.
pub fn validate_node(kind: &str, _version: i64, config: &Value) -> AppResult<()> {
    let type_error =
        |e: serde_json::Error| AppError::BadRequest(format!("node '{kind}' config invalid: {e}"));
    match kind {
        T_START => {
            let c: StartConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
            for p in &c.params {
                if p.variable.is_empty() {
                    return Err(AppError::BadRequest(
                        "start.params[].variable 不能为空".into(),
                    ));
                }
                if p.max_length.is_some_and(|ml| ml < 1) {
                    return Err(AppError::BadRequest(format!(
                        "start.params['{}'].max_length 须为 ≥1 的整数",
                        p.variable
                    )));
                }
                if !START_PARAM_TYPES.contains(&p.kind.as_str()) {
                    return Err(AppError::BadRequest(format!(
                        "start.params['{}'].type '{}' 非法（允许: {}）",
                        p.variable,
                        p.kind,
                        START_PARAM_TYPES.join(" | ")
                    )));
                }
                if is_file_kind(&p.kind) {
                    let accepts = p.accept.as_deref().unwrap_or(&[]);
                    let valid = !accepts.is_empty()
                        && accepts
                            .iter()
                            .all(|a| FILE_ACCEPT_TYPES.contains(&a.as_str()));
                    if !valid {
                        return Err(AppError::BadRequest(format!(
                            "start.params['{}'].accept 须为 {} 的非空子集（可多选，不允许任意文件）",
                            p.variable,
                            FILE_ACCEPT_TYPES.join(" | ")
                        )));
                    }
                } else if p.accept.is_some() {
                    return Err(AppError::BadRequest(format!(
                        "start.params['{}'].accept 仅用于 file/file-array 类型",
                        p.variable
                    )));
                }
                if p.max_count.is_some() && p.kind != "file-array" {
                    return Err(AppError::BadRequest(format!(
                        "start.params['{}'].max_count 仅用于 file-array 类型",
                        p.variable
                    )));
                }
                if p.max_count.is_some_and(|c| c < 1) {
                    return Err(AppError::BadRequest(format!(
                        "start.params['{}'].max_count 须为 ≥1 的整数",
                        p.variable
                    )));
                }
            }
        }
        T_END => {
            let _c: EndConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
        }
        T_SCRIPT => {
            let c: ScriptConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
            if c.code.is_empty() && c.plugin_id.is_none() {
                return Err(AppError::BadRequest(
                    "script: code 与 plugin_id 至少给一个".into(),
                ));
            }
            validate_input_map("script", c.input.as_ref())?;
        }
        T_EGRESS => {
            let c: EgressConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
            if c.client_key.is_empty() || c.op.is_empty() {
                return Err(AppError::BadRequest("egress: client_key 与 op 必填".into()));
            }
            validate_input_map("egress", c.input.as_ref())?;
        }
        T_BRANCH => {
            let c: BranchConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
            if c.branches.is_empty() {
                return Err(AppError::BadRequest("branch: 至少一个 branches".into()));
            }
        }
        T_AWAIT => {
            let _c: AwaitConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
        }
        T_LLM => {
            let c: LlmConfig = serde_json::from_value(config.clone()).map_err(type_error)?;
            if c.messages.is_empty() {
                return Err(AppError::BadRequest(
                    "llm: messages 不能为空且需至少一条 user".into(),
                ));
            }
            if c.messages[0].role == "system" && c.messages.len() == 1 {
                return Err(AppError::BadRequest(
                    "llm: messages 不能为空且需至少一条 user".into(),
                ));
            }
            if !c.messages.iter().any(|m| m.role == "user") {
                return Err(AppError::BadRequest(
                    "llm: messages 不能为空且需至少一条 user".into(),
                ));
            }
            for m in &c.messages {
                if !matches!(m.role.as_str(), "system" | "user" | "assistant") {
                    return Err(AppError::BadRequest(format!(
                        "llm: role '{}' 非法 (system|user|assistant)",
                        m.role
                    )));
                }
                if m.text.trim().is_empty() {
                    return Err(AppError::BadRequest(format!(
                        "llm: messages[{}] text 不能为空",
                        m.role
                    )));
                }
            }
            if let Some(t) = c.temperature
                && !(0.0..=2.0).contains(&t)
            {
                return Err(AppError::BadRequest("llm: temperature 须在 [0,2]".into()));
            }
            if c.max_tokens.is_some_and(|t| t <= 0) {
                return Err(AppError::BadRequest("llm: max_tokens 须 > 0".into()));
            }
            if c.stop.as_ref().is_some_and(|s| s.len() > 4) {
                return Err(AppError::BadRequest("llm: stop 最多 4 条".into()));
            }
            if let Some(schema) = &c.json_schema
                && !(schema.is_object()
                    && schema.get("type").and_then(Value::as_str) == Some("object"))
            {
                return Err(AppError::BadRequest(
                    "llm: json_schema 须为 {\"type\":\"object\",...}".into(),
                ));
            }
        }
        other => {
            return Err(AppError::BadRequest(format!(
                "node type '{other}' not supported (v1: start|end|script|egress|branch)"
            )));
        }
    }
    Ok(())
}

/// Node type string union (TS literal union; wire = `data.type`).
#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-types", ts(rename_all = "lowercase"))]
#[allow(dead_code)]
pub enum NodeKind {
    Start,
    End,
    Script,
    Egress,
    Branch,
    Await,
    Llm,
}

/// TS-only union of every node's config shape (editor drives panels off it).
/// Discriminator lives at `node.data.type`; config payloads are the inner
/// object (no tag inside).
#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-types", ts(untagged))]
#[allow(dead_code)]
#[allow(clippy::large_enum_variant)]
pub enum NodeConfigVariant {
    Start(StartConfig),
    End(EndConfig),
    Script(ScriptConfig),
    Egress(EgressConfig),
    Branch(BranchConfig),
    Await(AwaitConfig),
    Llm(LlmConfig),
}

/// TS-only union for ValueExpr (literal | ref selector | expr string).
#[cfg_attr(feature = "export-types", derive(ts_rs::TS))]
#[cfg_attr(feature = "export-types", ts(untagged))]
#[allow(dead_code)]
pub enum ValueExpr {
    Literal {
        #[cfg_attr(feature = "export-types", ts(type = "unknown"))]
        literal: Value,
    },
    Ref {
        #[cfg_attr(feature = "export-types", ts(rename = "ref"))]
        ref_: Vec<String>,
    },
    Expr {
        expr: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn validates_known_types_and_rejects_unknown() {
        assert!(validate_node(T_SCRIPT, 1, &json!({"language": "js", "code": "return 1"})).is_ok());
        assert!(
            validate_node(T_SCRIPT, 1, &json!({"language": "js"})).is_err(),
            "无 code/plugin"
        );
        assert!(validate_node(T_EGRESS, 1, &json!({"client_key": "llm", "op": "chat"})).is_ok());
        assert!(validate_node("nope", 1, &json!({})).is_err(), "未知 type");
    }

    #[test]
    fn value_expr_shapes() {
        assert!(validate_value_expr("x", &json!({"ref": ["start", "msg"]})).is_ok());
        assert!(validate_value_expr("x", &json!({"literal": 5})).is_ok());
        assert!(validate_value_expr("x", &json!({"expr": "{{#start.msg#}}.length > 0"})).is_ok());
        assert!(validate_value_expr("x", &json!({"ref": "start"})).is_err());
        assert!(
            validate_value_expr("x", &json!({"ref": ["a"], "literal": 1})).is_err(),
            "多键"
        );
    }

    #[test]
    fn start_param_max_length_bounds() {
        let ok = json!({
            "params": [
                {"variable": "q", "label": "Q", "type": "text", "required": true, "max_length": 100}
            ]
        });
        assert!(validate_node(T_START, 1, &ok).is_ok());

        for bad in [0, -5] {
            let cfg = json!({
                "params": [
                    {"variable": "q", "label": "Q", "type": "text", "max_length": bad}
                ]
            });
            let err = validate_node(T_START, 1, &cfg).unwrap_err();
            assert!(err.to_string().contains("max_length"), "{err}");
        }

        // fractional input is rejected at deserialization (i64)
        let frac = json!({
            "params": [
                {"variable": "q", "label": "Q", "type": "text", "max_length": 1.5}
            ]
        });
        assert!(validate_node(T_START, 1, &frac).is_err());
    }

    #[test]
    fn start_param_file_accept_rules() {
        // file without accept -> rejected (no "any" files)
        let bad = json!({
            "params": [{"variable": "f", "label": "F", "type": "file", "required": true}]
        });
        let err = validate_node(T_START, 1, &bad).unwrap_err();
        assert!(err.to_string().contains("accept"), "{err}");

        // file-array + multi-category accept -> ok
        let ok = json!({
            "params": [
                {"variable": "imgs", "label": "Images", "type": "file-array", "accept": ["image", "document"]}
            ]
        });
        assert!(validate_node(T_START, 1, &ok).is_ok());

        // empty accept array -> rejected
        let empty = json!({
            "params": [{"variable": "f", "label": "F", "type": "file", "accept": []}]
        });
        assert!(validate_node(T_START, 1, &empty).is_err());

        // accept outside the 5 categories -> rejected
        let bad2 = json!({
            "params": [{"variable": "f", "label": "F", "type": "file", "accept": ["anything"]}]
        });
        assert!(validate_node(T_START, 1, &bad2).is_err());

        // accept on non-file type -> rejected
        let bad3 = json!({
            "params": [{"variable": "q", "label": "Q", "type": "text", "accept": "image"}]
        });
        assert!(validate_node(T_START, 1, &bad3).is_err());

        // legacy control type renamed away -> rejected
        let bad4 = json!({
            "params": [{"variable": "q", "label": "Q", "type": "text-input"}]
        });
        assert!(validate_node(T_START, 1, &bad4).is_err());
    }

    #[test]
    fn llm_config_validation() {
        let ok = json!({
            "messages": [
                {"role": "system", "text": "你是助手"},
                {"role": "user", "text": "hi {{#start.q#}}"}
            ],
            "temperature": 0.3, "max_tokens": 100, "stop": ["\n"]
        });
        assert!(validate_node(T_LLM, 1, &ok).is_ok());

        assert!(
            validate_node(T_LLM, 1, &json!({"messages": []})).is_err(),
            "空 messages"
        );
        assert!(
            validate_node(
                T_LLM,
                1,
                &json!({"messages": [{"role": "system", "text": "only sys"}]})
            )
            .is_err(),
            "只有 system 无 user"
        );
        assert!(
            validate_node(
                T_LLM,
                1,
                &json!({"messages": [{"role": "tool", "text": "x"}]})
            )
            .is_err(),
            "非法 role"
        );
        assert!(
            validate_node(
                T_LLM,
                1,
                &json!({"messages": [{"role": "user", "text": "x"}], "temperature": 5.0})
            )
            .is_err(),
            "temperature 越界"
        );
        assert!(
            validate_node(
                T_LLM,
                1,
                &json!({"messages": [{"role": "user", "text": "x"}], "stop": ["a","b","c","d","e"]})
            )
            .is_err(),
            "stop 超 4 条"
        );
        assert!(
            validate_node(
                T_LLM,
                1,
                &json!({
                    "messages": [{"role": "user", "text": "x"}],
                    "json_schema": {"type": "array"}
                })
            )
            .is_err(),
            "json_schema 非 object"
        );
    }
}
