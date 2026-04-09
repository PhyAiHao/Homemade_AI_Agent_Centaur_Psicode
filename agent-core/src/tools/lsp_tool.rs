//! LSPTool — IPC proxy for Language Server Protocol operations.
//!
//! Provides access to LSP features (go-to-definition, hover, references, etc.)
//! by forwarding to the agent-integrations TypeScript layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct LspTool;

#[async_trait]
impl Tool for LspTool {
    fn name(&self) -> &'static str { "LSPTool" }

    fn description(&self) -> &str {
        "Interact with a Language Server Protocol server. Supports operations like \
         go-to-definition, hover, find references, and diagnostics."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": [
                        "definition",
                        "hover",
                        "references",
                        "completion",
                        "diagnostics",
                        "rename",
                        "codeAction"
                    ],
                    "description": "The LSP operation to perform"
                },
                "filePath": {
                    "type": "string",
                    "description": "Absolute path to the file"
                },
                "line": {
                    "type": "integer",
                    "description": "Zero-indexed line number in the file"
                },
                "character": {
                    "type": "integer",
                    "description": "Zero-indexed character offset in the line"
                },
                "newName": {
                    "type": "string",
                    "description": "New name for rename operations"
                }
            },
            "required": ["operation", "filePath", "line", "character"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "LSPTool requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}
