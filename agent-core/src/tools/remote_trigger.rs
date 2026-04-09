//! RemoteTriggerTool — manages scheduled remote agent triggers via REST API.
//!
//! Mirrors `src/tools/RemoteTriggerTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct RemoteTriggerTool;

const TRIGGERS_ENDPOINT: &str = "https://api.anthropic.com/v1/code/triggers";

#[async_trait]
impl Tool for RemoteTriggerTool {
    fn name(&self) -> &'static str { "RemoteTrigger" }

    fn description(&self) -> &str {
        "Manage scheduled remote agent triggers (list, get, create, update, run)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["list", "get", "create", "update", "run"],
                    "description": "API action to perform"
                },
                "trigger_id": {
                    "type": "string",
                    "description": "Required for get/update/run"
                },
                "body": {
                    "type": "object",
                    "description": "JSON body for create/update"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let action = input.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let trigger_id = input.get("trigger_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let body = input.get("body").cloned().unwrap_or(json!({}));

        // Read API key from environment
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .unwrap_or_default();

        if api_key.is_empty() {
            return Ok(ToolResult::error("ANTHROPIC_API_KEY not set. Run `agent login` first."));
        }

        let client = reqwest::Client::new();

        let (method, url, req_body) = match action {
            "list" => ("GET", TRIGGERS_ENDPOINT.to_string(), None),
            "get" => {
                if trigger_id.is_empty() {
                    return Ok(ToolResult::error("trigger_id required for 'get'"));
                }
                ("GET", format!("{TRIGGERS_ENDPOINT}/{trigger_id}"), None)
            }
            "create" => ("POST", TRIGGERS_ENDPOINT.to_string(), Some(body)),
            "update" => {
                if trigger_id.is_empty() {
                    return Ok(ToolResult::error("trigger_id required for 'update'"));
                }
                ("PATCH", format!("{TRIGGERS_ENDPOINT}/{trigger_id}"), Some(body))
            }
            "run" => {
                if trigger_id.is_empty() {
                    return Ok(ToolResult::error("trigger_id required for 'run'"));
                }
                ("POST", format!("{TRIGGERS_ENDPOINT}/{trigger_id}/run"), None)
            }
            _ => return Ok(ToolResult::error(format!("Invalid action: {action}"))),
        };

        let _ = output_tx.send(ToolOutput {
            text: format!("{method} {url}"),
            is_error: false,
        }).await;

        let mut request = match method {
            "GET" => client.get(&url),
            "POST" => client.post(&url),
            "PATCH" => client.patch(&url),
            _ => unreachable!(),
        };

        request = request
            .header("x-api-key", &api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json");

        if let Some(b) = req_body {
            request = request.json(&b);
        }

        match request.send().await {
            Ok(resp) => {
                let status = resp.status().as_u16();
                let body_text = resp.text().await.unwrap_or_default();

                let result = format!("HTTP {status}\n{body_text}");
                let _ = output_tx.send(ToolOutput {
                    text: result.clone(),
                    is_error: status >= 400,
                }).await;

                if status < 400 {
                    Ok(ToolResult::ok(result))
                } else {
                    Ok(ToolResult::error(result))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("HTTP request failed: {e}"))),
        }
    }
}
