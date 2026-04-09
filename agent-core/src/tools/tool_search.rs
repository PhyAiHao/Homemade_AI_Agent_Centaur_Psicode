//! ToolSearchTool — searches available tools by keyword or direct selection.
//!
//! When deferred tool loading is active, only core tools (11) are sent to the
//! LLM API. The remaining ~35 tools are discoverable via this tool. The LLM
//! calls ToolSearch to find and load deferred tools by name or keyword.
//!
//! The full tool catalog is populated after registry construction via
//! `set_full_catalog()`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::OnceLock;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

/// Full catalog of ALL tools (core + deferred). Set once after registry init.
static FULL_TOOL_CATALOG: OnceLock<Vec<ToolInfo>> = OnceLock::new();

/// Info about a tool — stored in the global catalog for ToolSearch queries.
#[derive(Clone, Debug)]
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub is_core: bool,
}

/// Populate the global tool catalog. Called once after ToolRegistry is built.
pub fn set_full_catalog(tools: Vec<ToolInfo>) {
    let _ = FULL_TOOL_CATALOG.set(tools);
}

/// Get deferred (non-core) tools only.
fn deferred_tools() -> &'static [ToolInfo] {
    FULL_TOOL_CATALOG.get().map(|v| v.as_slice()).unwrap_or(&[])
}

pub struct ToolSearchTool;

#[async_trait]
impl Tool for ToolSearchTool {
    fn name(&self) -> &'static str { "ToolSearch" }

    fn description(&self) -> &str {
        "Search for and load deferred tools by name or keyword. \
         Not all tools are loaded by default — use this to discover tools for \
         planning, scheduling, web search, MCP, notebooks, teams, and more. \
         Use \"select:Name1,Name2\" for direct lookup, or keywords to search. \
         Returns full tool schemas that the system will load for this session."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search query. Use \"select:Name1,Name2\" for direct or keywords to search."
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

        let catalog = deferred_tools();

        // ── Direct selection: "select:ToolA,ToolB" ─────────────────────
        if let Some(names) = query.strip_prefix("select:") {
            let selected: Vec<&str> = names.split(',').map(|s| s.trim()).collect();
            let mut results: Vec<Value> = Vec::new();

            for name in &selected {
                if let Some(info) = catalog.iter().find(|t| t.name.eq_ignore_ascii_case(name)) {
                    results.push(json!({
                        "name": info.name,
                        "description": info.description,
                        "input_schema": info.input_schema,
                        "status": if info.is_core { "already_loaded" } else { "loaded" },
                    }));
                } else {
                    results.push(json!({
                        "name": name,
                        "status": "not_found",
                    }));
                }
            }

            let _ = output_tx.send(ToolOutput {
                text: format!("Loaded {} tool(s): {}",
                    results.iter().filter(|r| r["status"] != "not_found").count(),
                    selected.join(", ")),
                is_error: false,
            }).await;

            // Return as a <functions> block that the system prompt parses
            let schema_block = results.iter()
                .filter(|r| r["status"] != "not_found")
                .map(|r| format!(
                    "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                    r["name"].as_str().unwrap_or(""),
                    r["description"].as_str().unwrap_or("").replace('"', "\\\""),
                    r["input_schema"],
                ))
                .collect::<Vec<_>>()
                .join("\n");

            let output = format!("<functions>\n{schema_block}\n</functions>");
            return Ok(ToolResult {
                content: output,
                is_error: false,
                metadata: Some(json!({ "matched_tools": results })),
            });
        }

        // ── Keyword search ─────────────────────────────────────────────
        let keywords: Vec<String> = query.split_whitespace()
            .map(|s| s.to_lowercase())
            .collect();

        let mut scored: Vec<(usize, &ToolInfo)> = catalog.iter()
            .map(|tool| {
                let name_lower = tool.name.to_lowercase();
                let desc_lower = tool.description.to_lowercase();
                let mut score = 0usize;
                for kw in &keywords {
                    if name_lower.contains(kw.as_str()) { score += 3; }
                    if desc_lower.contains(kw.as_str()) { score += 1; }
                }
                (score, tool)
            })
            .filter(|(score, _)| *score > 0)
            .collect();

        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.truncate(max_results);

        if scored.is_empty() {
            let all_names: Vec<&str> = catalog.iter()
                .filter(|t| !t.is_core)
                .map(|t| t.name.as_str())
                .collect();
            return Ok(ToolResult::ok(format!(
                "No tools matched \"{query}\". Available deferred tools: {}",
                all_names.join(", ")
            )));
        }

        let _ = output_tx.send(ToolOutput {
            text: format!("Found {} tool(s) matching \"{}\"", scored.len(), query),
            is_error: false,
        }).await;

        let results: Vec<Value> = scored.iter().map(|(score, info)| {
            json!({
                "name": info.name,
                "description": info.description,
                "input_schema": info.input_schema,
                "relevance": score,
                "status": if info.is_core { "already_loaded" } else { "loaded" },
            })
        }).collect();

        let schema_block = results.iter()
            .map(|r| format!(
                "<function>{{\"name\": \"{}\", \"description\": \"{}\", \"parameters\": {}}}</function>",
                r["name"].as_str().unwrap_or(""),
                r["description"].as_str().unwrap_or("").replace('"', "\\\""),
                r["input_schema"],
            ))
            .collect::<Vec<_>>()
            .join("\n");

        let output = format!("<functions>\n{schema_block}\n</functions>");

        Ok(ToolResult {
            content: output,
            is_error: false,
            metadata: Some(json!({ "matched_tools": results })),
        })
    }
}
