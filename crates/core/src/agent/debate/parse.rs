//! Round-output parsing: lenient JSON extraction + defensive schema checks
//! (multi-agent §6.2-§6.3).
//!
//! `[照抄 kb/distill.rs parse_json_object]` — models wrap JSON in prose or
//! code fences; we slice the outermost braces. Validation failures carry a
//! model-readable reason so the orchestrator can run exactly one repair
//! retry (claw-code `InvalidArgumentsError` semantics) before degrading.

use serde::Deserialize;

use super::ledger::{ChallengeKind, Nature, RequirementItem, Severity, Stance};
/// R0 itemization output (proposer, first round).
#[derive(Debug, Deserialize)]
pub struct R0Output {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub items: Vec<RequirementItem>,
}

/// B's per-round challenge payload (validated form).
#[derive(Debug, Deserialize)]
pub struct ChallengePayload {
    pub target: String,
    #[serde(rename = "type", alias = "kind")]
    pub kind: ChallengeKind,
    pub severity: Severity,
    #[serde(default)]
    pub nature: Option<Nature>,
    pub evidence: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub proposed_resolution: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReVisitPayload {
    pub dispute_id: String,
    /// MVP: `withdraw` only — reinforcing a closed dispute is rejected by
    /// ledger rules (anti-nitpick lock, multi-agent §6.3).
    pub action: String,
    #[serde(default)]
    pub argument: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ChallengeRound {
    #[serde(default)]
    pub coverage: Vec<String>,
    #[serde(default)]
    pub new_challenges: Vec<ChallengePayload>,
    #[serde(default)]
    pub re_visits: Vec<ReVisitPayload>,
    #[serde(default)]
    pub no_issues: Option<String>,
}

/// A's per-dispute response payload (validated form).
#[derive(Debug, Deserialize)]
pub struct DefensePayload {
    pub dispute_id: String,
    pub stance: Stance,
    #[serde(default)]
    pub reasoning: String,
    #[serde(default)]
    pub sources: Vec<String>,
    #[serde(default)]
    pub revision_targets: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DefenseRound {
    #[serde(default)]
    pub responses: Vec<DefensePayload>,
    #[serde(default)]
    pub revised_items: Vec<RequirementItem>,
}

/// Slice the outermost `{...}` out of arbitrary model prose and decode.
/// `[照抄 kb/distill.rs parse_json_object]`.
fn parse_json_object<T: serde::de::DeserializeOwned>(text: &str) -> Option<T> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    if end < start {
        return None;
    }
    serde_json::from_str(&text[start..=end]).ok()
}

/// Parse + validate the R0 itemization. `Err` carries the repair instruction.
pub fn parse_r0(text: &str) -> Result<R0Output, String> {
    let out: R0Output = parse_json_object(text).ok_or_else(|| {
        "输出不是合法 JSON：请只输出一个 JSON 对象（以 { 开头、} 结尾）".to_string()
    })?;
    if out.items.is_empty() {
        return Err("items 为空：至少需要 1 条需求条目".into());
    }
    let mut seen = std::collections::HashSet::new();
    for it in &out.items {
        if !it.id.starts_with("REQ") || !seen.insert(it.id.clone()) {
            return Err(format!(
                "条目 id 非法或重复：{}（必须形如 REQ-1 且唯一）",
                it.id
            ));
        }
        if it.criteria.is_empty() {
            return Err(format!("条目 {} 的 criteria 为空", it.id));
        }
    }
    Ok(out)
}

/// Parse + validate B's round. `open_ids` = disputes currently open;
/// `item_ids` = valid anchor ids plus `summary`.
pub fn parse_challenge(
    text: &str,
    open_ids: &[String],
    item_ids: &[String],
    first_round: bool,
) -> Result<ChallengeRound, String> {
    let out: ChallengeRound = parse_json_object(text)
        .ok_or_else(|| "输出不是合法 JSON：请只输出一个 JSON 对象".to_string())?;

    for c in &out.new_challenges {
        if c.evidence.trim().is_empty() {
            return Err(format!(
                "challenge（target={}）缺少 evidence：没有论证的质疑无效",
                c.target
            ));
        }
        if c.sources.is_empty() {
            return Err(format!(
                "challenge（target={}）缺少 sources：必须给出依据出处",
                c.target
            ));
        }
        if !item_ids.iter().any(|i| i == &c.target) {
            return Err(format!(
                "challenge target {} 不存在：必须是 {} 之一",
                c.target,
                item_ids.join(", ")
            ));
        }
    }
    for r in &out.re_visits {
        if r.action != "withdraw" {
            return Err(format!(
                "re_visits 只支持 withdraw（本阶段不支持 reinforce）：收到 {}",
                r.action
            ));
        }
        if !open_ids
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&r.dispute_id))
        {
            return Err(format!(
                "re_visits 的 dispute_id {} 不是 open 状态，不可操作",
                r.dispute_id
            ));
        }
        if r.argument.as_deref().unwrap_or("").trim().is_empty() {
            return Err(format!("withdraw {} 必须说明接受理由", r.dispute_id));
        }
    }
    if first_round
        && out.new_challenges.is_empty()
        && out.no_issues.as_deref().unwrap_or("").trim().is_empty()
    {
        return Err("首轮必须至少提出 1 条 challenge，或给出 no_issues 说明".into());
    }
    Ok(out)
}

