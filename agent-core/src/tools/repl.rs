//! ReplTool — JavaScript/Python runtime evaluation tool.
//!
//! Mirrors the REPL primitive tool concept. In the Rust rewrite, this
//! delegates to an embedded scripting runtime or to the Python IPC layer.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct ReplTool;

#[async_trait]
impl Tool for ReplTool {
    fn name(&self) -> &'static str { "Repl" }

    fn description(&self) -> &str {
        "Evaluate code in an interactive runtime (JavaScript or Python). \
         Useful for quick calculations, data transformations, or testing snippets."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "enum": ["javascript", "python"],
                    "description": "Language runtime to use"
                },
                "code": {
                    "type": "string",
                    "description": "Code to evaluate"
                }
            },
            "required": ["code"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let language = input.get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("python");
        let code = input.get("code")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if code.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: code"));
        }

        let _ = output_tx.send(ToolOutput {
            text: format!("Evaluating {language} code..."),
            is_error: false,
        }).await;

        // Delegate to a subprocess for the appropriate runtime
        let (cmd, args) = match language {
            "javascript" | "js" => ("node", vec!["-e", code]),
            _ => ("python3", vec!["-c", code]),
        };

        let output = tokio::process::Command::new(cmd)
            .args(&args)
            .output()
            .await;

        match output {
            Ok(out) => {
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();
                let combined = if stderr.is_empty() {
                    stdout.clone()
                } else {
                    format!("{stdout}\n[stderr]\n{stderr}")
                };

                let _ = output_tx.send(ToolOutput {
                    text: combined.clone(),
                    is_error: !out.status.success(),
                }).await;

                if out.status.success() {
                    Ok(ToolResult::ok(combined))
                } else {
                    Ok(ToolResult::error(combined))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to run {cmd}: {e}"))),
        }
    }
}
