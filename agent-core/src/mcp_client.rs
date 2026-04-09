//! MCP (Model Context Protocol) client — manages MCP server processes,
//! discovers tools, and forwards tool execution requests.
//!
//! This implements the full MCP integration pipeline:
//! 1. Load server configs from .claude/settings.json
//! 2. Spawn server processes (stdio transport)
//! 3. Perform JSON-RPC 2.0 initialize handshake
//! 4. Discover tools via tools/list
//! 5. Register tools as DynamicMcpTool in ToolRegistry
//! 6. Forward tool calls via tools/call
//!
//! Protocol: JSON-RPC 2.0 over stdin/stdout with Content-Length headers.
#![allow(dead_code)]

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

// ─── MCP Server Config ─────────────────────────────────────────────────────

/// Configuration for a single MCP server, from settings.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub cwd: Option<String>,
    /// Transport type: "stdio" (default), "sse", "http"
    #[serde(default = "default_transport")]
    pub transport: String,
    /// URL for SSE/HTTP transports.
    pub url: Option<String>,
}

fn default_transport() -> String { "stdio".into() }

/// Load MCP server configs from settings files.
pub fn load_mcp_configs() -> Vec<McpServerConfig> {
    let mut configs = Vec::new();

    // Check .claude/settings.json and ~/.claude/settings.json
    let search_paths = [
        std::env::current_dir().ok().map(|d| d.join(".claude").join("settings.json")),
        std::env::current_dir().ok().map(|d| d.join(".claude").join("settings.local.json")),
        dirs::home_dir().map(|h| h.join(".claude").join("settings.json")),
    ];

    for path in search_paths.iter().flatten() {
        if let Ok(content) = std::fs::read_to_string(path) {
            if let Ok(json) = serde_json::from_str::<Value>(&content) {
                if let Some(servers) = json.get("mcpServers").and_then(|v| v.as_object()) {
                    for (name, config) in servers {
                        let mut server_config: McpServerConfig = match serde_json::from_value(config.clone()) {
                            Ok(c) => c,
                            Err(e) => {
                                warn!(name = %name, error = %e, "Failed to parse MCP server config");
                                continue;
                            }
                        };
                        server_config.name = name.clone();
                        configs.push(server_config);
                    }
                }
            }
        }
    }

    // Deduplicate by name (last wins)
    let mut seen = HashMap::new();
    for config in configs {
        seen.insert(config.name.clone(), config);
    }

    seen.into_values().collect()
}

// ─── JSON-RPC 2.0 ──────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    id: u64,
    method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<u64>,
    result: Option<Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

// ─── MCP Connection ─────────────────────────────────────────────────────────

/// A single MCP server connection.
pub struct McpConnection {
    pub config: McpServerConfig,
    child: Child,
    stdin: tokio::process::ChildStdin,
    reader: BufReader<tokio::process::ChildStdout>,
    next_id: u64,
    /// Tools discovered from this server.
    pub tools: Vec<McpToolDef>,
    /// Server capabilities from initialize.
    pub server_info: Option<Value>,
}

/// A tool definition from an MCP server.
#[derive(Debug, Clone)]
pub struct McpToolDef {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
}

