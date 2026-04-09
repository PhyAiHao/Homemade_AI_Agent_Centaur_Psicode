//! TaskListTool — lists all active tasks.
//!
//! Mirrors `src/tools/TaskListTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct TaskListTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &'static str { "TaskList" }

    fn description(&self) -> &str {
        "List all active tasks, filtering out completed dependencies."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        _input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let state = self.state.read().await;
        let tasks = state.list_active_tasks();

        let task_list: Vec<Value> = tasks.iter().map(|t| {
            json!({
                "id": t.id,
                "subject": t.subject,
                "status": t.status,
                "owner": t.owner,
                "blocked_by": t.blocked_by,
            })
        }).collect();

        let _ = output_tx.send(ToolOutput {
            text: format!("{} active task(s)", task_list.len()),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(serde_json::to_string_pretty(&task_list)
            .unwrap_or_else(|_| "[]".to_string())))
    }
}
