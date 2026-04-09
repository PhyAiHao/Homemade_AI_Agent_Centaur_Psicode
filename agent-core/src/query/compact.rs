//! Full compaction pipeline — mirrors `src/services/compact/`.
//!
//! Three-tier system:
//! 1. Microcompact: clears old tool result content (cheapest)
//! 2. Session memory compact: tries to reduce via session memory (medium)
//! 3. Full compact: summarizes the entire conversation via LLM call (expensive)
//!
//! Also handles:
//! - Reactive compact (on prompt_too_long errors)
//! - Compact boundary markers with metadata
//! - Circuit breaker after 3 consecutive failures
#![allow(dead_code)]

use anyhow::Result;
use serde_json::{json, Value};
use tracing::{info, warn};

use crate::ipc::{IpcClient, IpcMessage};
use super::message::{ConversationMessage, Role};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Buffer tokens reserved for auto-compact (headroom before API call).
pub const AUTOCOMPACT_BUFFER_TOKENS: u64 = 13_000;
/// Max output tokens reserved for compact summary generation.
pub const MAX_OUTPUT_RESERVE: u64 = 20_000;
/// Buffer for warning threshold (UI turns yellow).
pub const WARNING_THRESHOLD_BUFFER: u64 = 20_000;
/// Buffer for blocking limit (manual compact required).
pub const MANUAL_COMPACT_BUFFER: u64 = 3_000;
/// Max consecutive auto-compact failures before circuit breaker trips.
pub const MAX_CONSECUTIVE_FAILURES: u32 = 3;
/// Max retries for prompt-too-long within compact itself.
pub const MAX_PTL_RETRIES: u32 = 3;

// ── Microcompact constants ───────────────────────────────────────────────

/// Keep the last N tool results intact.
pub const MICROCOMPACT_KEEP_RECENT: usize = 4;
/// Replacement text for cleared tool results.
pub const CLEARED_STUB: &str = "[Old tool result content cleared]";
/// Tools whose results can be safely cleared.
pub const COMPACTABLE_TOOLS: &[&str] = &[
    "FileRead", "Bash", "Grep", "Glob", "WebSearch", "WebFetch", "FileEdit", "FileWrite",
];

// ─── Token estimation ───────────────────────────────────────────────────────

/// Rough token estimation: ~3 chars per token, padded by 4/3.
pub fn estimate_tokens(messages: &[ConversationMessage]) -> u64 {
    let chars: usize = messages.iter().map(|m| {
        let content_len = match &m.content {
            Value::String(s) => s.len(),
            Value::Array(arr) => arr.iter().map(|v| {
                v.as_str().map(|s| s.len())
                    .or_else(|| v.get("text").and_then(|t| t.as_str()).map(|s| s.len()))
                    .unwrap_or(50)
            }).sum(),
            _ => 50,
        };
        content_len + 20
    }).sum();
    ((chars as f64 / 3.0) * 4.0 / 3.0) as u64
}

// ─── Threshold calculations ─────────────────────────────────────────────────

/// Compute the auto-compact threshold for a given context window.
///
/// Now accounts for estimated system prompt size to prevent the system prompt
/// from eating into the message budget and causing late compaction triggers.
pub fn auto_compact_threshold(context_window: u64, max_output_tokens: u32) -> u64 {
    let reserved = (max_output_tokens as u64).min(MAX_OUTPUT_RESERVE);
    context_window
        .saturating_sub(reserved)
        .saturating_sub(AUTOCOMPACT_BUFFER_TOKENS)
}

/// Compute the auto-compact threshold accounting for system prompt size.
/// `system_prompt_chars` is the byte length of the full system prompt.
pub fn auto_compact_threshold_with_prompt(
    context_window: u64,
    max_output_tokens: u32,
    system_prompt_chars: usize,
) -> u64 {
    let system_prompt_tokens = (system_prompt_chars as f64 / 3.0 * 4.0 / 3.0) as u64;
    let reserved = (max_output_tokens as u64).min(MAX_OUTPUT_RESERVE);
    context_window
        .saturating_sub(reserved)
        .saturating_sub(AUTOCOMPACT_BUFFER_TOKENS)
        .saturating_sub(system_prompt_tokens)
}

