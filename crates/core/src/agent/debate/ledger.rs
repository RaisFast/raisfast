//! Dispute ledger — the debate's single source of state
//! (`dev-docs/agent/multi-agent.md` §6.4).
//!
//! Rounds exchange the ledger snapshot, never the raw transcript, so per-round
//! input stays bounded and termination/report projection is deterministic
//! Rust. `nature` (factual/value/ambiguity) describes the *resolution path*;
//! `kind` describes the *problem category* (Kiro's five) — orthogonal axes.

use serde::{Deserialize, Serialize};

use crate::types::snowflake_id::SnowflakeId;

/// Ledger container persisted as the `ai_debates.ledger` JSON column.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Ledger {
    /// Schema version for forward-compatible evolution.
    pub version: u32,
    /// Itemized requirement set produced by R0 (`REQ-n` ids are the anchor
    /// every dispute `target` points at).
    #[serde(default)]
    pub items: Vec<RequirementItem>,
    #[serde(default)]
    pub disputes: Vec<Dispute>,
    /// Append-only per-round bookkeeping (refs + transition counts).
    #[serde(default)]
    pub rounds_log: Vec<RoundLog>,
}

/// One itemized requirement (EARS-lite, multi-agent §6.2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequirementItem {
    /// Stable anchor id, e.g. `REQ-1`.
    pub id: String,
    pub story: String,
    #[serde(default)]
    pub criteria: Vec<String>,
    /// Required on revisions: what changed + which dispute justifies it
    /// (feeds the report's "fixed with evidence" section).
    #[serde(default)]
    pub change_note: Option<String>,
}

/// Dispute lifecycle: `open` → `improved | upheld`; escalation path via
/// `escalated` → (human verdict) → terminal. `default_rule` closes minors
/// before humans ever see them (§6.5 escalation threshold).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisputeStatus {
    Open,
    /// Escalated to human judgment; consumed by verdicts (§8).
    Escalated,
    /// Consensus: A accepted and revised.
    Improved,
    /// Consensus: A rejected with reasons and B withdrew.
    Upheld,
}

/// How a dispute reached its terminal state (audit trail).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolvedBy {
    /// Closed inside the debate (accept/withdraw).
    Debate,
    /// Closed by the minor-dispute default rule without a human.
    DefaultRule,
    /// Closed by human verdict.
    Verdict,
}

/// Resolution path class (multi-agent §6.3): factual = verifiable by
/// evidence/recomputation, value = preference trade-off, ambiguity = the
/// requirement itself needs rewriting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Nature {
    Factual,
    Value,
    Ambiguity,
}

/// Problem category (Kiro Analyze Requirements' five; B's initial judgment).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChallengeKind {
    LogicalInconsistency,
    Ambiguity,
    ConflictingConstraints,
    UnstatedAssumption,
    MissingEdgeCase,
}

/// `blocker | major | minor` — drives report ordering and the escalation
/// threshold (only major+ reaches humans; minors auto-resolve).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Minor,
    Major,
    Blocker,
}

/// A's stance on a dispute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stance {
    Accept,
    Reject,
    Partial,
}

/// One contested point, with full history of both sides (anti-flip-flop:
/// every round's stance stays visible to both agents).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    /// Stable id, e.g. `D1` (`D{seq}`).
    pub id: String,
    pub title: String,
    /// Anchor: `REQ-n` or `summary`.
    pub target: String,
    #[serde(rename = "type")]
    pub kind: ChallengeKind,
    #[serde(default)]
    pub nature: Option<Nature>,
    pub severity: Severity,
    pub status: DisputeStatus,
    /// `None` while `open`.
    #[serde(default)]
    pub resolved_by: Option<ResolvedBy>,
    /// Id of the rule when `resolved_by = default_rule`.
    #[serde(default)]
    pub rule_id: Option<String>,
    #[serde(default)]
    pub challenges: Vec<Challenge>,
    #[serde(default)]
    pub responses: Vec<DefenseResponse>,
    /// Round in which B withdrew (accepted A's reasons) — report audit.
    #[serde(default)]
    pub withdraw_round: Option<u32>,
    /// Escalation brief (recommendation / cost / impact, §6.7).
    #[serde(default)]
    pub brief: Option<EscalationBrief>,
    /// Human judgment (§8).
    #[serde(default)]
    pub verdict: Option<Verdict>,
}

