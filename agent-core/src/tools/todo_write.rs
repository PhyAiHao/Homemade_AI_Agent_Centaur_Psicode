//! TodoWriteTool — manages the session's task checklist.
//!
//! Mirrors `src/tools/TodoWriteTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &'static str { "TodoWrite" }

    fn description(&self) -> &str {
        "Create and manage a structured task list for the current session. \
         Use to track progress on multi-step tasks."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "The updated todo list",
                    "items": {
                        "type": "object",
                        "properties": {
                            "content": {
                                "type": "string",
                                "description": "Task description (imperative form)"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed"]
                            },
                            "activeForm": {
                                "type": "string",
                                "description": "Present continuous form for display"
                            }
                        },
                        "required": ["content", "status"]
                    }
                }
            },
            "required": ["todos"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let todos = input.get("todos")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let total = todos.len();
        let completed = todos.iter()
            .filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("completed"))
            .count();
        let in_progress = todos.iter()
            .filter(|t| t.get("status").and_then(|s| s.as_str()) == Some("in_progress"))
            .count();

        // Show a brief summary
        let _ = output_tx.send(ToolOutput {
            text: format!("Todos updated: {completed}/{total} done, {in_progress} in progress"),
            is_error: false,
        }).await;

        // If all completed, note it
        if total > 0 && completed == total {
            let _ = output_tx.send(ToolOutput {
                text: "All tasks completed!".to_string(),
                is_error: false,
            }).await;
        }

        Ok(ToolResult {
            content: json!({
                "total": total,
                "completed": completed,
                "in_progress": in_progress,
                "todos": todos,
            }).to_string(),
            is_error: false,
            metadata: None,
        })
    }
}
