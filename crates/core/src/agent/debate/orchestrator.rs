//! Debate orchestrator: the deterministic state machine above the TurnEngine
//! (multi-agent §6.5, `dev-docs/agent/multi-agent.md`).
//!
//! - Rounds are full engine runs; the dispute ledger is the single source of
//!   state and the only thing passed between rounds (bounded context).
//! - Termination is decided by Rust code (stall / round budget / token
//!   budget), never by the model. Auto-adjudication closes `minor` disputes
//!   before the terminal check — escalation is a cost, minors never reach
//!   humans.
//! - Zero-progress stall detection `[照抄 opencode doom-loop]`: one stale
//!   round escalates immediately; debate rounds are expensive.
//! - One repair retry per unparseable round `[照抄 kb/distill.rs
//!   parse_json_object + claw-code InvalidArgumentsError]`; degradation
//!   paths never corrupt the ledger.

use serde_json::json;

use super::ledger::{Challenge, Dispute, DisputeStatus, Ledger, ResolvedBy, RoundLog, Stance};
use super::parse::{parse_challenge, parse_defense, parse_r0};
use super::spawn::{AgentTurnRequest, continue_agent_turn, run_agent_turn};
use crate::agent::models::ai_debate::{self, AiDebate};
use crate::agent::service::AgentTurnResult;
use crate::db::DbDriver;
use crate::errors::app_error::{AppError, AppResult};
use crate::event::Event;
use crate::middleware::auth::AuthUser;
use crate::types::snowflake_id::SnowflakeId;
use raisfast_agent::TokenUsage;

/// Debate lifecycle statuses (multi-agent §7).
pub mod status {
    pub const RUNNING: &str = "running";
    pub const CONSENSUS: &str = "consensus";
    pub const ESCALATED: &str = "escalated";
    pub const FINALIZING: &str = "finalizing";
    pub const CONCLUDED: &str = "concluded";
    pub const CANCELLED: &str = "cancelled";
    pub const FAILED: &str = "failed";
}

/// Create the debate row and drive it in a background task. Returns
/// immediately with the `running` row (poll the row or subscribe to
/// `ai.debate.*` events for progress).
pub async fn start_debate(
    state: &crate::AppState,
    auth: &AuthUser,
    agent_a_id: SnowflakeId,
    agent_b_id: SnowflakeId,
    origin_session_id: Option<SnowflakeId>,
    requirement: String,
    max_rounds: Option<u32>,
) -> AppResult<AiDebate> {
    let cfg = &state.config.ai.debate;
    if !cfg.enabled {
        return Err(AppError::BadRequest("debate subsystem disabled".into()));
    }
    if agent_a_id == agent_b_id {
        return Err(AppError::BadRequest(
            "proposer and reviewer must be different agents".into(),
        ));
    }
    let tenant = auth.tenant_id();
    // Anchored mode: origin session must exist and belong to the caller; the
    // proposer is that session's own agent (the one the user talked to).
    if let Some(origin) = origin_session_id {
        let session =
            crate::agent::models::ai_session::find_session_by_id(&state.pool, origin, tenant)
                .await?;
        let owner = owner_id(auth)?;
        if session.user_id != owner {
            return Err(AppError::ForbiddenOwnership);
        }
    }
    // Concurrency governor: per-tenant running cap (multi-agent §9).
    let running = sqlx::query_scalar::<_, i64>(crate::db::safe_sql(&format!(
        "SELECT {} FROM ai_debates WHERE status = {} AND tenant_id = {}",
        crate::db::Driver::cast_int("COUNT(*)"),
        crate::db::Driver::ph(1),
        crate::db::Driver::ph(2)
    )))
    .bind(status::RUNNING)
    .bind(tenant)
    .fetch_one(&state.pool)
    .await
    .unwrap_or(0);
    if running >= i64::from(cfg.max_concurrent) {
        return Err(AppError::Conflict("debate_concurrency_limit".into()));
    }

    let ledger = serde_json::to_value(Ledger::default()).expect("empty ledger serializes");
    let debate = ai_debate::create_debate(
        &state.pool,
        tenant,
        owner_id(auth)?,
        agent_a_id,
        agent_b_id,
        origin_session_id,
        &requirement,
        max_rounds.map(|r| json!({ "max_rounds": r })),
        ledger,
    )
    .await?;

    let st = state.clone();
    let actor = auth.clone();
    let debate_id = debate.id;
    tokio::spawn(async move {
        if let Err(e) = run_debate(&st, &actor, debate_id).await {
            tracing::error!(debate = debate_id.0, err = %e, "debate failed");
            let _ = ai_debate::set_debate_failed(
                &st.pool,
                debate_id,
                actor.tenant_id(),
                &e.to_string(),
            )
            .await;
            emit(
                &st,
                debate_id,
                "ai.debate.failed",
                json!({ "error": e.to_string() }),
            );
        }
    });
    Ok(debate)
}

