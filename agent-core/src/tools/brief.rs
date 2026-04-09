//! BriefTool — sends a message to the user with optional file attachments.
//!
//! Mirrors `src/tools/BriefTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct BriefTool;

#[async_trait]
impl Tool for BriefTool {
    fn name(&self) -> &'static str { "Brief" }

    fn description(&self) -> &str {
        "Send a message to the user with optional file attachments. \
         Supports normal and proactive status modes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "message": {
                    "type": "string",
                    "description": "Message content (markdown supported)"
                },
                "attachments": {
                    "type": "array",
                    "description": "File paths to attach",
                    "items": { "type": "string" }
                },
                "status": {
                    "type": "string",
                    "enum": ["normal", "proactive"],
                    "description": "Message delivery mode"
                }
            },
            "required": ["message"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let message = input.get("message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let status = input.get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("normal");

        // Send the message to TUI
        let _ = output_tx.send(ToolOutput {
            text: message.to_string(),
            is_error: false,
        }).await;

        // Process attachments
        let mut attachment_info = Vec::new();
        if let Some(attachments) = input.get("attachments").and_then(|v| v.as_array()) {
            for att in attachments {
                if let Some(path_str) = att.as_str() {
                    let path = Path::new(path_str);
                    if path.exists() {
                        let meta = tokio::fs::metadata(path).await;
                        let size = meta.map(|m| m.len()).unwrap_or(0);
                        let is_image = matches!(
                            path.extension().and_then(|e| e.to_str()),
                            Some("png" | "jpg" | "jpeg" | "gif" | "svg" | "webp")
                        );
                        attachment_info.push(json!({
                            "path": path_str,
                            "size": size,
                            "is_image": is_image,
                        }));
                        let _ = output_tx.send(ToolOutput {
                            text: format!("📎 {path_str} ({size} bytes)"),
                            is_error: false,
                        }).await;
                    } else {
                        let _ = output_tx.send(ToolOutput {
                            text: format!("⚠ Attachment not found: {path_str}"),
                            is_error: true,
                        }).await;
                    }
                }
            }
        }

        Ok(ToolResult {
            content: format!("Briefed user ({status}): {message}"),
            is_error: false,
            metadata: if attachment_info.is_empty() {
                None
            } else {
                Some(json!({ "attachments": attachment_info }))
            },
        })
    }
}
