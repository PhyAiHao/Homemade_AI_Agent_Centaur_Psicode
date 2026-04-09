//! McpAuthTool — IPC proxy for MCP server authentication.
//!
//! Handles OAuth/API key authentication flows for MCP servers
//! by forwarding to the agent-integrations TypeScript layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct McpAuthTool;

#[async_trait]
impl Tool for McpAuthTool {
    fn name(&self) -> &'static str { "McpAuthTool" }

    fn description(&self) -> &str {
        "Authenticate with an MCP server. Initiates the OAuth or API key \
         authentication flow for a specified MCP server."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server to authenticate with"
                },
                "auth_type": {
                    "type": "string",
                    "enum": ["oauth", "api_key"],
                    "description": "Authentication type to use"
                },
                "credentials": {
                    "type": "object",
                    "description": "Authentication credentials (structure depends on auth_type)",
                    "additionalProperties": true
                }
            },
            "required": ["server_name"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "McpAuthTool requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}