/// Drive the full debate for an already-created `running` row.
async fn run_debate(
    state: &crate::AppState,
    auth: &AuthUser,
    debate_id: SnowflakeId,
) -> AppResult<()> {
    let tenant = auth.tenant_id();
    let debate = ai_debate::find_debate_by_id(&state.pool, debate_id, tenant).await?;
    let max_rounds = debate
        .params
        .as_ref()
        .and_then(|p| p.get("max_rounds"))
        .and_then(serde_json::Value::as_u64)
        .map(|r| r.min(u64::from(state.config.ai.debate.max_rounds)) as u32)
        .unwrap_or(state.config.ai.debate.max_rounds)
        .clamp(1, 5);

    emit(
        state,
        debate_id,
        "ai.debate.started",
        json!({ "max_rounds": max_rounds }),
    );

    // ── R0: itemization (anchored = in the origin session itself) ────────
    let r0_instruction = r0_instruction(&debate.requirement);
    let (r0, r0_session): (AgentTurnResult, SnowflakeId) = match debate.origin_session_id {
        Some(origin) => {
            let r = continue_agent_turn(state, auth, origin, &r0_instruction).await?;
            (r, origin)
        }
        None => {
            let out = run_agent_turn(
                state,
                auth,
                &AgentTurnRequest {
                    agent_id: debate.agent_a_id,
                    parent_session_id: None,
                    title: "debate R0 (proposer itemization)",
                    role: "proposer",
                    debate_id: Some(debate_id),
                    input: &r0_instruction,
                },
            )
            .await?;
            let session_id = out.session_id;
            (out.turns, session_id)
        }
    };
    let mut ledger: Ledger = match parse_r0(&r0.text) {
        Ok(out) => Ledger::new(out.items),
        Err(reason) => {
            let repaired =
                match continue_agent_turn(state, auth, r0_session, &repair_instruction(&reason))
                    .await
                {
                    Ok(r) => parse_r0(&r.text),
                    Err(e) => Err(e.to_string()),
                };
            match repaired {
                Ok(out) => Ledger::new(out.items),
                Err(e) => {
                    return Err(AppError::BadRequest(format!("R0 itemization failed: {e}")));
                }
            }
        }
    };
    let mut usage_total = r0.usage;
    persist_ledger(state, debate_id, tenant, &ledger, 0).await?;
    emit(
        state,
        debate_id,
        "ai.debate.round_started",
        json!({ "round": 1 }),
    );

    // ── Rounds ─────────────────────────────────────────────────────────
    for round in 1..=max_rounds {
        // Cancellation: DELETE flips the row status; bail before the next
        // round (an in-flight engine turn finishes naturally, §11 cancel).
        let row = ai_debate::find_debate_by_id(&state.pool, debate_id, tenant).await?;
        if row.status != status::RUNNING {
            tracing::info!(debate = debate_id.0, "debate cancelled, bailing");
            return Ok(());
        }
        ai_debate::update_heartbeat(&state.pool, debate_id, tenant).await?;
        let mut transitions: u32 = 0;
        let item_ids = ledger_item_ids(&ledger);

        // Reviewer round (challenger B).
        let b_out = run_agent_turn(
            state,
            auth,
            &AgentTurnRequest {
                agent_id: debate.agent_b_id,
                parent_session_id: debate.origin_session_id,
                title: &format!("debate round {round} (reviewer)"),
                role: "reviewer",
                debate_id: Some(debate_id),
                input: &challenger_instruction(&debate.requirement, &ledger, round, round == 1),
            },
        )
        .await?;
        accumulate(&mut usage_total, &b_out.turns.usage);
        let open_ids = open_dispute_ids(&ledger);
        let challenge = match parse_challenge(&b_out.text, &open_ids, &item_ids, round == 1) {
            Ok(c) => c,
            Err(reason) => match repair_round(state, auth, b_out.session_id, &reason, |text| {
                parse_challenge(text, &open_ids, &item_ids, round == 1).map_err(|e| e.to_string())
            })
            .await
            {
                Ok(c) => c,
                Err(e) => {
                    tracing::warn!(debate = debate_id.0, round, "challenge unparseable: {e}");
                    finish_round(state, debate_id, tenant, &mut ledger, round, 0).await?;
                    if maybe_terminate(
                        state,
                        auth,
                        &debate,
                        &mut ledger,
                        RoundCheck {
                            round,
                            max_rounds,
                            transitions: 0,
                            budget_hit: false,
                        },
                    )
                    .await?
                    {
                        return Ok(());
                    }
                    continue;
                }
            },
        };

        // Apply: new challenges → open disputes; re_visits → withdrawals.
        for c in &challenge.new_challenges {
            let id = ledger.next_dispute_id();
            ledger.disputes.push(Dispute {
                id,
                title: c.evidence.chars().take(80).collect(),
                target: c.target.clone(),
                kind: c.kind,
                nature: c.nature,
                severity: c.severity,
                status: DisputeStatus::Open,
                resolved_by: None,
                rule_id: None,
                challenges: vec![Challenge {
                    round,
                    argument: c.evidence.clone(),
                    sources: c.sources.clone(),
                    nature: c.nature,
                    proposed_resolution: c.proposed_resolution.clone(),
                }],
                responses: vec![],
                withdraw_round: None,
                brief: None,
                verdict: None,
            });
            transitions += 1;
        }
        for r in &challenge.re_visits {
            if let Some(d) = ledger.find_dispute_mut(&r.dispute_id) {
                d.status = DisputeStatus::Upheld;
                d.resolved_by = Some(ResolvedBy::Debate);
                d.withdraw_round = Some(round);
                transitions += 1;
            }
        }

        // Proposer round (defender A) — respond to everything now open.
        let open_ids = open_dispute_ids(&ledger);
        let a_out = run_agent_turn(
            state,
            auth,
            &AgentTurnRequest {
                agent_id: debate.agent_a_id,
                parent_session_id: debate.origin_session_id,
                title: &format!("debate round {round} (proposer)"),
                role: "proposer",
                debate_id: Some(debate_id),
                input: &defender_instruction(&debate.requirement, &ledger, &b_out.text),
            },
        )
        .await?;
        accumulate(&mut usage_total, &a_out.turns.usage);
        let defense = match parse_defense(&a_out.text, &open_ids) {
            Ok(d) => d,
            Err(reason) => match repair_round(state, auth, a_out.session_id, &reason, |text| {
                parse_defense(text, &open_ids).map_err(|e| e.to_string())
            })
            .await
            {
                Ok(d) => d,
                Err(e) => {
                    tracing::warn!(debate = debate_id.0, round, "defense unparseable: {e}");
                    finish_round(state, debate_id, tenant, &mut ledger, round, transitions).await?;
                    if maybe_terminate(
                        state,
                        auth,
                        &debate,
                        &mut ledger,
                        RoundCheck {
                            round,
                            max_rounds,
                            transitions,
                            budget_hit: false,
                        },
                    )
                    .await?
                    {
                        return Ok(());
                    }
                    continue;
                }
            },
        };

        // Apply responses + revisions.
        for r in &defense.responses {
            if let Some(d) = ledger.find_dispute_mut(&r.dispute_id) {
                match r.stance {
                    Stance::Accept => {
                        d.status = DisputeStatus::Improved;
                        d.resolved_by = Some(ResolvedBy::Debate);
                    }
                    other => {
                        // reject/partial stays open; A's reasons are recorded
                        // so B must bring new evidence or withdraw (anti-flip).
                        d.responses.push(super::ledger::DefenseResponse {
                            round,
                            stance: other,
                            reasoning: r.reasoning.clone(),
                            sources: r.sources.clone(),
                        });
                    }
                }
                transitions += 1;
            }
        }
        for rev in &defense.revised_items {
            if let Some(item) = ledger.items.iter_mut().find(|i| i.id == rev.id) {
                item.story = rev.story.clone();
                item.criteria = rev.criteria.clone();
                item.change_note = rev.change_note.clone();
            }
        }
        // Auto-adjudication: minors never reach humans (§6.5).
        if state.config.ai.debate.auto_resolve_minor {
            transitions += ledger.auto_resolve_minors("default#minor");
        }

        // Token budget: exceeded → force escalation after this round.
        let budget = state.config.ai.debate.max_total_tokens;
        let budget_hit =
            budget > 0 && total_tokens(usage_total) >= u64::try_from(budget).unwrap_or(u64::MAX);

        finish_round(state, debate_id, tenant, &mut ledger, round, transitions).await?;
        ai_debate::update_usage_total(&state.pool, debate_id, tenant, usage_json(usage_total))
            .await?;
        if maybe_terminate(
            state,
            auth,
            &debate,
            &mut ledger,
            RoundCheck {
                round,
                max_rounds,
                transitions,
                budget_hit,
            },
        )
        .await?
        {
            return Ok(());
        }
    }
    Ok(())
}