/// Parse + validate A's round. `open_ids` = disputes awaiting response.
pub fn parse_defense(text: &str, open_ids: &[String]) -> Result<DefenseRound, String> {
    let out: DefenseRound = parse_json_object(text)
        .ok_or_else(|| "输出不是合法 JSON：请只输出一个 JSON 对象".to_string())?;

    let mut answered = std::collections::HashSet::new();
    for r in &out.responses {
        if !answered.insert(r.dispute_id.to_ascii_lowercase()) {
            return Err(format!("dispute {} 被回应了多次", r.dispute_id));
        }
        if !open_ids
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&r.dispute_id))
        {
            return Err(format!("dispute {} 不是 open 状态，无需回应", r.dispute_id));
        }
        if r.reasoning.trim().is_empty() {
            return Err(format!("dispute {} 缺少 reasoning", r.dispute_id));
        }
        if r.stance != Stance::Accept && r.sources.is_empty() {
            return Err(format!(
                "dispute {} 的 stance={} 时 sources 必填（驳回必须给依据）",
                r.dispute_id,
                match r.stance {
                    Stance::Reject => "reject",
                    Stance::Partial => "partial",
                    Stance::Accept => "accept",
                }
            ));
        }
    }
    for missed in open_ids {
        if !answered.contains(&missed.to_ascii_lowercase()) {
            return Err(format!("漏掉了对 open dispute {} 的回应", missed));
        }
    }
    for it in &out.revised_items {
        if it.change_note.as_deref().unwrap_or("").trim().is_empty() {
            return Err(format!(
                "revised_items {} 缺少 change_note：必须写明改了什么、依据哪条分歧",
                it.id
            ));
        }
    }
    Ok(out)
}

/// One dispute's decision brief payload (multi-agent §6.7).
#[derive(Debug, Deserialize)]
pub struct BriefPayload {
    pub dispute_id: String,
    #[serde(default)]
    pub nature: Option<Nature>,
    #[serde(default)]
    pub recommendation: String,
    #[serde(default)]
    pub cost_if_a: String,
    #[serde(default)]
    pub cost_if_b: String,
    #[serde(default)]
    pub impact: String,
}

#[derive(Debug, Deserialize)]
pub struct BriefRound {
    #[serde(default)]
    pub briefs: Vec<BriefPayload>,
}

/// Parse + validate the escalation brief. Every escalated dispute must be
/// covered; fields may be empty (report marks them 缺失) but ids must match.
pub fn parse_brief(text: &str, expected_ids: &[String]) -> Result<BriefRound, String> {
    let out: BriefRound = parse_json_object(text)
        .ok_or_else(|| "输出不是合法 JSON：请只输出一个 JSON 对象".to_string())?;
    for b in &out.briefs {
        if !expected_ids
            .iter()
            .any(|i| i.eq_ignore_ascii_case(&b.dispute_id))
        {
            return Err(format!(
                "brief 的 dispute_id {} 不在待判列表（{:?}）中",
                b.dispute_id, expected_ids
            ));
        }
    }
    for id in expected_ids {
        if !out
            .briefs
            .iter()
            .any(|b| b.dispute_id.eq_ignore_ascii_case(id))
        {
            return Err(format!("缺少对 dispute {} 的 brief", id));
        }
    }
    Ok(out)
}
