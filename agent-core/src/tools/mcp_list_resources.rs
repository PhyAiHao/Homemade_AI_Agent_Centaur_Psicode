//! ListMcpResourcesTool — IPC proxy for listing MCP server resources.
//!
//! Enumerates resources available on a connected MCP server
//! by forwarding to the agent-integrations TypeScript layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct ListMcpResourcesTool;

#[async_trait]
impl Tool for ListMcpResourcesTool {
    fn name(&self) -> &'static str { "ListMcpResources" }

    fn description(&self) -> &str {
        "List resources available on an MCP server. Returns the resource URIs, \
         names, and descriptions provided by the server."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server to query"
                },
                "cursor": {
                    "type": "string",
                    "description": "Pagination cursor for fetching next page of results"
                }
            },
            "required": ["server_name"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "ListMcpResources requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}
