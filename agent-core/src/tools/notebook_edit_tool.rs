//! NotebookEditTool — IPC proxy for Jupyter notebook cell editing.
//!
//! Edits cells in Jupyter notebooks (.ipynb) by forwarding to the
//! agent-integrations TypeScript layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct NotebookEditTool;

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &'static str { "NotebookEdit" }

    fn description(&self) -> &str {
        "Edit cells in a Jupyter notebook. Supports replacing cell source, \
         changing cell type, and inserting/deleting cells."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Absolute path to the .ipynb notebook file"
                },
                "cell_id": {
                    "type": "string",
                    "description": "ID or index of the cell to edit"
                },
                "new_source": {
                    "type": "string",
                    "description": "New source content for the cell"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown", "raw"],
                    "description": "Cell type (code, markdown, or raw)"
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert_before", "insert_after", "delete"],
                    "description": "How to edit: replace cell content, insert before/after, or delete"
                }
            },
            "required": ["notebook_path", "cell_id"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "NotebookEdit requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}
