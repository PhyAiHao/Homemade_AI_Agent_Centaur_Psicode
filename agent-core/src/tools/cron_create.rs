//! CronCreateTool — schedules a recurring or one-shot prompt on a cron schedule.
//!
//! Mirrors `src/tools/ScheduleCronTool/CronCreateTool.ts`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{CronJob, SharedState};

const MAX_CRON_JOBS: usize = 50;

pub struct CronCreateTool {
    pub state: SharedState,
}

/// Parse a 5-field cron expression into a human-readable string.
fn humanize_cron(cron: &str) -> String {
    let fields: Vec<&str> = cron.split_whitespace().collect();
    if fields.len() != 5 {
        return format!("Invalid cron: {cron}");
    }
    // Simple heuristic descriptions for common patterns
    match (fields[0], fields[1], fields[2], fields[3], fields[4]) {
        ("*", "*", "*", "*", "*") => "Every minute".to_string(),
        ("0", "*", "*", "*", "*") => "Every hour".to_string(),
        ("0", "0", "*", "*", "*") => "Every day at midnight".to_string(),
        (m, h, "*", "*", "*") => format!("Daily at {h}:{m:0>2}"),
        (m, h, "*", "*", d) => format!("At {h}:{m:0>2} on weekday {d}"),
        _ => format!("Cron: {cron}"),
    }
}

#[async_trait]
impl Tool for CronCreateTool {
    fn name(&self) -> &'static str { "CronCreate" }

    fn description(&self) -> &str {
        "Schedule a recurring or one-shot prompt on a cron schedule."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "cron": {
                    "type": "string",
                    "description": "5-field cron expression (minute hour day-of-month month day-of-week)"
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to enqueue at each fire time"
                },
                "recurring": {
                    "type": "boolean",
                    "description": "true = recurring, false = one-shot (default: true)"
                },
                "durable": {
                    "type": "boolean",
                    "description": "true = persist to disk (default: false)"
                }
            },
            "required": ["cron", "prompt"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let cron_expr = input.get("cron")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let prompt = input.get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let recurring = input.get("recurring")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let durable = input.get("durable")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if cron_expr.is_empty() || prompt.is_empty() {
            return Ok(ToolResult::error("Missing required parameters: cron and prompt"));
        }

        // Validate 5-field cron format
        let fields: Vec<&str> = cron_expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Ok(ToolResult::error(format!(
                "Invalid cron expression: expected 5 fields, got {}. Format: minute hour day-of-month month day-of-week",
                fields.len()
            )));
        }

        let mut state = self.state.write().await;

        // Check max jobs limit
        if state.cron_jobs.len() >= MAX_CRON_JOBS {
            return Ok(ToolResult::error(format!(
                "Maximum cron jobs ({MAX_CRON_JOBS}) reached. Delete some first."
            )));
        }

        let human = humanize_cron(cron_expr);
        let id = Uuid::new_v4().to_string();

        let job = CronJob {
            id: id.clone(),
            cron: cron_expr.to_string(),
            prompt: prompt.to_string(),
            recurring,
            durable,
            owner: None,
            human_schedule: human.clone(),
        };

        state.add_cron_job(job);

        let _ = output_tx.send(ToolOutput {
            text: format!("Scheduled cron job {id}: {human}"),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(json!({
            "id": id,
            "cron": cron_expr,
            "human_schedule": human,
            "recurring": recurring,
            "durable": durable,
        }).to_string()))
    }
}