/// Compute the blocking limit (manual compact required).
pub fn blocking_limit(context_window: u64, max_output_tokens: u32) -> u64 {
    let reserved = (max_output_tokens as u64).min(MAX_OUTPUT_RESERVE);
    context_window
        .saturating_sub(reserved)
        .saturating_sub(MANUAL_COMPACT_BUFFER)
}

/// Token usage status for UI display.
#[derive(Debug, Clone)]
pub struct TokenUsageStatus {
    pub percent_used: f64,
    pub is_above_warning: bool,
    pub is_above_error: bool,
    pub is_above_auto_compact: bool,
    pub is_at_blocking_limit: bool,
}

pub fn calculate_token_status(
    estimated_tokens: u64,
    context_window: u64,
    max_output_tokens: u32,
) -> TokenUsageStatus {
    let effective = context_window.saturating_sub(
        (max_output_tokens as u64).min(MAX_OUTPUT_RESERVE)
    );
    let pct = if effective > 0 {
        estimated_tokens as f64 / effective as f64
    } else {
        1.0
    };
    let threshold = auto_compact_threshold(context_window, max_output_tokens);
    let block_limit = blocking_limit(context_window, max_output_tokens);

    TokenUsageStatus {
        percent_used: pct,
        is_above_warning: estimated_tokens > effective.saturating_sub(WARNING_THRESHOLD_BUFFER),
        is_above_error: estimated_tokens > effective.saturating_sub(AUTOCOMPACT_BUFFER_TOKENS),
        is_above_auto_compact: estimated_tokens > threshold,
        is_at_blocking_limit: estimated_tokens > block_limit,
    }
}

// ─── Microcompact ───────────────────────────────────────────────────────────

/// Clear old tool result contents to free context space.
/// Keeps the most recent `keep_recent` tool results intact.
pub fn microcompact(messages: &mut [ConversationMessage], keep_recent: usize) {
    // Find all tool_result message indices
    let tool_result_indices: Vec<usize> = messages.iter().enumerate()
        .filter(|(_, m)| {
            if let Value::Array(blocks) = &m.content {
                blocks.iter().any(|b| b.get("type").and_then(|t| t.as_str()) == Some("tool_result"))
            } else {
                false
            }
        })
        .map(|(i, _)| i)
        .collect();

    if tool_result_indices.len() <= keep_recent {
        return;
    }

    let compact_up_to = tool_result_indices.len() - keep_recent;
    for &idx in &tool_result_indices[..compact_up_to] {
        if let Value::Array(ref mut blocks) = messages[idx].content {
            for block in blocks.iter_mut() {
                if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
                    continue;
                }
                // Check if compactable
                let is_compactable = block.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| COMPACTABLE_TOOLS.iter().any(|&t| n.contains(t)))
                    .unwrap_or(true);
                if !is_compactable {
                    continue;
                }
                // Clear content if large enough to matter
                for field_name in &["content", "output"] {
                    if let Some(val) = block.get_mut(*field_name) {
                        if let Some(s) = val.as_str() {
                            if s.len() > 200 {
                                *val = Value::String(CLEARED_STUB.to_string());
                            }
                        }
                    }
                }
            }
        }
    }
}

// ─── Full compact via IPC ───────────────────────────────────────────────────

/// Compact boundary marker metadata.
#[derive(Debug, Clone)]
pub struct CompactBoundary {
    pub trigger: String,       // "auto" | "manual" | "reactive"
    pub pre_tokens: u64,
    pub messages_summarized: usize,
}

/// Request the Python brain to summarize and compact the conversation.
/// Returns (summary_text, compacted_messages, boundary_metadata) on success.
pub async fn request_full_compact(
    ipc: &mut IpcClient,
    messages: &[ConversationMessage],
    token_budget: Option<u32>,
    trigger: &str,
) -> Result<Option<(String, Vec<Value>, CompactBoundary)>> {
    let pre_tokens = estimate_tokens(messages);
    let msg_count = messages.len();
    let messages_json: Vec<Value> = messages.iter().map(|m| {
        json!({ "role": m.role, "content": m.content })
    }).collect();

    let request = IpcMessage::CompactRequest(crate::ipc::CompactRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        messages: messages_json,
        token_budget,
    });

    match ipc.request(request).await {
        Ok(IpcMessage::CompactResponse(resp)) => {
            if resp.summary.is_empty() && resp.messages.is_empty() {
                return Ok(None);
            }
            let boundary = CompactBoundary {
                trigger: trigger.to_string(),
                pre_tokens,
                messages_summarized: msg_count,
            };
            Ok(Some((resp.summary, resp.messages, boundary)))
        }
        Ok(_) => Ok(None),
        Err(e) => {
            warn!(error = %e, "Compact IPC request failed");
            Ok(None)
        }
    }
}

