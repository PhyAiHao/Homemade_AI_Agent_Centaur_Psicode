//! ExitPlanModeTool — exits plan mode and presents plan for approval.
//!
//! Mirrors `src/tools/ExitPlanModeTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct ExitPlanModeTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &'static str { "ExitPlanMode" }

    fn description(&self) -> &str {
        "Exit plan mode and present the plan for user approval."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "allowedPrompts": {
                    "type": "array",
                    "description": "Semantic permission requests (tool + prompt description)",
                    "items": {
                        "type": "object",
                        "properties": {
                            "tool": { "type": "string" },
                            "prompt": { "type": "string" }
                        }
                    }
                }
            }
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        _input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let mut state = self.state.write().await;

        if !state.plan_mode {
            return Ok(ToolResult::error("Not currently in plan mode."));
        }

        state.plan_mode = false;

        let _ = output_tx.send(ToolOutput {
            text: "Exited plan mode — write operations are now available.".to_string(),
            is_error: false,
        }).await;

        Ok(ToolResult::ok("Plan mode deactivated. Full tool access restored."))
    }
}
