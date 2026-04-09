//! TaskGetTool — retrieves a task by ID.
//!
//! Mirrors `src/tools/TaskGetTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct TaskGetTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &'static str { "TaskGet" }

    fn description(&self) -> &str {
        "Retrieve a task by ID with full details including status and dependencies."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "The task ID to fetch"
                }
            },
            "required": ["taskId"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        _output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let task_id = input.get("taskId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if task_id.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: taskId"));
        }

        let state = self.state.read().await;
        match state.get_task(task_id) {
            Some(task) => {
                Ok(ToolResult::ok(serde_json::to_string_pretty(task)
                    .unwrap_or_else(|_| "{}".to_string())))
            }
            None => Ok(ToolResult::error(format!("Task not found: {task_id}"))),
        }
    }
}
