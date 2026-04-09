//! TaskStopTool — stops a running background task.
//!
//! Mirrors `src/tools/TaskStopTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{SharedState, TaskStatus};

pub struct TaskStopTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &'static str { "TaskStop" }

    fn description(&self) -> &str {
        "Stop a running background task immediately."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ID of the task to stop"
                }
            },
            "required": ["task_id"]
        })
    }

    fn requires_permission(&self) -> bool { true }

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

        let mut state = self.state.write().await;
        match state.tasks.get_mut(task_id) {
            Some(task) => {
                if task.status != TaskStatus::InProgress {
                    return Ok(ToolResult::error(format!(
                        "Task {task_id} is not running (status: {:?})", task.status
                    )));
                }
                let subject = task.subject.clone();
                task.status = TaskStatus::Deleted;

                let _ = output_tx.send(ToolOutput {
                    text: format!("Stopped task {task_id}: {subject}"),
                    is_error: false,
                }).await;

                Ok(ToolResult::ok(json!({
                    "id": task_id,
                    "subject": subject,
                    "status": "stopped"
                }).to_string()))
            }
            None => Ok(ToolResult::error(format!("Task not found: {task_id}"))),
        }
    }
}
