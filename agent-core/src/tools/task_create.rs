//! TaskCreateTool — creates a new task in the task system.
//!
//! Mirrors `src/tools/TaskCreateTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct TaskCreateTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &'static str { "TaskCreate" }

    fn description(&self) -> &str {
        "Create a new task with subject, description, and optional metadata."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Brief task title"
                },
                "description": {
                    "type": "string",
                    "description": "What needs to be done"
                },
                "activeForm": {
                    "type": "string",
                    "description": "Present continuous form (e.g., \"Running tests\")"
                },
                "metadata": {
                    "type": "object",
                    "description": "Custom metadata"
                }
            },
            "required": ["subject", "description"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let subject = input.get("subject")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let description = input.get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let active_form = input.get("activeForm")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        if subject.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: subject"));
        }

        let metadata: HashMap<String, serde_json::Value> = input.get("metadata")
            .and_then(|v| serde_json::from_value(v.clone()).ok())
            .unwrap_or_default();

        let mut state = self.state.write().await;
        let id = state.create_task(subject.clone(), description, active_form, metadata);

        let _ = output_tx.send(ToolOutput {
            text: format!("Created task {id}: {subject}"),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(json!({
            "id": id,
            "subject": subject,
            "status": "pending"
        }).to_string()))
    }
}