/// B's evidence-anchored challenge; `sources` are mandatory locators.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Challenge {
    pub round: u32,
    pub argument: String,
    #[serde(default)]
    pub sources: Vec<String>,
    /// B's initial nature guess; the escalation brief may finalize it.
    #[serde(default)]
    pub nature: Option<Nature>,
    #[serde(default)]
    pub proposed_resolution: Option<String>,
}

/// A's per-dispute response; reject/partial require reasoning + sources
/// (schema-validated upstream, one repair retry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DefenseResponse {
    pub round: u32,
    pub stance: Stance,
    pub reasoning: String,
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Per-side recommendation attached at escalation time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscalationBrief {
    /// Final nature classification (may override B's initial guess).
    #[serde(default)]
    pub nature: Option<Nature>,
    pub recommendation: String,
    pub cost_if_a: String,
    pub cost_if_b: String,
    pub impact: String,
}

/// Human verdict on one escalated dispute.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verdict {
    /// `side_a | side_b | custom`.
    pub verdict: String,
    /// Required when `verdict = custom`.
    #[serde(default)]
    pub resolution: Option<String>,
    #[serde(default)]
    pub note: Option<String>,
}

/// One orchestration round's bookkeeping entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoundLog {
    pub round: u32,
    /// Ledger state transitions observed this round (new disputes + stance
    /// changes + withdrawals + default-rule closures).
    pub transitions: u32,
    #[serde(default)]
    pub a_session_id: Option<SnowflakeId>,
    #[serde(default)]
    pub b_session_id: Option<SnowflakeId>,
    #[serde(default)]
    pub outcome: Option<String>,
}

impl Ledger {
    /// Seed a ledger from R0's itemized requirement.
    pub fn new(items: Vec<RequirementItem>) -> Self {
        Self {
            version: 1,
            items,
            disputes: Vec::new(),
            rounds_log: Vec::new(),
        }
    }

    /// Disputes still `open` (auto-adjudication runs before this is checked).
    pub fn open_count(&self) -> usize {
        self.disputes
            .iter()
            .filter(|d| d.status == DisputeStatus::Open)
            .count()
    }

    /// Consensus reached: nothing left open (default-rule closures included).
    pub fn all_resolved(&self) -> bool {
        self.open_count() == 0
    }

    /// Next dispute id (`D{n}`), one past the highest existing suffix.
    pub fn next_dispute_id(&self) -> String {
        let max = self
            .disputes
            .iter()
            .filter_map(|d| d.id.trim_start_matches('D').parse::<u32>().ok())
            .max()
            .unwrap_or(0);
        format!("D{}", max + 1)
    }

    /// Look up a dispute by id (case-insensitive on the `D` prefix).
    pub fn find_dispute_mut(&mut self, id: &str) -> Option<&mut Dispute> {
        let id = id.trim();
        self.disputes
            .iter_mut()
            .find(|d| d.id.eq_ignore_ascii_case(id))
    }

    /// Close every open `minor` dispute with the default rule (adopt the
    /// proposer's version). Returns how many were closed. Deterministic,
    /// runs before each terminal check (multi-agent §6.5).
    pub fn auto_resolve_minors(&mut self, rule_id: &str) -> u32 {
        let mut closed = 0;
        for d in &mut self.disputes {
            if d.status == DisputeStatus::Open && d.severity == Severity::Minor {
                d.status = DisputeStatus::Upheld;
                d.resolved_by = Some(ResolvedBy::DefaultRule);
                d.rule_id = Some(rule_id.to_string());
                closed += 1;
            }
        }
        closed
    }

    /// Escalation gate: any open dispute at `major` or above after the
    /// auto-adjudication pass.
    pub fn needs_escalation(&self) -> bool {
        self.disputes
            .iter()
            .any(|d| d.status == DisputeStatus::Open && d.severity >= Severity::Major)
    }

    /// Open disputes with a factual nature that survived evidence exchange —
    /// the report must label these "evidence conflict" (§6.5).
    pub fn factual_conflicts(&self) -> Vec<&Dispute> {
        self.disputes
            .iter()
            .filter(|d| {
                d.status == DisputeStatus::Open && matches!(d.nature, Some(Nature::Factual))
            })
            .collect()
    }

    /// Disputes awaiting human verdicts (post-escalation).
    pub fn escalated_ids(&self) -> Vec<String> {
        self.disputes
            .iter()
            .filter(|d| d.status == DisputeStatus::Escalated)
            .map(|d| d.id.clone())
            .collect()
    }

