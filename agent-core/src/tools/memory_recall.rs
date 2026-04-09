//! MemoryRecall tool — allows the LLM to search the memory archive on demand.
//!
//! This is the pull-based complement to the push-based M4/M5 injection.
//! The LLM calls this tool when it needs context from past conversations
//! that isn't in the core memories (system prompt).

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct MemoryRecallTool;

#[async_trait]
impl Tool for MemoryRecallTool {
    fn name(&self) -> &'static str { "MemoryRecall" }

    fn description(&self) -> &str {
        "Search your memory archive for information from past conversations. \
         Use this when you need context that isn't in the core memories shown \
         in the system prompt. Searches across all stored memories (user preferences, \
         project context, feedback, references) using keyword matching."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query — keywords describing what you're looking for"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of memories to return (default: 5, max: 10)",
                    "default": 5
                }
            },
            "required": ["query"]
        })
    }

    /// Read-only tool — does not require user permission.
    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if query.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: query"));
        }

        let limit = input.get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(5)
            .min(10) as u32;

        let _ = output_tx.send(ToolOutput {
            text: format!("Searching memories for: \"{query}\"..."),
            is_error: false,
        }).await;

        // Send recall request via IPC to Python brain
        let socket = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());

        let mut ipc = match crate::ipc::IpcClient::connect_to(&socket).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("IPC connection failed: {e}"))),
        };

        let request = crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
            request_id: crate::ipc::IpcClient::new_request_id(),
            action: "recall".to_string(),
            payload: {
                let mut p = HashMap::new();
                p.insert("query".to_string(), Value::String(query.to_string()));
                p.insert("limit".to_string(), json!(limit));
                p.insert("include_team".to_string(), Value::Bool(true));
                p
            },
        });

        match ipc.request(request).await {
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) if resp.ok => {
                let memories = resp.payload.get("memories")
                    .and_then(|v| v.as_array());

                match memories {
                    Some(mems) if !mems.is_empty() => {
                        let mut result = format!("Found {} matching memories:\n\n", mems.len());
                        for mem in mems {
                            let metadata = mem.get("metadata").unwrap_or(mem);
                            let name = metadata.get("name")
                                .and_then(|v| v.as_str()).unwrap_or("Untitled");
                            let mem_type = metadata.get("memory_type")
                                .and_then(|v| v.as_str()).unwrap_or("unknown");
                            let tier = metadata.get("tier")
                                .and_then(|v| v.as_str()).unwrap_or("archive");
                            let description = metadata.get("description")
                                .and_then(|v| v.as_str()).unwrap_or("");
                            let body = mem.get("body")
                                .and_then(|v| v.as_str()).unwrap_or("");
                            let freshness = mem.get("freshness")
                                .and_then(|v| v.as_str()).unwrap_or("unknown");

                            result.push_str(&format!(
                                "### [{mem_type}][{tier}] {name}\n"
                            ));
                            if !description.is_empty() {
                                result.push_str(&format!("_{description}_\n"));
                            }
                            result.push_str(&format!("Updated: {freshness}\n\n"));
                            // Cap body at 600 chars per memory
                            if body.len() > 600 {
                                result.push_str(&body[..600]);
                                result.push_str("...\n\n");
                            } else {
                                result.push_str(body);
                                result.push_str("\n\n");
                            }
                        }

                        let _ = output_tx.send(ToolOutput {
                            text: format!("Found {} memories", mems.len()),
                            is_error: false,
                        }).await;

                        Ok(ToolResult::ok(result))
                    }
                    _ => {
                        Ok(ToolResult::ok(format!(
                            "No memories found matching \"{query}\". \
                             The memory archive may not contain information about this topic."
                        )))
                    }
                }
            }
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) => {
                let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
                Ok(ToolResult::error(format!("Memory recall failed: {err}")))
            }
            Ok(_) => Ok(ToolResult::error("Unexpected response from memory service")),
            Err(e) => Ok(ToolResult::error(format!("Memory recall IPC error: {e}"))),
        }
    }
}