impl McpConnection {
    /// Spawn an MCP server process and connect via stdio.
    pub async fn connect(config: McpServerConfig) -> Result<Self> {
        if config.transport != "stdio" {
            bail!("Only stdio transport is supported. Got: {}", config.transport);
        }

        info!(name = %config.name, command = %config.command, "Starting MCP server");

        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args);
        cmd.stdin(Stdio::piped());
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::null());

        // Set environment
        for (key, value) in &config.env {
            cmd.env(key, value);
        }
        if let Some(ref cwd) = config.cwd {
            cmd.current_dir(cwd);
        }

        let mut child = cmd.spawn()
            .with_context(|| format!("Failed to spawn MCP server: {} {}", config.command, config.args.join(" ")))?;

        let stdin = child.stdin.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture MCP server stdin"))?;
        let stdout = child.stdout.take()
            .ok_or_else(|| anyhow::anyhow!("Failed to capture MCP server stdout"))?;

        let mut conn = McpConnection {
            config,
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
            tools: Vec::new(),
            server_info: None,
        };

        // Perform MCP initialize handshake
        conn.initialize().await?;

        // Discover tools
        conn.discover_tools().await?;

        Ok(conn)
    }

    /// Send a JSON-RPC request and read the response.
    async fn request(&mut self, method: &str, params: Option<Value>) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        };

        let body = serde_json::to_string(&request)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);

        self.stdin.write_all(message.as_bytes()).await?;
        self.stdin.flush().await?;

        debug!(method = %method, id = id, "MCP request sent");

        // Read response (Content-Length header + body)
        let response = self.read_response().await?;

        if let Some(error) = response.error {
            bail!("MCP error ({}): {}", error.code, error.message);
        }

        response.result.ok_or_else(|| anyhow::anyhow!("MCP response missing result"))
    }

    /// Read a JSON-RPC response from stdout.
    async fn read_response(&mut self) -> Result<JsonRpcResponse> {
        // Read Content-Length header
        let mut header_line = String::new();
        loop {
            header_line.clear();
            self.reader.read_line(&mut header_line).await?;
            let trimmed = header_line.trim();
            if trimmed.is_empty() {
                continue; // skip blank lines between messages
            }
            if trimmed.starts_with("Content-Length:") {
                break;
            }
            // Might be a notification or log line — skip
            if trimmed.starts_with('{') {
                // Some servers send JSON without Content-Length headers
                let response: JsonRpcResponse = serde_json::from_str(trimmed)?;
                return Ok(response);
            }
        }

        let content_length: usize = header_line
            .trim()
            .strip_prefix("Content-Length:")
            .unwrap_or("0")
            .trim()
            .parse()
            .unwrap_or(0);

        // Read the blank line separator
        let mut blank = String::new();
        self.reader.read_line(&mut blank).await?;

        // Read the body
        let mut body = vec![0u8; content_length];
        tokio::io::AsyncReadExt::read_exact(&mut self.reader, &mut body).await?;

        let response: JsonRpcResponse = serde_json::from_slice(&body)?;
        Ok(response)
    }

    /// MCP initialize handshake.
    async fn initialize(&mut self) -> Result<()> {
        let result = self.request("initialize", Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {
                "roots": { "listChanged": true }
            },
            "clientInfo": {
                "name": "centaur-agent",
                "version": env!("CARGO_PKG_VERSION")
            }
        }))).await?;

        self.server_info = Some(result.clone());

        info!(
            name = %self.config.name,
            server = %result.get("serverInfo").and_then(|s| s.get("name")).and_then(|n| n.as_str()).unwrap_or("unknown"),
            "MCP server initialized"
        );

        // Send initialized notification (no response expected)
        let notify = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let body = serde_json::to_string(&notify)?;
        let message = format!("Content-Length: {}\r\n\r\n{}", body.len(), body);
        self.stdin.write_all(message.as_bytes()).await?;
        self.stdin.flush().await?;

        Ok(())
    }

    /// Discover tools from the MCP server.
    async fn discover_tools(&mut self) -> Result<()> {
        let result = self.request("tools/list", None).await?;

        if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
            for tool_val in tools {
                let name = tool_val.get("name").and_then(|n| n.as_str()).unwrap_or("").to_string();
                let description = tool_val.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let input_schema = tool_val.get("inputSchema").cloned().unwrap_or(json!({"type": "object"}));

                if !name.is_empty() {
                    self.tools.push(McpToolDef { name, description, input_schema });
                }
            }
        }

        info!(
            name = %self.config.name,
            tools = self.tools.len(),
            "MCP tools discovered"
        );

        Ok(())
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, tool_name: &str, arguments: Value) -> Result<Value> {
        let result = self.request("tools/call", Some(json!({
            "name": tool_name,
            "arguments": arguments
        }))).await?;

        Ok(result)
    }

    /// Shut down the MCP server gracefully.
    pub async fn shutdown(&mut self) {
        // Try graceful shutdown
        let _ = self.request("shutdown", None).await;
        // Kill process if still running
        let _ = self.child.kill().await;
    }
}

// ─── MCP Manager ────────────────────────────────────────────────────────────

/// Manages all MCP server connections.
pub struct McpManager {
    connections: HashMap<String, Arc<Mutex<McpConnection>>>,
}

impl McpManager {
    pub fn new() -> Self {
        McpManager {
            connections: HashMap::new(),
        }
    }

