//! Escalation report — a pure projection of the dispute ledger
//! (multi-agent §6.6). Zero LLM, zero information loss: every line below is
//! mechanically derived from ledger state, so the report can never invent
//! anything the debate did not say.
//!
//! Four sections (external review template, 2026-09-24):
//! 一、需你拍板（severity 降序，带建议/代价/影响面）；
//! 二、已修复（附依据）；三、已驳回（附证据）；四、按默认规则继续。

use super::ledger::{Dispute, DisputeStatus, Ledger, Nature, ResolvedBy, Severity};

/// Render the four-section markdown report.
pub fn render(requirement: &str, ledger: &Ledger) -> String {
    let mut out = String::new();
    out.push_str("# 需求评审报告\n\n## 原始需求\n");
    out.push_str(requirement.trim());
    out.push_str("\n\n");

    render_pending(&mut out, ledger);
    render_improved(&mut out, ledger);
    render_upheld(&mut out, ledger);
    render_default_rule(&mut out, ledger);
    out
}

/// Section 一：disputes waiting for a human verdict, severity descending.
fn render_pending(out: &mut String, ledger: &Ledger) {
    let mut pending: Vec<&Dispute> = ledger
        .disputes
        .iter()
        .filter(|d| d.status == DisputeStatus::Escalated)
        .collect();
    if pending.is_empty() {
        out.push_str("## 一、需你拍板\n\n（无——所有分歧已在辩论中解决）\n\n");
        return;
    }
    pending.sort_by_key(|d| std::cmp::Reverse(d.severity));
    out.push_str("## 一、需你拍板（severity 降序）\n\n");
    for d in pending {
        out.push_str(&format!(
            "### {}: {}（{:?} · {:?}）\n",
            d.id, d.title, d.kind, d.severity
        ));
        // B's latest argument + sources.
        if let Some(c) = d.challenges.last() {
            out.push_str(&format!("- 分歧（B）：{}\n", c.argument));
            out.push_str(&format!("  - 依据：{}\n", join_sources(&c.sources)));
        }
        // A's latest counter-argument (if any round rejected).
        if let Some(r) = d.responses.last() {
            out.push_str(&format!(
                "- A 回应（第 {} 轮，{:?}）：{}\n  - 依据：{}\n",
                r.round,
                r.stance,
                r.reasoning,
                join_sources(&r.sources)
            ));
        }
        // Nature — factual conflicts get the dedicated label (§6.5). The
        // brief's final classification wins; B's initial guess is the fallback.
        match (d.nature, d.brief.as_ref().and_then(|b| b.nature)) {
            (Some(n), _) | (None, Some(n)) if n == Nature::Factual => {
                out.push_str(
                    "- 性质：**事实分歧（证据冲突）**——双方证据不一致，需裁决数据源或授权核验\n",
                );
            }
            (Some(n), _) => out.push_str(&format!("- 性质：{}\n", nature_label(n))),
            (None, Some(n)) => out.push_str(&format!("- 性质：{}\n", nature_label(n))),
            (None, None) => out.push_str("- 性质：未判定\n"),
        }
        // Brief: recommendation / cost / impact (may be absent on failure).
        match d.brief.as_ref() {
            Some(b) => {
                out.push_str(&format!("- 建议：{}\n", b.recommendation));
                out.push_str(&format!("- 影响面：{}\n", b.impact));
                out.push_str(&format!("- 若选 A 的代价：{}\n", b.cost_if_a));
                out.push_str(&format!("- 若选 B 的代价：{}\n", b.cost_if_b));
            }
            None => out.push_str("- 建议：缺失（brief 生成失败，请依据上方论据自行判断）\n"),
        }
        out.push('\n');
    }
}

/// Section 二：accepted + revised, with the change note as evidence.
fn render_improved(out: &mut String, ledger: &Ledger) {
    let items: Vec<&Dispute> = ledger
        .disputes
        .iter()
        .filter(|d| d.status == DisputeStatus::Improved)
        .collect();
    if items.is_empty() {
        return;
    }
    out.push_str("## 二、已修复（附依据）\n\n");
    for d in items {
        let note = d
            .challenges
            .first()
            .map(|c| c.argument.clone())
            .unwrap_or_default();
        // The revision note lives on the target item's change_note.
        let change = ledger
            .items
            .iter()
            .find(|i| i.id == d.target)
            .and_then(|i| i.change_note.clone());
        out.push_str(&format!(
            "- {}：{} → 已采纳并修订（依据：{}；修订记录：{}）\n",
            d.id,
            d.title,
            note,
            change.unwrap_or_else(|| "（无备注）".into())
        ));
    }
    out.push('\n');
}

/// Section 三：rejected with reasons and B withdrew (or verdict sided A).
fn render_upheld(out: &mut String, ledger: &Ledger) {
    let items: Vec<&Dispute> = ledger
        .disputes
        .iter()
        .filter(|d| d.status == DisputeStatus::Upheld)
        .collect();
    if items.is_empty() {
        return;
    }
    out.push_str("## 三、已驳回（附证据）\n\n");
    for d in items {
        let default_rule = d.resolved_by == Some(ResolvedBy::DefaultRule);
        if default_rule {
            continue; // rendered in section 四
        }
        let reason = d
            .responses
            .last()
            .map(|r| format!("{}（依据：{}）", r.reasoning, join_sources(&r.sources)))
            .unwrap_or_else(|| "（无记录）".into());
        let withdraw = match d.withdraw_round {
            Some(r) => format!("；B 第 {r} 轮 withdraw"),
            None => String::new(),
        };
        out.push_str(&format!(
            "- {}：{} → 驳回（{}{}）\n",
            d.id, d.title, reason, withdraw
        ));
    }
    out.push('\n');
}

/// Section 四：minor disputes closed by the default rule without a human.
fn render_default_rule(out: &mut String, ledger: &Ledger) {
    let items: Vec<&Dispute> = ledger
        .disputes
        .iter()
        .filter(|d| {
            d.status == DisputeStatus::Upheld && d.resolved_by == Some(ResolvedBy::DefaultRule)
        })
        .collect();
    if items.is_empty() {
        return;
    }
    out.push_str("## 四、按默认规则继续（未上抛）\n\n");
    for d in items {
        out.push_str(&format!(
            "- {}（{:?}·{:?}）：取提案方版本（{}）\n",
            d.id,
            d.severity,
            d.kind,
            d.rule_id.as_deref().unwrap_or("default")
        ));
    }
    out.push('\n');
}

fn join_sources(sources: &[String]) -> String {
    if sources.is_empty() {
        "（无）".into()
    } else {
        sources.join("；")
    }
}

fn nature_label(n: Nature) -> &'static str {
    match n {
        Nature::Factual => "事实分歧（可验证）",
        Nature::Value => "价值/偏好分歧",
        Nature::Ambiguity => "需求歧义（需改写）",
    }
}

/// Severity ordering helper for tests.
#[allow(dead_code)]
pub fn severity_rank(s: Severity) -> u8 {
    match s {
        Severity::Minor => 0,
        Severity::Major => 1,
        Severity::Blocker => 2,
    }
}
