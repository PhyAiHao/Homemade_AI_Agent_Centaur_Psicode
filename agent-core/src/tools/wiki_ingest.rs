//! WikiIngest tool — ingest a source (URL, file, or text) into the wiki.
//!
//! Orchestrates: fetch content -> send to Python WikiService -> LLM extracts pages
//! -> saves wiki pages with cross-references -> appends to wiki/log.md.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct WikiIngestTool;

#[async_trait]
impl Tool for WikiIngestTool {
    fn name(&self) -> &'static str { "WikiIngest" }

    fn description(&self) -> &str {
        "Ingest a source into the wiki knowledge base. Accepts a URL, file path, \
         or raw text. The LLM reads the source, creates summary and entity pages, \
         updates cross-references, and logs the ingestion. Use this to build up \
         the wiki from articles, documents, or any text content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "URL, file path, or raw text content to ingest"
                },
                "source_type": {
                    "type": "string",
                    "enum": ["url", "file", "text"],
                    "description": "Type of source"
                },
                "title": {
                    "type": "string",
                    "description": "Title for the source (used in summary page)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags to apply to created pages"
                }
            },
            "required": ["source", "source_type", "title"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let source = input.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let source_type = input.get("source_type").and_then(|v| v.as_str()).unwrap_or("text");
        let title = input.get("title").and_then(|v| v.as_str()).unwrap_or("Untitled");
        let tags: Vec<String> = input.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        if source.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: source"));
        }

        let _ = output_tx.send(ToolOutput {
            text: format!("Ingesting into wiki: \"{title}\" ({source_type})..."),
            is_error: false,
        }).await;

        // For URL sources, we could fetch content here via WebFetch,
        // but the Python side can also handle raw content. For now,
        // pass the source directly and let the caller pre-fetch if needed.
        let content = source.to_string();
        let source_url = if source_type == "url" { source.to_string() } else { String::new() };

        // Send to Python WikiService
        let socket = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
        let mut ipc = match crate::ipc::IpcClient::connect_to(&socket).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("IPC connection failed: {e}"))),
        };

        let mut payload = HashMap::new();
        payload.insert("content".to_string(), Value::String(content));
        payload.insert("title".to_string(), Value::String(title.to_string()));
        payload.insert("tags".to_string(), json!(tags));
        payload.insert("source_url".to_string(), Value::String(source_url));
        payload.insert("source_type".to_string(), Value::String(source_type.to_string()));

        let request = crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
            request_id: crate::ipc::IpcClient::new_request_id(),
            action: "wiki_ingest".to_string(),
            payload,
        });

        let timeout = std::time::Duration::from_secs(300);
        match ipc.request_with_timeout(request, timeout).await {
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) if resp.ok => {
                let created = resp.payload.get("pages_created")
                    .and_then(|v| v.as_u64()).unwrap_or(0);
                let updated = resp.payload.get("pages_updated")
                    .and_then(|v| v.as_u64()).unwrap_or(0);
                let summary = resp.payload.get("summary")
                    .and_then(|v| v.as_str()).unwrap_or("Ingestion complete");

                let _ = output_tx.send(ToolOutput {
                    text: format!("Wiki ingest: {created} pages created, {updated} updated"),
                    is_error: false,
                }).await;

                let result = json!({
                    "pages_created": created,
                    "pages_updated": updated,
                    "summary": summary,
                });
                Ok(ToolResult::ok(result.to_string()))
            }
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) => {
                let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
                Ok(ToolResult::error(format!("Wiki ingest failed: {err}")))
            }
            Ok(_) => Ok(ToolResult::error("Unexpected response from wiki service")),
            Err(e) => Ok(ToolResult::error(format!("Wiki ingest IPC error: {e}"))),
        }
    }
}
