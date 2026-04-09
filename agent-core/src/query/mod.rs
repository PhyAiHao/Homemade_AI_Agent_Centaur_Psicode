//! Query module — the streaming tool-use loop and supporting subsystems.
//!
//! Mirrors `src/query.ts`, `src/query/tokenBudget.ts`, `src/query/stopHooks.ts`.

pub mod abort;
pub mod compact;
pub mod message;
pub mod prompt_cache;
pub mod query_loop;
pub mod retry;
pub mod stop_hooks;
pub mod token_budget;
pub mod tool_result_budget;
pub mod tool_summary;
pub mod memory_integration;

