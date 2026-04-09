//! MCPTool — IPC proxy for MCP (Model Context Protocol) tool execution.
//!
//! Forwards arbitrary JSON input to the agent-integrations TypeScript layer
//! via IPC for execution by an MCP server.
#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct McpTool;

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &'static str { "MCPTool" }

    fn description(&self) -> &str {
        "Execute a tool provided by an MCP (Model Context Protocol) server. \
         Accepts arbitrary JSON input and forwards it to the connected MCP server."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "server_name": {
                    "type": "string",
                    "description": "Name of the MCP server to target"
                },
                "tool_name": {
                    "type": "string",
                    "description": "Name of the tool to invoke on the MCP server"
                },
                "input": {
                    "type": "object",
                    "description": "Arbitrary JSON input to pass to the MCP tool",
                    "additionalProperties": true
                }
            },
            "required": ["server_name", "tool_name"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, _input: Value, _tx: Sender<ToolOutput>) -> Result<ToolResult> {
        Ok(ToolResult::error(
            "MCPTool requires the agent-integrations TypeScript layer. Start with: make dev"
        ))
    }
}

// ─── Dynamic MCP Tool (registered at runtime from IPC discovery) ────────────

/// A dynamically registered MCP tool. Created when an MCP server advertises
/// its tools via IPC. Each instance wraps a specific server+tool pair.
///
/// This enables Principle 4 (Composability): MCP tools are registered into
/// the ToolRegistry at runtime and go through the same permission gate.
pub struct DynamicMcpTool {
    /// Unique name (format: "mcp_<server>_<tool>")
    tool_name: String,
    /// Human-readable description from the MCP server.
    tool_description: String,
    /// Input schema (JSON Schema) from the MCP server.
    schema: Value,
    /// MCP server name for routing.
    server_name: String,
    /// Original tool name on the MCP server.
    remote_tool_name: String,
}

impl DynamicMcpTool {
    /// Create a new dynamic MCP tool from server advertisement.
    pub fn new(
        server_name: &str,
        tool_name: &str,
        description: &str,
        input_schema: Value,
    ) -> Self {
        DynamicMcpTool {
            tool_name: format!("mcp_{server_name}_{tool_name}"),
            tool_description: description.to_string(),
            schema: input_schema,
            server_name: server_name.to_string(),
            remote_tool_name: tool_name.to_string(),
        }
    }

    /// Register tools from an MCP server's tool list into the registry.
    pub fn register_from_server(
        registry: &mut super::ToolRegistry,
        server_name: &str,
        tools: &[Value],
    ) -> usize {
        let mut count = 0;
        for tool_def in tools {
            let name = tool_def.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let desc = tool_def.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let schema = tool_def.get("input_schema").cloned().unwrap_or(json!({"type": "object"}));

            if name.is_empty() { continue; }

            let dynamic = DynamicMcpTool::new(server_name, name, desc, schema);
            if registry.add_tool(std::sync::Arc::new(dynamic)) {
                count += 1;
            }
        }
        count
    }
}

#[async_trait]
impl Tool for DynamicMcpTool {
    fn name(&self) -> &'static str {
        // SAFETY: We leak the string to get a 'static str.
        // This is acceptable because dynamic tools live for the process lifetime.
        Box::leak(self.tool_name.clone().into_boxed_str())
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        // Forward to the generic MCPTool with server_name and tool_name injected
        let _wrapped = json!({
            "server_name": self.server_name,
            "tool_name": self.remote_tool_name,
            "input": input,
        });
        // For now, same stub — would be replaced with real IPC forwarding
        let _ = tx.send(ToolOutput {
            text: format!("MCP: {}.{}", self.server_name, self.remote_tool_name),
            is_error: false,
        }).await;
        Ok(ToolResult::error(format!(
            "MCP tool {}.{} requires agent-integrations TypeScript layer",
            self.server_name, self.remote_tool_name
        )))
    }
}