/// Terminal outcomes decided by pure code (never the model).
enum Terminal {
    Consensus,
    Escalated { reason: &'static str },
}

/// Per-round termination inputs (multi-agent §6.5).
#[derive(Clone, Copy)]
struct RoundCheck {
    round: u32,
    max_rounds: u32,
    transitions: u32,
    budget_hit: bool,
}

fn decide_terminal(ledger: &Ledger, check: RoundCheck) -> Option<Terminal> {
    if ledger.all_resolved() {
        Some(Terminal::Consensus)
    } else if check.transitions == 0 {
        Some(Terminal::Escalated { reason: "stall" })
    } else if check.budget_hit {
        Some(Terminal::Escalated {
            reason: "token_budget",
        })
    } else if check.round >= check.max_rounds {
        Some(Terminal::Escalated {
            reason: "max_rounds",
        })
    } else {
        None
    }
}

/// Execute a terminal outcome: escalation runs the decision brief (§6.7,
/// best effort) and the four-section report projection (§6.6) before the
/// status flip; consensus just flips.
async fn handle_terminal(
    state: &crate::AppState,
    auth: &AuthUser,
    debate: &AiDebate,
    ledger: &mut Ledger,
    round: u32,
    terminal: Terminal,
) -> AppResult<()> {
    let tenant = debate.tenant_id.as_deref();
    match terminal {
        Terminal::Consensus => {
            ai_debate::set_debate_status(&state.pool, debate.id, tenant, status::CONSENSUS).await?;
            emit(
                state,
                debate.id,
                "ai.debate.concluded",
                json!({
                    "outcome": "consensus",
                    "rounds": round,
                    "disputes": ledger.disputes.len(),
                }),
            );
        }
        Terminal::Escalated { reason } => {
            // 1. Open disputes become the human verdict queue.
            ledger.mark_all_escalated();
            // 2. Decision brief (best effort — failure renders as 缺失, §6.7).
            run_decision_brief(state, auth, debate, ledger).await;
            // 3. Persist ledger + report projection.
            persist_ledger(state, debate.id, tenant, ledger, round).await?;
            let report = super::report::render(&debate.requirement, ledger);
            ai_debate::update_report(&state.pool, debate.id, tenant, &report).await?;
            ai_debate::set_debate_status(&state.pool, debate.id, tenant, status::ESCALATED).await?;
            emit(
                state,
                debate.id,
                "ai.debate.escalated",
                json!({
                    "rounds": round,
                    "open": ledger.escalated_ids().len(),
                    "reason": reason,
                    "report_ready": true,
                }),
            );
        }
    }
    Ok(())
}

/// Escalation brief: one light turn to the reviewer covering every escalated
/// dispute. Failure is tolerated — the report marks missing fields (§6.7).
async fn run_decision_brief(
    state: &crate::AppState,
    auth: &AuthUser,
    debate: &AiDebate,
    ledger: &mut Ledger,
) {
    let ids = ledger.escalated_ids();
    if ids.is_empty() {
        return;
    }
    let input = brief_instruction(&debate.requirement, ledger, &ids);
    let outcome = run_agent_turn(
        state,
        auth,
        &AgentTurnRequest {
            agent_id: debate.agent_b_id,
            parent_session_id: debate.origin_session_id,
            title: "escalation brief (reviewer)",
            role: "reviewer",
            debate_id: Some(debate.id),
            input: &input,
        },
    )
    .await;
    let briefs = match outcome {
        Ok(o) => match super::parse::parse_brief(&o.text, &ids) {
            Ok(b) => b,
            Err(e) => {
                tracing::warn!(debate = debate.id.0, "brief unparseable: {e}");
                return;
            }
        },
        Err(e) => {
            tracing::warn!(debate = debate.id.0, err = %e, "brief turn failed");
            return;
        }
    };
    for b in briefs.briefs {
        if let Some(d) = ledger.find_dispute_mut(&b.dispute_id) {
            d.nature = d.nature.or(b.nature);
            d.brief = Some(super::ledger::EscalationBrief {
                nature: b.nature,
                recommendation: b.recommendation,
                cost_if_a: b.cost_if_a,
                cost_if_b: b.cost_if_b,
                impact: b.impact,
            });
        }
    }
}

/// Persist the round, then apply the termination rules. Returns `true` when
/// the debate reached a terminal state.
async fn maybe_terminate(
    state: &crate::AppState,
    auth: &AuthUser,
    debate: &AiDebate,
    ledger: &mut Ledger,
    check: RoundCheck,
) -> AppResult<bool> {
    match decide_terminal(ledger, check) {
        Some(t) => {
            handle_terminal(state, auth, debate, ledger, check.round, t).await?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// One round's bookkeeping: rounds_log append + ledger persist.
async fn finish_round(
    state: &crate::AppState,
    debate_id: SnowflakeId,
    tenant: Option<&str>,
    ledger: &mut Ledger,
    round: u32,
    transitions: u32,
) -> AppResult<()> {
    ledger.rounds_log.push(RoundLog {
        round,
        transitions,
        a_session_id: None,
        b_session_id: None,
        outcome: Some("applied".into()),
    });
    persist_ledger(state, debate_id, tenant, ledger, round).await
}

async fn persist_ledger(
    state: &crate::AppState,
    debate_id: SnowflakeId,
    tenant: Option<&str>,
    ledger: &Ledger,
    rounds_done: u32,
) -> AppResult<()> {
    ai_debate::update_ledger(
        &state.pool,
        debate_id,
        tenant,
        serde_json::to_value(ledger).expect("ledger serializes"),
        i32::try_from(rounds_done).unwrap_or(i32::MAX),
    )
    .await
}

/// One repair retry on the same session (model sees its previous answer).
async fn repair_round<T>(
    state: &crate::AppState,
    auth: &AuthUser,
    session_id: SnowflakeId,
    reason: &str,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<T, String> {
    let repaired = continue_agent_turn(state, auth, session_id, &repair_instruction(reason))
        .await
        .map_err(|e| e.to_string())?;
    parse(&repaired.text)
}

fn open_dispute_ids(ledger: &Ledger) -> Vec<String> {
    ledger
        .disputes
        .iter()
        .filter(|d| d.status == DisputeStatus::Open)
        .map(|d| d.id.clone())
        .collect()
}

fn ledger_item_ids(ledger: &Ledger) -> Vec<String> {
    let mut ids: Vec<String> = ledger.items.iter().map(|i| i.id.clone()).collect();
    ids.push("summary".into());
    ids
}

fn accumulate(total: &mut Option<TokenUsage>, add: &Option<TokenUsage>) {
    if let Some(add) = add {
        let t = total.get_or_insert_with(TokenUsage::default);
        t.accumulate(*add);
    }
}

fn total_tokens(usage: Option<TokenUsage>) -> u64 {
    usage
        .map(|u| u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0))
        .unwrap_or(0)
}

fn usage_json(usage: Option<TokenUsage>) -> serde_json::Value {
    match usage {
        Some(u) => json!({
            "input_tokens": u.input_tokens,
            "output_tokens": u.output_tokens,
            "cache_read": u.cache_read,
            "cache_write": u.cache_write,
        }),
        None => json!({}),
    }
}

fn owner_id(auth: &AuthUser) -> AppResult<SnowflakeId> {
    auth.user_id()
        .map(SnowflakeId)
        .ok_or(AppError::Unauthorized)
}

fn emit(
    state: &crate::AppState,
    debate_id: SnowflakeId,
    event_type: &str,
    mut data: serde_json::Value,
) {
    if let Some(obj) = data.as_object_mut() {
        obj.insert("debate_id".into(), json!(debate_id.0));
    }
    state.emitter.emit(Event::Custom {
        source: "ai".into(),
        event_type: event_type.to_string(),
        data,
    });
}

// ── Prompt assembly (round contracts; schemas live in prompts/*.md) ────

fn r0_instruction(requirement: &str) -> String {
    let contract = crate::utils::prompt_file::prompt_file!("src/agent/prompts/debate_r0.md");
    format!("{contract}\n\n─── 需求原文 ───\n{requirement}")
}

fn challenger_instruction(
    requirement: &str,
    ledger: &Ledger,
    round: u32,
    first_round: bool,
) -> String {
    let contract =
        crate::utils::prompt_file::prompt_file!("src/agent/prompts/debate_challenger.md");
    let round_note = if first_round {
        "这是第一轮：必须至少提出 1 条 challenge。".to_string()
    } else {
        "只提新的、有新证据的问题；对已解决项不得翻案。".to_string()
    };
    format!(
        "{contract}\n\n─── 需求原文 ───\n{requirement}\n\n─── 条目化清单与分歧台账 ───\n{ledger_json}\n\n─── 本轮要求 ───\n第 {round} 轮评审。{round_note}",
        ledger_json = serde_json::to_string_pretty(ledger).unwrap_or_default(),
    )
}

fn defender_instruction(requirement: &str, ledger: &Ledger, reviewer_output: &str) -> String {
    let contract = crate::utils::prompt_file::prompt_file!("src/agent/prompts/debate_defender.md");
    format!(
        "{contract}\n\n─── 需求原文 ───\n{requirement}\n\n─── 条目化清单与分歧台账 ───\n{ledger_json}\n\n─── 评审员本轮输出 ───\n{reviewer_output}",
        ledger_json = serde_json::to_string_pretty(ledger).unwrap_or_default(),
    )
}

fn repair_instruction(reason: &str) -> String {
    format!(
        "你上一条输出无法通过格式校验：{reason}\n请重新输出：只输出一个合法 JSON 对象，严格遵守此前给出的字段与取值，不要包含任何 JSON 之外的文字。"
    )
}

// ── Human verdicts + final round (multi-agent §7-§8, M-A3) ─────────────

/// One human verdict input (multi-agent §8).
pub struct VerdictInput {
    pub dispute_id: String,
    /// `side_a | side_b | custom`.
    pub verdict: String,
    /// Required when `verdict = custom`.
    pub resolution: Option<String>,
    pub note: Option<String>,
}

/// Record human verdicts on escalated disputes (service policy: debate
/// owner; the handler layer adds the admin bypass). When every escalated
/// dispute is consumed, the debate flips to `finalizing` and the final
/// round (proposer absorbs consensus + verdicts) runs in the background.
pub async fn submit_verdicts(
    state: &crate::AppState,
    auth: &AuthUser,
    debate_id: SnowflakeId,
    verdicts: Vec<VerdictInput>,
) -> AppResult<AiDebate> {
    let tenant = auth.tenant_id();
    let debate = ai_debate::find_debate_by_id(&state.pool, debate_id, tenant).await?;
    if debate.user_id != owner_id(auth)? {
        return Err(AppError::ForbiddenOwnership);
    }
    if debate.status != status::ESCALATED {
        return Err(AppError::BadRequest(format!(
            "debate status is {} — verdicts are accepted only on {}",
            debate.status,
            status::ESCALATED
        )));
    }
    let mut ledger: Ledger = serde_json::from_value(debate.ledger.clone()).expect("ledger decodes");
    for v in &verdicts {
        let d = ledger
            .find_dispute_mut(&v.dispute_id)
            .ok_or_else(|| AppError::BadRequest(format!("dispute {} 不存在", v.dispute_id)))?;
        if d.status != DisputeStatus::Escalated {
            return Err(AppError::BadRequest(format!(
                "dispute {} 不是待判状态",
                v.dispute_id
            )));
        }
        let terminal = match v.verdict.as_str() {
            "side_a" => DisputeStatus::Improved,
            "side_b" => DisputeStatus::Upheld,
            "custom" => {
                if v.resolution.as_deref().unwrap_or("").trim().is_empty() {
                    return Err(AppError::BadRequest(
                        "custom 判决必须提供 resolution".into(),
                    ));
                }
                DisputeStatus::Improved
            }
            other => {
                return Err(AppError::BadRequest(format!(
                    "verdict 非法：{other}（side_a | side_b | custom）"
                )));
            }
        };
        d.status = terminal;
        d.resolved_by = Some(ResolvedBy::Verdict);
        d.verdict = Some(super::ledger::Verdict {
            verdict: v.verdict.clone(),
            resolution: v.resolution.clone(),
            note: v.note.clone(),
        });
    }
    persist_ledger(
        state,
        debate_id,
        tenant,
        &ledger,
        u32::try_from(debate.rounds_done).unwrap_or(0),
    )
    .await?;
    emit(
        state,
        debate_id,
        "ai.debate.verdict_recorded",
        json!({ "recorded": verdicts.len(), "pending": ledger.escalated_ids().len() }),
    );

    if ledger.escalated_ids().is_empty() {
        ai_debate::set_debate_status(&state.pool, debate_id, tenant, status::FINALIZING).await?;
        let st = state.clone();
        let actor = auth.clone();
        tokio::spawn(async move {
            if let Err(e) = finalize_debate(&st, &actor, debate_id).await {
                tracing::error!(debate = debate_id.0, err = %e, "debate finalize failed");
                let _ = ai_debate::set_debate_failed(
                    &st.pool,
                    debate_id,
                    actor.tenant_id(),
                    &e.to_string(),
                )
                .await;
                emit(
                    &st,
                    debate_id,
                    "ai.debate.failed",
                    json!({ "error": e.to_string() }),
                );
            }
        });
    }
    ai_debate::find_debate_by_id(&state.pool, debate_id, tenant).await
}

/// Final round: the proposer absorbs consensus + verdicts (binding, no
/// rebuttal) and emits the complete final item list (multi-agent §7).
async fn finalize_debate(
    state: &crate::AppState,
    auth: &AuthUser,
    debate_id: SnowflakeId,
) -> AppResult<()> {
    let tenant = auth.tenant_id();
    let debate = ai_debate::find_debate_by_id(&state.pool, debate_id, tenant).await?;
    let ledger: Ledger = serde_json::from_value(debate.ledger.clone()).expect("ledger decodes");
    let input = final_instruction(&debate.requirement, &ledger);

    let (final_turn, final_session): (AgentTurnResult, SnowflakeId) = match debate.origin_session_id
    {
        Some(origin) => {
            let r = continue_agent_turn(state, auth, origin, &input).await?;
            (r, origin)
        }
        None => {
            let out = run_agent_turn(
                state,
                auth,
                &AgentTurnRequest {
                    agent_id: debate.agent_a_id,
                    parent_session_id: None,
                    title: "debate final round (proposer)",
                    role: "proposer",
                    debate_id: Some(debate_id),
                    input: &input,
                },
            )
            .await?;
            let sid = out.session_id;
            (out.turns, sid)
        }
    };
    let parsed = match parse_r0(&final_turn.text) {
        Ok(out) => Ok(out),
        Err(reason) => {
            // One repair retry, then the debate fails with report intact.
            continue_agent_turn(state, auth, final_session, &repair_instruction(&reason))
                .await
                .map_err(|e| e.to_string())
                .and_then(|r| parse_r0(&r.text).map_err(|e| e.to_string()))
        }
    };
    let final_items =
        parsed.map_err(|e| AppError::BadRequest(format!("final round output unparseable: {e}")))?;

    let mut ledger = ledger;
    ledger.items = final_items.items;
    persist_ledger(
        state,
        debate_id,
        tenant,
        &ledger,
        u32::try_from(debate.rounds_done).unwrap_or(0),
    )
    .await?;
    ai_debate::set_debate_status(&state.pool, debate_id, tenant, status::CONCLUDED).await?;
    emit(
        state,
        debate_id,
        "ai.debate.concluded",
        json!({ "outcome": "final", "items": ledger.items.len() }),
    );
    Ok(())
}

fn brief_instruction(requirement: &str, ledger: &Ledger, ids: &[String]) -> String {
    let contract = crate::utils::prompt_file::prompt_file!("src/agent/prompts/debate_brief.md");
    format!(
        "{contract}\n\n─── 需求原文 ───\n{requirement}\n\n─── 台账 ───\n{ledger_json}\n\n─── 待判分歧 ───\n{}",
        ids.join(", "),
        ledger_json = serde_json::to_string_pretty(ledger).unwrap_or_default(),
    )
}

fn final_instruction(requirement: &str, ledger: &Ledger) -> String {
    let contract = crate::utils::prompt_file::prompt_file!("src/agent/prompts/debate_final.md");
    format!(
        "{contract}\n\n─── 需求原文 ───\n{requirement}\n\n─── 终版台账（含全部共识与人工判决）───\n{ledger_json}",
        ledger_json = serde_json::to_string_pretty(ledger).unwrap_or_default(),
    )
}
