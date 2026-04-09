//! ToolSearchTool — searches available tools by keyword or direct selection.
//!
//! Mirrors `src/tools/ToolSearchTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &'static str { "ToolSearch" }

    fn description(&self) -> &str {
        "Search available tools by keyword or direct selection. \
         Use \"select:ToolA,ToolB\" for direct lookup, or keywords to search."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use \"select:Name1,Name2\" for direct or keywords."
                },
                "max_results": {
                    "type": "number",
                    "description": "Maximum number of results (default: 5)"
                }
            },
            "required": ["query"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let query = input.get("query")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let max_results = input.get("max_results")
            .and_then(|v| v.as_u64())
            .unwrap_or(5) as usize;

        if query.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: query"));
        }

        // Direct selection mode: "select:ToolA,ToolB"
        if let Some(names) = query.strip_prefix("select:") {
            let selected: Vec<&str> = names.split(',').map(|s| s.trim()).collect();
            let _ = output_tx.send(ToolOutput {
                text: format!("Selected tools: {}", selected.join(", ")),
                is_error: false,
            }).await;
            // In the full implementation, this would return full tool schemas
            // for the requested tools from the deferred tool registry.
            let results: Vec<Value> = selected.iter().map(|name| {
                json!({ "name": name, "status": "loaded" })
            }).collect();
            return Ok(ToolResult {
                content: serde_json::to_string(&results).unwrap(),
                is_error: false,
                metadata: Some(json!({ "matched_tools": results })),
            });
        }

        // Keyword search mode — score tools by name/description match
        // In the full implementation, this searches all registered + deferred tools.
        // For now, return a placeholder indicating the search was performed.
        let keywords: Vec<&str> = query.split_whitespace().collect();
        let _ = output_tx.send(ToolOutput {
            text: format!("Searching tools for: {} (max {max_results})", keywords.join(" ")),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(format!(
            "Tool search for \"{query}\" — results depend on deferred tool registry"
        )))
    }
}
