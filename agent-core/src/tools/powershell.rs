//! PowerShellTool — executes PowerShell commands (Windows/cross-platform).
//!
//! Mirrors `src/tools/PowerShellTool/`. On non-Windows platforms, falls back
//! to `pwsh` if available.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};

pub struct PowerShellTool;

impl PowerShellTool {
    fn powershell_binary() -> &'static str {
        if cfg!(target_os = "windows") {
            "powershell.exe"
        } else {
            "pwsh" // PowerShell Core on macOS/Linux
        }
    }
}

#[async_trait]
impl Tool for PowerShellTool {
    fn name(&self) -> &'static str { "PowerShell" }

    fn description(&self) -> &str {
        "Execute PowerShell commands. Primarily for Windows, but works on \
         macOS/Linux with PowerShell Core (pwsh) installed."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "PowerShell command or script to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory"
                },
                "timeout": {
                    "type": "number",
                    "description": "Timeout in milliseconds (default: 120000)"
                },
                "description": {
                    "type": "string",
                    "description": "Semantic description of what the command does"
                }
            },
            "required": ["command"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let command = input.get("command")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let cwd = input.get("cwd")
            .and_then(|v| v.as_str());
        let timeout_ms = input.get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(120_000);

        if command.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: command"));
        }

        let ps = Self::powershell_binary();
        let _ = output_tx.send(ToolOutput {
            text: format!("$ {ps} -Command \"{command}\""),
            is_error: false,
        }).await;

        let mut cmd = tokio::process::Command::new(ps);
        cmd.args(["-NoProfile", "-NonInteractive", "-Command", command]);

        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            cmd.output(),
        ).await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let text = if stderr.is_empty() {
                    format!("{stdout}\n[exit code: {exit_code}]")
                } else {
                    format!("{stdout}\n[stderr]\n{stderr}\n[exit code: {exit_code}]")
                };

                let _ = output_tx.send(ToolOutput {
                    text: text.clone(),
                    is_error: !output.status.success(),
                }).await;

                if output.status.success() {
                    Ok(ToolResult::ok(text))
                } else {
                    Ok(ToolResult::error(text))
                }
            }
            Ok(Err(e)) => Ok(ToolResult::error(format!("Failed to run {ps}: {e}"))),
            Err(_) => Ok(ToolResult::error(format!(
                "Command timed out after {timeout_ms}ms"
            ))),
        }
    }
}
