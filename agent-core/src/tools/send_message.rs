//! SendMessageTool — sends messages between teammates or to the team lead.
//!
//! Mirrors `src/tools/SendMessageTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct SendMessageTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &'static str { "SendMessage" }

    fn description(&self) -> &str {
        "Send a message to a teammate, the team lead, or broadcast to all. \
         Use \"*\" for broadcast."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient: teammate name, \"*\" for broadcast"
                },
                "summary": {
                    "type": "string",
                    "description": "5-10 word preview of the message"
                },
                "message": {
                    "description": "Message content (string or structured protocol message)"
                }
            },
            "required": ["to", "message"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let to = input.get("to")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let summary = input.get("summary")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let message = input.get("message").cloned().unwrap_or(Value::Null);

        if to.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: to"));
        }

        let message_text = match &message {
            Value::String(s) => s.clone(),
            other => serde_json::to_string(other).unwrap_or_default(),
        };

        let mut state = self.state.write().await;

        // Check we have a team
        if state.team.is_none() {
            return Ok(ToolResult::error("Not part of a team. Create a team first."));
        }

        if to == "*" {
            // Broadcast to all team members
            let team = match state.team.as_ref() {
                Some(t) => t,
                None => return Ok(ToolResult::error("Team state is missing")),
            };
            let member_names: Vec<String> = team.members.iter()
                .map(|m| m.name.clone())
                .collect();

            for name in &member_names {
                state.send_to_mailbox(name, message_text.clone());
            }

            let _ = output_tx.send(ToolOutput {
                text: format!("Broadcast to {} members: {summary}", member_names.len()),
                is_error: false,
            }).await;

            Ok(ToolResult::ok(json!({
                "delivered_to": member_names,
                "broadcast": true,
            }).to_string()))
        } else {
            // Send to specific recipient
            state.send_to_mailbox(to, message_text);

            let _ = output_tx.send(ToolOutput {
                text: format!("Sent to {to}: {summary}"),
                is_error: false,
            }).await;

            Ok(ToolResult::ok(json!({
                "delivered_to": to,
                "broadcast": false,
            }).to_string()))
        }
    }
}