/// Apply compaction result to the message list.
/// Replaces all messages with: boundary marker + summary + compacted messages.
pub fn apply_compact_result(
    messages: &mut Vec<ConversationMessage>,
    summary: &str,
    compacted_msgs: &[Value],
    boundary: &CompactBoundary,
) {
    let msgs_before = messages.len();
    messages.clear();

    // Compact boundary marker
    messages.push(ConversationMessage::user_text(format!(
        "[Conversation compacted ({trigger}). Summary of {count} prior messages ({tokens} est. tokens):\n\
         {summary}\n]",
        trigger = boundary.trigger,
        count = boundary.messages_summarized,
        tokens = boundary.pre_tokens,
    )));

    // Re-add compacted messages from Python
    for msg_val in compacted_msgs {
        if let (Some(role_str), Some(content)) = (
            msg_val.get("role").and_then(|r| r.as_str()),
            msg_val.get("content").cloned(),
        ) {
            messages.push(ConversationMessage {
                role: if role_str == "assistant" { Role::Assistant } else { Role::User },
                content,
            });
        }
    }

    info!(
        before = msgs_before,
        after = messages.len(),
        trigger = %boundary.trigger,
        "Compact applied"
    );
}

// ─── Auto-compact orchestration ─────────────────────────────────────────────

/// Tracking state for auto-compact circuit breaker.
#[derive(Debug, Default)]
pub struct AutoCompactTracking {
    pub consecutive_failures: u32,
}

/// Attempt auto-compaction if tokens exceed threshold.
/// Returns true if compaction was performed.
pub async fn auto_compact_if_needed(
    messages: &mut Vec<ConversationMessage>,
    ipc: &mut IpcClient,
    context_window: u64,
    max_output_tokens: u32,
    tracking: &mut AutoCompactTracking,
) -> bool {
    // Circuit breaker
    if tracking.consecutive_failures >= MAX_CONSECUTIVE_FAILURES {
        return false;
    }

    let threshold = auto_compact_threshold(context_window, max_output_tokens);
    let estimated = estimate_tokens(messages);

    if estimated <= threshold {
        return false;
    }

    info!(
        tokens = estimated,
        threshold = threshold,
        "Token budget tight — requesting auto-compact"
    );

    // Try session memory compact first (via Python IPC with special budget)
    let target_budget = (threshold / 2) as u32;
    match request_full_compact(ipc, messages, Some(target_budget), "auto").await {
        Ok(Some((summary, compacted, boundary))) => {
            apply_compact_result(messages, &summary, &compacted, &boundary);
            tracking.consecutive_failures = 0;
            true
        }
        Ok(None) => {
            tracking.consecutive_failures += 1;
            warn!(
                failures = tracking.consecutive_failures,
                "Auto-compact returned no result"
            );
            false
        }
        Err(e) => {
            tracking.consecutive_failures += 1;
            warn!(
                error = %e,
                failures = tracking.consecutive_failures,
                "Auto-compact failed"
            );
            false
        }
    }
}

/// Reactive compact — triggered when API returns prompt_too_long.
/// Single-shot attempt, no circuit breaker.
pub async fn reactive_compact(
    messages: &mut Vec<ConversationMessage>,
    ipc: &mut IpcClient,
    context_window: u64,
    max_output_tokens: u32,
) -> bool {
    let target = (auto_compact_threshold(context_window, max_output_tokens) / 2) as u32;
    match request_full_compact(ipc, messages, Some(target), "reactive").await {
        Ok(Some((summary, compacted, boundary))) => {
            apply_compact_result(messages, &summary, &compacted, &boundary);
            true
        }
        _ => false,
    }
}