    /// Mark every `open` dispute `escalated` (debate-level escalation, §7).
    /// Returns how many were marked.
    pub fn mark_all_escalated(&mut self) -> u32 {
        let mut n = 0;
        for d in &mut self.disputes {
            if d.status == DisputeStatus::Open {
                d.status = DisputeStatus::Escalated;
                n += 1;
            }
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str) -> RequirementItem {
        RequirementItem {
            id: id.to_string(),
            story: "As a user, I want export".into(),
            criteria: vec!["WHEN export THE SYSTEM SHALL skip drafts".into()],
            change_note: None,
        }
    }

    fn dispute(id: &str, severity: Severity) -> Dispute {
        Dispute {
            id: id.to_string(),
            title: format!("{id} title"),
            target: "REQ-1".into(),
            kind: ChallengeKind::Ambiguity,
            nature: Some(Nature::Factual),
            severity,
            status: DisputeStatus::Open,
            resolved_by: None,
            rule_id: None,
            challenges: vec![Challenge {
                round: 1,
                argument: "drafts ambiguity".into(),
                sources: vec!["REQ-1".into()],
                nature: Some(Nature::Ambiguity),
                proposed_resolution: Some("exclude drafts".into()),
            }],
            responses: Vec::new(),
            withdraw_round: None,
            brief: None,
            verdict: None,
        }
    }

    #[test]
    fn serde_roundtrip_snake_case() {
        let mut ledger = Ledger::new(vec![item("REQ-1")]);
        ledger.disputes.push(dispute("D1", Severity::Blocker));
        let json = serde_json::to_string(&ledger).expect("serialize");
        assert!(json.contains("\"logical_inconsistency\"") || json.contains("\"ambiguity\""));
        assert!(!json.contains("\"missing_edge_case\""));
        let back: Ledger = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.disputes[0].severity, Severity::Blocker);
        assert_eq!(back.items[0].id, "REQ-1");
    }

    #[test]
    fn next_dispute_id_increments_past_max() {
        let mut ledger = Ledger::new(vec![item("REQ-1")]);
        ledger.disputes.push(dispute("D2", Severity::Minor));
        assert_eq!(ledger.next_dispute_id(), "D3");
        assert_eq!(Ledger::default().next_dispute_id(), "D1");
    }

    #[test]
    fn auto_resolve_closes_only_open_minors() {
        let mut ledger = Ledger::new(vec![item("REQ-1")]);
        let mut minor = dispute("D1", Severity::Minor);
        minor.nature = Some(Nature::Ambiguity);
        let mut major = dispute("D2", Severity::Major);
        major.nature = Some(Nature::Factual);
        let mut closed = dispute("D3", Severity::Minor);
        closed.status = DisputeStatus::Improved;
        closed.resolved_by = Some(ResolvedBy::Debate);
        ledger.disputes = vec![minor, major, closed];

        let n = ledger.auto_resolve_minors("default#minor");
        assert_eq!(n, 1, "only the open minor is closed");
        assert_eq!(
            ledger.disputes[0].resolved_by,
            Some(ResolvedBy::DefaultRule)
        );
        assert_eq!(ledger.disputes[0].status, DisputeStatus::Upheld);
        assert_eq!(ledger.disputes[1].status, DisputeStatus::Open);
        assert_eq!(ledger.disputes[2].status, DisputeStatus::Improved);
    }

    #[test]
    fn escalation_gate_and_factual_conflicts() {
        let mut ledger = Ledger::new(vec![item("REQ-1")]);
        // Only a minor open → gate is false (minors never reach humans) and
        // the auto-pass closes it.
        ledger.disputes.push(dispute("D1", Severity::Minor));
        assert!(!ledger.needs_escalation());
        ledger.auto_resolve_minors("default#minor");
        assert!(!ledger.needs_escalation());
        assert!(ledger.all_resolved());

        // A major factual open → escalation + evidence-conflict labeling.
        let mut major = dispute("D2", Severity::Major);
        major.nature = Some(Nature::Factual);
        ledger.disputes.push(major);
        assert!(ledger.needs_escalation());
        assert_eq!(ledger.factual_conflicts().len(), 1);
    }

    #[test]
    fn dispute_lookup_is_case_insensitive() {
        let mut ledger = Ledger::new(vec![item("REQ-1")]);
        ledger.disputes.push(dispute("D7", Severity::Minor));
        assert!(ledger.find_dispute_mut("d7").is_some());
        assert!(ledger.find_dispute_mut("D8").is_none());
    }
}
