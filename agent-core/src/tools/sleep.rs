//! SleepTool — pauses execution for a specified duration.
//!
//! Mirrors `src/tools/SleepTool/`. Does not hold a shell process.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &'static str { "Sleep" }

    fn description(&self) -> &str {
        "Pauses execution for the specified duration in milliseconds. Use this instead of \
         busy-waiting in a shell."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "duration": {
                    "type": "number",
                    "description": "Duration to sleep in milliseconds"
                },
                "reason": {
                    "type": "string",
                    "description": "Why the tool is sleeping"
                }
            },
            "required": ["duration"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let duration_ms = input.get("duration")
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0) as u64;
        let reason = input.get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("no reason given");

        // Cap at 10 minutes
        let duration_ms = duration_ms.min(600_000);

        let _ = output_tx.send(ToolOutput {
            text: format!("Sleeping for {duration_ms}ms ({reason})"),
            is_error: false,
        }).await;

        tokio::time::sleep(std::time::Duration::from_millis(duration_ms)).await;

        Ok(ToolResult::ok(format!("Slept for {duration_ms}ms")))
    }
}
