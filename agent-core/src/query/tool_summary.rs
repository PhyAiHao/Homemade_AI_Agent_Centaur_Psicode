//! Tool use summary generation — mirrors the async tool summary in `src/query.ts`.
//!
//! After tool execution, fires an async background LLM call to generate a
//! concise summary of the tool calls. This summary is injected into the
//! conversation for more efficient context usage on subsequent turns.
#![allow(dead_code)]

use serde_json::{json, Value};
use tracing::{debug, warn};

use crate::ipc::{IpcClient, IpcMessage};
use super::message::{ConversationMessage, ToolUseBlock, ToolResultBlock};

/// Generate a concise summary of tool calls + results via the Python brain.
///
/// This is fire-and-forget — the summary is produced asynchronously and
/// injected before the next API call if available.
///
/// Returns a future that resolves to an optional summary message.
pub async fn generate_tool_use_summary(
    ipc: &mut IpcClient,
    tool_blocks: &[ToolUseBlock],
    tool_results: &[ToolResultBlock],
) -> Option<String> {
    if tool_blocks.is_empty() {
        return None;
    }

    // Build a compact representation for the summarizer
    let mut pairs: Vec<Value> = Vec::new();
    for (block, result) in tool_blocks.iter().zip(tool_results.iter()) {
        let result_preview = if result.content.len() > 500 {
            format!("{}...[truncated]", &result.content[..500])
        } else {
            result.content.clone()
        };

        pairs.push(json!({
            "tool": block.name,
            "input_preview": block.input.to_string().chars().take(200).collect::<String>(),
            "result_preview": result_preview,
            "is_error": result.is_error,
        }));
    }

    let _summary_request = json!({
        "type": "skill_request",
        "request_id": crate::ipc::IpcClient::new_request_id(),
        "skill_name": "_tool_summary",
        "arguments": {
            "tool_calls": pairs,
        }
    });

    // Use skill_request IPC to ask Python to generate a summary
    let request = IpcMessage::SkillRequest(crate::ipc::SkillRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        skill_name: "_tool_summary".to_string(),
        arguments: {
            let mut map = std::collections::HashMap::new();
            map.insert("tool_calls".to_string(), Value::Array(pairs));
            map
        },
    });

    match ipc.request(request).await {
        Ok(IpcMessage::SkillResponse(resp)) if !resp.content.is_empty() => {
            debug!(summary_len = resp.content.len(), "Tool use summary generated");
            Some(resp.content)
        }
        Ok(_) => {
            debug!("Tool summary returned empty/unexpected response");
            None
        }
        Err(e) => {
            warn!(error = %e, "Tool summary generation failed");
            None
        }
    }
}

/// Build a ToolUseSummaryMessage for injection into the conversation.
pub fn build_summary_message(summary: &str) -> ConversationMessage {
    ConversationMessage::user_text(format!(
        "[Tool use summary for context efficiency: {summary}]"
    ))
}
