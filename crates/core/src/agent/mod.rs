//! AI Agent core (hosting side, in `crates/core`).
//!
//! Service/prompt/handler wiring for `raisfast-agent` (the engine crate),
//! with its table models kept in [`self::models`] for cohesion.
//! Full design: `dev-docs/agent/`.

pub mod context;
pub mod debate;
pub mod handler;
pub mod memory_sql;
pub mod models;
pub mod prompt;
pub mod service;
pub mod skills;
pub mod tools;

pub use models::ai_agent::AiAgent;
pub use models::ai_memory::AiMemory;
pub use models::ai_message::AiMessage;
pub use models::ai_session::AiSession;
