//! TaskOutputTool — retrieves output from a background task.
//!
//! Mirrors `src/tools/TaskOutputTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct TaskOutputTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskOutputTool {
    fn name(&self) -> &'static str { "TaskOutput" }

    fn description(&self) -> &str {
        "Return the output from a background task execution."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ID of the background task"
                }
            },
            "required": ["task_id"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let task_id = input.get("task_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if task_id.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: task_id"));
        }

        let state = self.state.read().await;
        match state.get_task(task_id) {
            Some(task) => {
                match &task.output {
                    Some(out) => {
                        let text = if out.stderr.is_empty() {
                            out.stdout.clone()
                        } else {
                            format!("{}\n[stderr]\n{}", out.stdout, out.stderr)
                        };
                        let _ = output_tx.send(ToolOutput {
                            text: text.clone(),
                            is_error: false,
                        }).await;
                        Ok(ToolResult {
                            content: text,
                            is_error: false,
                            metadata: Some(json!({
                                "exit_code": out.exit_code,
                            })),
                        })
                    }
                    None => Ok(ToolResult::ok(format!(
                        "Task {task_id} ({}) has no output yet (status: {:?})",
                        task.subject, task.status
                    ))),
                }
            }
            None => Ok(ToolResult::error(format!("Task not found: {task_id}"))),
        }
    }
}
