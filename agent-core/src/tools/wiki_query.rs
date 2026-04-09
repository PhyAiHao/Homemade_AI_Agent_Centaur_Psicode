//! WikiQuery tool — answer questions from the wiki, optionally saving as a new page.
//!
//! Searches wiki pages, synthesizes an answer via LLM, and can file the answer
//! back as a new wiki page so that explorations compound.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct WikiQueryTool;

#[async_trait]
impl Tool for WikiQueryTool {
    fn name(&self) -> &'static str { "WikiQuery" }

    fn description(&self) -> &str {
        "Ask a question against the wiki knowledge base. The LLM searches for \
         relevant pages, synthesizes an answer with [[slug]] citations, and \
         optionally saves the answer as a new wiki page. Use this for complex \
         questions that require synthesizing multiple wiki pages."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to answer using the wiki"
                },
                "save_as_page": {
                    "type": "boolean",
                    "description": "If true, save the answer as a new wiki page (default: false)"
                },
                "page_title": {
                    "type": "string",
                    "description": "Title for the saved page (required if save_as_page is true)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Tags for the saved page"
                }
            },
            "required": ["question"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let question = input.get("question").and_then(|v| v.as_str()).unwrap_or("");
        let save_as_page = input.get("save_as_page").and_then(|v| v.as_bool()).unwrap_or(false);
        let page_title = input.get("page_title").and_then(|v| v.as_str()).unwrap_or("");
        let tags: Vec<String> = input.get("tags")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        if question.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: question"));
        }

        let _ = output_tx.send(ToolOutput {
            text: format!("Querying wiki: \"{question}\"..."),
            is_error: false,
        }).await;

        let socket = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
        let mut ipc = match crate::ipc::IpcClient::connect_to(&socket).await {
            Ok(c) => c,
            Err(e) => return Ok(ToolResult::error(format!("IPC connection failed: {e}"))),
        };

        let mut payload = HashMap::new();
        payload.insert("question".to_string(), Value::String(question.to_string()));
        payload.insert("save_as_page".to_string(), Value::Bool(save_as_page));
        payload.insert("page_title".to_string(), Value::String(page_title.to_string()));
        payload.insert("tags".to_string(), json!(tags));

        let request = crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
            request_id: crate::ipc::IpcClient::new_request_id(),
            action: "wiki_query".to_string(),
            payload,
        });

        let timeout = std::time::Duration::from_secs(120);
        match ipc.request_with_timeout(request, timeout).await {
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) if resp.ok => {
                let answer = resp.payload.get("answer")
                    .and_then(|v| v.as_str()).unwrap_or("[No answer]");
                let page_slug = resp.payload.get("page_slug")
                    .and_then(|v| v.as_str());

                if let Some(slug) = page_slug {
                    let _ = output_tx.send(ToolOutput {
                        text: format!("Answer saved as wiki page: [[{slug}]]"),
                        is_error: false,
                    }).await;
                }

                Ok(ToolResult::ok(answer))
            }
            Ok(crate::ipc::IpcMessage::MemoryResponse(resp)) => {
                let err = resp.error.unwrap_or_else(|| "unknown error".to_string());
                Ok(ToolResult::error(format!("Wiki query failed: {err}")))
            }
            Ok(_) => Ok(ToolResult::error("Unexpected response from wiki service")),
            Err(e) => Ok(ToolResult::error(format!("Wiki query IPC error: {e}"))),
        }
    }
}
