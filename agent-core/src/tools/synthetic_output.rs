//! SyntheticOutputTool — returns structured JSON output for SDK/CLI mode.
//!
//! Mirrors `src/tools/SyntheticOutputTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct SyntheticOutputTool;

#[async_trait]
impl Tool for SyntheticOutputTool {
    fn name(&self) -> &'static str { "SyntheticOutput" }

    fn description(&self) -> &str {
        "Return structured JSON output in non-interactive SDK/CLI mode. \
         The input object is passed through as the structured output."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "description": "Passthrough object — any fields are returned as structured output",
            "additionalProperties": true
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        _output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        Ok(ToolResult {
            content: serde_json::to_string_pretty(&input)
                .unwrap_or_else(|_| "{}".to_string()),
            is_error: false,
            metadata: Some(json!({ "structured_output": input })),
        })
    }
}
