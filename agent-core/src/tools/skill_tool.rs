//! SkillTool — invokes user-defined skills via IPC to the Python brain.
//!
//! Sends a SkillRequest to the Python SkillService, which loads the skill
//! definition, expands the Jinja2 template, and returns the expanded prompt.
//! The expanded prompt is then returned as the tool result for the model
//! to follow.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};
use crate::ipc::{IpcClient, IpcMessage};

pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &'static str { "Skill" }

    fn description(&self) -> &str {
        "Invoke a user-defined skill or prompt. Skills are loaded from \
         bundled definitions or user-created YAML files in .claude/skills/. \
         Supports fully qualified MCP names (e.g., \"ms-office-suite:pdf\")."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Skill name (e.g., 'commit', 'review', 'debug')"
                },
                "args": {
                    "type": "string",
                    "description": "Arguments to pass to the skill"
                }
            },
            "required": ["skill"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let skill_name = input.get("skill")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let args = input.get("args")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if skill_name.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: skill"));
        }

        let _ = output_tx.send(ToolOutput {
            text: format!("Loading skill: {skill_name}"),
            is_error: false,
        }).await;

        debug!("SkillTool: invoking {skill_name} with args={args}");

        // ── Send SkillRequest to Python brain via IPC ───────────────────
        let socket = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());

        let mut ipc = match IpcClient::connect_to(&socket).await {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "Cannot connect to agent-brain for skill execution: {e}. \
                     Is the Python IPC server running? (make dev-python)"
                )));
            }
        };

        let mut arguments = HashMap::new();
        if !args.is_empty() {
            arguments.insert("args".to_string(), Value::String(args.to_string()));
        }

        let request = IpcMessage::SkillRequest(crate::ipc::SkillRequest {
            request_id: IpcClient::new_request_id(),
            skill_name: skill_name.to_string(),
            arguments,
        });

        match ipc.request(request).await {
            Ok(IpcMessage::SkillResponse(resp)) => {
                if resp.content.is_empty() {
                    // Skill not found or empty response
                    let _ = output_tx.send(ToolOutput {
                        text: format!("Skill '{skill_name}' not found or returned empty"),
                        is_error: true,
                    }).await;
                    Ok(ToolResult::error(format!(
                        "Skill '{skill_name}' not found. Available skills can be listed with /skills."
                    )))
                } else {
                    let _ = output_tx.send(ToolOutput {
                        text: format!("Skill '{skill_name}' loaded ({} chars)", resp.content.len()),
                        is_error: false,
                    }).await;

                    // Return the expanded skill prompt as the tool result.
                    // The model will then follow the instructions in the prompt.
                    Ok(ToolResult::ok(resp.content))
                }
            }
            Ok(other) => {
                Ok(ToolResult::error(format!(
                    "Unexpected IPC response for skill '{skill_name}': {other:?}"
                )))
            }
            Err(e) => {
                Ok(ToolResult::error(format!(
                    "Skill IPC request failed: {e}"
                )))
            }
        }
    }
}