    /// Start all configured MCP servers and discover their tools.
    pub async fn start_all(&mut self) -> Vec<(String, Vec<McpToolDef>)> {
        let configs = load_mcp_configs();
        let mut all_tools = Vec::new();

        if configs.is_empty() {
            debug!("No MCP servers configured");
            return all_tools;
        }

        info!(count = configs.len(), "Starting MCP servers");

        for config in configs {
            let name = config.name.clone();
            match McpConnection::connect(config).await {
                Ok(conn) => {
                    let tools = conn.tools.clone();
                    all_tools.push((name.clone(), tools));
                    self.connections.insert(name, Arc::new(Mutex::new(conn)));
                }
                Err(e) => {
                    warn!(name = %name, error = %e, "Failed to start MCP server");
                }
            }
        }

        all_tools
    }

    /// Register all discovered MCP tools into a ToolRegistry.
    pub fn register_tools(
        &self,
        registry: &mut crate::tools::ToolRegistry,
        all_tools: &[(String, Vec<McpToolDef>)],
    ) -> usize {
        let mut count = 0;
        for (server_name, tools) in all_tools {
            for tool in tools {
                let mcp_tool = LiveMcpTool {
                    tool_name: format!("mcp__{}__{}", server_name, tool.name),
                    tool_description: tool.description.clone(),
                    schema: tool.input_schema.clone(),
                    server_name: server_name.clone(),
                    remote_tool_name: tool.name.clone(),
                    manager: self.get_connection(server_name),
                };
                if registry.add_tool(Arc::new(mcp_tool)) {
                    count += 1;
                }
            }
        }
        info!(count, "MCP tools registered in ToolRegistry");
        count
    }

    /// Get a connection handle for a server.
    fn get_connection(&self, server_name: &str) -> Option<Arc<Mutex<McpConnection>>> {
        self.connections.get(server_name).cloned()
    }

    /// Shut down all MCP servers.
    pub async fn shutdown_all(&mut self) {
        for (name, conn) in self.connections.drain() {
            info!(name = %name, "Shutting down MCP server");
            conn.lock().await.shutdown().await;
        }
    }

    /// List connected servers and their tool counts.
    pub fn server_summary(&self) -> Vec<(String, usize)> {
        self.connections.keys().map(|name| {
            (name.clone(), 0) // tool count would need async lock
        }).collect()
    }
}

impl Default for McpManager {
    fn default() -> Self { Self::new() }
}

// ─── Live MCP Tool (executes via the manager) ───────────────────────────────

/// A dynamically registered MCP tool that actually forwards execution
/// to the MCP server process via JSON-RPC.
struct LiveMcpTool {
    tool_name: String,
    tool_description: String,
    schema: Value,
    server_name: String,
    remote_tool_name: String,
    manager: Option<Arc<Mutex<McpConnection>>>,
}

#[async_trait::async_trait]
impl crate::tools::Tool for LiveMcpTool {
    fn name(&self) -> &'static str {
        Box::leak(self.tool_name.clone().into_boxed_str())
    }

    fn description(&self) -> &str {
        &self.tool_description
    }

    fn input_schema(&self) -> Value {
        self.schema.clone()
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        tx: tokio::sync::mpsc::Sender<crate::tools::ToolOutput>,
    ) -> Result<crate::tools::ToolResult> {
        let conn = match &self.manager {
            Some(c) => c.clone(),
            None => {
                return Ok(crate::tools::ToolResult::error(format!(
                    "MCP server '{}' is not connected", self.server_name
                )));
            }
        };

        let _ = tx.send(crate::tools::ToolOutput {
            text: format!("MCP: {}.{}", self.server_name, self.remote_tool_name),
            is_error: false,
        }).await;

        let mut connection = conn.lock().await;
        match connection.call_tool(&self.remote_tool_name, input).await {
            Ok(result) => {
                // Extract text content from MCP result
                let content = if let Some(content_arr) = result.get("content").and_then(|c| c.as_array()) {
                    content_arr.iter()
                        .filter_map(|block| {
                            if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                                block.get("text").and_then(|t| t.as_str())
                            } else {
                                None
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n")
                } else {
                    result.to_string()
                };

                let is_error = result.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);

                let _ = tx.send(crate::tools::ToolOutput {
                    text: if content.len() > 200 {
                        format!("{}...", &content[..200])
                    } else {
                        content.clone()
                    },
                    is_error,
                }).await;

                if is_error {
                    Ok(crate::tools::ToolResult::error(content))
                } else {
                    Ok(crate::tools::ToolResult::ok(content))
                }
            }
            Err(e) => {
                Ok(crate::tools::ToolResult::error(format!(
                    "MCP tool call failed: {e}"
                )))
            }
        }
    }
}
