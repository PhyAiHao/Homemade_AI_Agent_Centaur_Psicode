//! AskUserQuestionTool — prompts the user with a question and optional choices.
//!
//! Mirrors `src/tools/AskUserQuestionTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct AskUserQuestionTool;

#[async_trait]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &'static str { "AskUserQuestion" }

    fn description(&self) -> &str {
        "Ask the user a question with optional multiple-choice options. Use when \
         you need clarification or user input to proceed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                },
                "options": {
                    "type": "array",
                    "description": "Optional list of choices",
                    "items": {
                        "type": "object",
                        "properties": {
                            "label": { "type": "string" },
                            "description": { "type": "string" },
                            "preview": { "type": "string" }
                        },
                        "required": ["label"]
                    }
                },
                "multiSelect": {
                    "type": "boolean",
                    "description": "Allow multiple selections"
                },
                "defaultOption": {
                    "type": "string",
                    "description": "Default choice label"
                }
            },
            "required": ["question"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let question = input.get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("No question provided");

        // Render the question to the TUI
        let _ = output_tx.send(ToolOutput {
            text: format!("❓ {question}"),
            is_error: false,
        }).await;

        // If options provided, render them
        if let Some(options) = input.get("options").and_then(|v| v.as_array()) {
            for (i, opt) in options.iter().enumerate() {
                let label = opt.get("label").and_then(|v| v.as_str()).unwrap_or("?");
                let desc = opt.get("description")
                    .and_then(|v| v.as_str())
                    .map(|d| format!(" — {d}"))
                    .unwrap_or_default();
                let _ = output_tx.send(ToolOutput {
                    text: format!("  {}. {label}{desc}", i + 1),
                    is_error: false,
                }).await;
            }
        }

        // In a real implementation, this would block on user input via the TUI
        // input system. For now, return the question as the tool result so the
        // LLM sees the question was presented.
        Ok(ToolResult::ok(format!("[Waiting for user response to: {question}]")))
    }
}
