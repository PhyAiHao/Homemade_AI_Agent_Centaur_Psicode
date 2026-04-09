//! EnterPlanModeTool — switches to plan mode (read-only exploration).
//!
//! Mirrors `src/tools/EnterPlanModeTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct EnterPlanModeTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &'static str { "EnterPlanMode" }

    fn description(&self) -> &str {
        "Enter plan mode for exploration and design before implementation. \
         In plan mode, write operations are blocked and only read/search tools work."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        _input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let mut state = self.state.write().await;

        if state.plan_mode {
            return Ok(ToolResult::error("Already in plan mode."));
        }

        state.plan_mode = true;

        let _ = output_tx.send(ToolOutput {
            text: "Entered plan mode — write operations are now blocked.".to_string(),
            is_error: false,
        }).await;

        Ok(ToolResult::ok("Plan mode activated. Only read/search tools are available."))
    }
}
