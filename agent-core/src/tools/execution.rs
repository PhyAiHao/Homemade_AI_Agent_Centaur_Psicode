//! Tool execution engine — coordinates running a single tool call.
//!
//! Mirrors `src/services/tools/toolExecution.ts`.
#![allow(dead_code)]

use anyhow::Result;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::{ToolOutput, ToolResult, ToolRegistry};
use crate::permissions::gate::{GateDecision, PermissionGate};

/// Execute a single tool call with permission checking, hooks, and streaming output.
pub async fn execute_tool(
    registry: &ToolRegistry,
    gate: &PermissionGate,
    tool_name: &str,
    tool_input: Value,
    _tool_use_id: &str,
    pre_hooks: &[super::hooks::HookDef],
    post_hooks: &[super::hooks::HookDef],
) -> Result<ToolResult> {
    let tool = registry.get(tool_name)
        .ok_or_else(|| anyhow::anyhow!("Unknown tool: {tool_name}"))?;

    // ─── Pre-hooks ──────────────────────────────────────────────────────
    for hook in pre_hooks.iter().filter(|h| h.matches(tool_name)) {
        info!(tool = tool_name, hook = %hook.command, "Running pre-hook");
        let hook_result = run_hook(&hook.command).await;
        if let Err(e) = hook_result {
            if hook.blocking {
                return Ok(ToolResult::error(format!(
                    "Pre-hook blocked execution: {e}"
                )));
            }
        }
    }

    // ─── Permission check ───────────────────────────────────────────────
    let input_json = tool_input.to_string();
    if tool.requires_permission() {
        match gate.check(tool_name, &input_json) {
            GateDecision::Allow   => {}
            GateDecision::Deny(r) => {
                return Ok(ToolResult::error(format!("Permission denied: {r}")));
            }
            GateDecision::PlanOnly => {
                return Ok(ToolResult::ok(format!(
                    "[Plan mode] Would execute {tool_name} with: {input_json}"
                )));
            }
        }
    }

    // ─── Execute ────────────────────────────────────────────────────────
    let (tx, mut rx) = mpsc::channel::<ToolOutput>(64);

    // Spawn a task to collect streaming output (for logging / TUI forwarding)
    let collect_handle = tokio::spawn(async move {
        let mut collected = Vec::new();
        while let Some(output) = rx.recv().await {
            collected.push(output);
        }
        collected
    });

    let result = tool.execute(tool_input, tx).await;

    // Wait for output collection to finish
    let _streaming_output = collect_handle.await.unwrap_or_default();

    // ─── Post-hooks ─────────────────────────────────────────────────────
    for hook in post_hooks.iter().filter(|h| h.matches(tool_name)) {
        info!(tool = tool_name, hook = %hook.command, "Running post-hook");
        if let Err(e) = run_hook(&hook.command).await {
            warn!(tool = tool_name, error = %e, "Post-hook failed");
        }
    }

    result
}

/// Run a hook command as a subprocess.
async fn run_hook(command: &str) -> Result<()> {
    let output = tokio::process::Command::new("sh")
        .args(["-c", command])
        .output()
        .await?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(anyhow::anyhow!("Hook failed (exit {}): {stderr}",
            output.status.code().unwrap_or(-1)))
    }
}
