//! Debate orchestration (multi-agent, `dev-docs/agent/multi-agent.md`).
//!
//! Deterministic host-side state machine above the TurnEngine: rounds are
//! full engine runs (proposer/reviewer agents), the dispute ledger
//! ([`ledger::Ledger`]) is the single source of state, and termination is
//! decided by code — never by the model.

pub mod ledger;
pub mod orchestrator;
pub mod parse;
pub mod report;
pub mod spawn;
