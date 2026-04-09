//! ReadMcpResourceTool — IPC proxy for reading an MCP server resource.
//!
//! Retrieves the content of a specific resource from a connected MCP server
//! by forwarding to the agent-integrations TypeScript layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct ReadMcpResourceTool;

#[async_trait]
impl Tool for ReadMcpResourceTool {
    fn name(&self) -> &'static str { "ReadMcpResource" }

    fn description(&self) -> &str {
        "Read the content of a resource from an MCP server. Returns the resource \
         content (text, binary, or structured data) identified by its URI."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server that hosts the resource"
                },
                "uri": {
                    "type": "string",
                    "description": "URI of the resource to read"
                }
            },
            "required": ["server_name", "uri"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "ReadMcpResource requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}
