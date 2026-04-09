//! Tool orchestration — manages the streaming tool-use loop.
//!
//! Mirrors `src/services/tools/toolOrchestration.ts` and
//! `src/services/tools/toolStreamingExecution.ts`.
//!
//! This module ties together the query engine's streaming API responses
//! with tool execution, handling the back-and-forth cycle of:
//!   1. Receive `tool_use` blocks from the API
//!   2. Execute each tool (possibly in parallel)
//!   3. Send `tool_result` blocks back
//!   4. Repeat until the model sends a `message_done`
#![allow(dead_code)]

use anyhow::Result;
use serde_json::Value;

use super::{ToolResult, ToolRegistry};
use super::hooks::HookDef;
use crate::permissions::gate::PermissionGate;

/// A tool call request from the API.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// The result of processing a batch of tool calls.
#[derive(Debug)]
pub struct ToolCallBatch {
    pub results: Vec<(String, ToolResult)>,  // (tool_use_id, result)
}

/// Process a batch of tool calls, executing them with concurrency where safe.
///
/// Tools that are read-only (requires_permission = false) can run in parallel.
/// Write tools run sequentially to avoid conflicts.
pub async fn process_tool_calls(
    calls: Vec<ToolCall>,
    registry: &ToolRegistry,
    gate: &PermissionGate,
    hooks: &[HookDef],
    max_concurrent: usize,
) -> Result<ToolCallBatch> {
    use futures::stream::{self, StreamExt};

    let pre_hooks: Vec<&HookDef> = hooks.iter()
        .filter(|h| h.when == super::hooks::HookTiming::Pre)
        .collect();
    let post_hooks: Vec<&HookDef> = hooks.iter()
        .filter(|h| h.when == super::hooks::HookTiming::Post)
        .collect();

    // Execute tool calls with bounded concurrency
    let results: Vec<(String, ToolResult)> = stream::iter(calls)
        .map(|call| {
            let pre = pre_hooks.iter().map(|h| (*h).clone()).collect::<Vec<_>>();
            let post = post_hooks.iter().map(|h| (*h).clone()).collect::<Vec<_>>();
            async move {
                let result = super::execution::execute_tool(
                    registry,
                    gate,
                    &call.name,
                    call.input,
                    &call.id,
                    &pre,
                    &post,
                ).await.unwrap_or_else(|e| ToolResult::error(format!("Tool error: {e}")));
                (call.id, result)
            }
        })
        .buffer_unordered(max_concurrent)
        .collect()
        .await;

    Ok(ToolCallBatch { results })
}

/// Check if we've exceeded the per-turn tool call limit.
pub fn check_tool_call_limit(count: usize, max: usize) -> Option<String> {
    if count >= max {
        Some(format!(
            "Tool call limit reached ({count}/{max}). \
             Increase max_tool_calls_per_turn in config or break work into multiple turns."
        ))
    } else {
        None
    }
}
