//! CronScheduleTool — lists and manages scheduled cron jobs.
//!
//! Combines CronListTool and CronDeleteTool from the original source.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct CronScheduleTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for CronScheduleTool {
    fn name(&self) -> &'static str { "CronSchedule" }

    fn description(&self) -> &str {
        "List or delete scheduled cron jobs."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "delete"],
                    "description": "'list' to view all jobs, 'delete' to remove one by ID"
                },
                "id": {
                    "type": "string",
                    "description": "Job ID to delete (required for 'delete' action)"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let action = input.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("list");

        match action {
            "list" => {
                let state = self.state.read().await;
                let jobs: Vec<Value> = state.list_cron_jobs().iter().map(|j| {
                    json!({
                        "id": j.id,
                        "cron": j.cron,
                        "human_schedule": j.human_schedule,
                        "prompt": j.prompt,
                        "recurring": j.recurring,
                        "durable": j.durable,
                    })
                }).collect();

                let _ = output_tx.send(ToolOutput {
                    text: format!("{} scheduled cron job(s)", jobs.len()),
                    is_error: false,
                }).await;

                Ok(ToolResult::ok(serde_json::to_string_pretty(&jobs)
                    .unwrap_or_else(|_| "[]".to_string())))
            }
            "delete" => {
                let id = input.get("id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");

                if id.is_empty() {
                    return Ok(ToolResult::error("Missing required parameter: id"));
                }

                let mut state = self.state.write().await;
                match state.remove_cron_job(id) {
                    Some(job) => {
                        let _ = output_tx.send(ToolOutput {
                            text: format!("Deleted cron job {id}: {}", job.human_schedule),
                            is_error: false,
                        }).await;
                        Ok(ToolResult::ok(json!({ "id": id, "status": "deleted" }).to_string()))
                    }
                    None => Ok(ToolResult::error(format!("Cron job not found: {id}"))),
                }
            }
            _ => Ok(ToolResult::error(format!("Invalid action: {action}. Use 'list' or 'delete'."))),
        }
    }
}
