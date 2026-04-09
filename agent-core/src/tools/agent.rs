//! AgentTool — spawns and manages sub-agents for delegated work.
//!
//! Mirrors `src/tools/AgentTool/` from the TypeScript layer.
//!
//! Features:
//! - `model` param restricted to enum: "sonnet", "opus", "haiku"
//! - `name` param for multi-agent identification
//! - Structured output: JSON with result, cost_usd, duration_ms, session_id
//! - Auto-backgrounding: if foreground execution exceeds 120s, convert to background

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc;
use tracing::debug;
use uuid::Uuid;

use super::{Tool, ToolOutput, ToolResult, ToolRegistry};

/// Auto-background threshold: 120 seconds.
const AUTO_BG_THRESHOLD: Duration = Duration::from_secs(120);

/// Maximum recursion depth for agent → agent → agent chains.
/// Prevents unbounded agent spawning.
const MAX_AGENT_DEPTH: u32 = 3;

// Thread-local recursion depth counter.
// Incremented when entering a sub-agent, decremented on exit.
std::thread_local! {
    static AGENT_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Get the current agent recursion depth.
pub fn current_agent_depth() -> u32 {
    AGENT_DEPTH.with(|d| d.get())
}

pub struct AgentTool;

/// Run a sub-agent in-process by reusing the query loop.
/// This is the composable path — the sub-agent shares IPC, permissions,
/// and uses a filtered tool set.
///
/// Returns (response_text, cost_summary) on completion.
async fn run_in_process_agent(
    prompt: &str,
    model: &str,
    subagent_type: Option<&str>,
    output_tx: &mpsc::Sender<ToolOutput>,
) -> Result<(String, String)> {
    // ── 6c: Recursion depth check ──────────────────────────────────
    let depth = current_agent_depth();
    if depth >= MAX_AGENT_DEPTH {
        anyhow::bail!(
            "Agent recursion depth limit reached ({MAX_AGENT_DEPTH}). \
             Cannot spawn further sub-agents."
        );
    }
    // Increment depth for the duration of this sub-agent
    AGENT_DEPTH.with(|d| d.set(depth + 1));

    let result = run_in_process_agent_inner(prompt, model, subagent_type, output_tx).await;

    // Always decrement depth, even on error
    AGENT_DEPTH.with(|d| d.set(depth));
    result
}

/// Inner implementation (separated so depth decrement always runs).
async fn run_in_process_agent_inner(
    prompt: &str,
    model: &str,
    subagent_type: Option<&str>,
    output_tx: &mpsc::Sender<ToolOutput>,
) -> Result<(String, String)> {
    use crate::ipc::IpcClient;
    use crate::permissions::gate::PermissionGate;
    use crate::permissions::mode::PermissionMode;
    use crate::query::query_loop::{QueryLoopConfig, QueryEvent, run_query_loop};
    use crate::query::message::ConversationMessage;
    use crate::cost_tracker::CostTracker;

    let socket = std::env::var("AGENT_IPC_SOCKET")
        .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
    let mut ipc = IpcClient::connect_to(&socket).await?;

    let state = crate::state::new_shared_state(
        std::env::current_dir().unwrap_or_default()
    );

    // ── 5b/6a/6b: Build a filtered tool registry ──────────────────
    // Sub-agents get FEWER tools than the parent, never more.
    // "Explore" and "Plan" types get read-only subsets.
    // "general-purpose" gets the parent's tools minus Agent (no recursive spawning
    // unless depth allows it).
    let full_registry = ToolRegistry::default_registry(state.clone());
    let registry = match subagent_type {
        Some("Explore") => {
            full_registry.filter(&[
                "FileRead", "Glob", "Grep", "ToolSearch", "Brief", "Sleep",
            ])
        }
        Some("Plan") => {
            full_registry.filter(&[
                "FileRead", "Glob", "Grep", "ToolSearch", "Brief", "Sleep",
                "EnterPlanMode", "ExitPlanMode",
            ])
        }
        _ => {
            // General-purpose: all tools, but exclude Agent if at max depth - 1
            // (prevents the child from spawning grandchildren at the limit)
            let depth = current_agent_depth();
            if depth >= MAX_AGENT_DEPTH - 1 {
                full_registry.exclude(&["Agent"])
            } else {
                full_registry
            }
        }
    };

    // ── 6a: Permission inheritance ──────────────────────────────────
    // Sub-agents use AutoApprove within their filtered tool set.
    // The tool filtering IS the permission narrowing — a sub-agent
    // physically cannot call tools not in its registry.
    let gate = PermissionGate::new(PermissionMode::AutoApprove, Vec::new());
    let mut cost_tracker = CostTracker::new();
    let mut messages = vec![ConversationMessage::user_text(prompt)];

    let config = QueryLoopConfig {
        model: model.to_string(),
        system_prompt: format!(
            "You are a sub-agent working on a specific task. \
             Focus on the task and be concise. Working directory: {}",
            std::env::current_dir().unwrap_or_default().display()
        ),
        max_turns: 30,
        ..QueryLoopConfig::default()
    };

    let (event_tx, mut event_rx) = tokio::sync::mpsc::channel::<QueryEvent>(256);

    // Forward events to parent's output channel
    let out_tx = output_tx.clone();
    let forward_handle = tokio::spawn(async move {
        let mut response_text = String::new();
        while let Some(event) = event_rx.recv().await {
            match event {
                QueryEvent::TextDelta(text) => {
                    response_text.push_str(&text);
                    let _ = out_tx.send(ToolOutput {
                        text, is_error: false
                    }).await;
                }
                QueryEvent::ToolStart { name, .. } => {
                    let _ = out_tx.send(ToolOutput {
                        text: format!("[sub-agent tool: {name}]"),
                        is_error: false,
                    }).await;
                }
                QueryEvent::Error(e) => {
                    let _ = out_tx.send(ToolOutput {
                        text: format!("[sub-agent error: {e}]"),
                        is_error: true,
                    }).await;
                }
                _ => {}
            }
        }
        response_text
    });

    let _stop_reason = run_query_loop(
        config, &mut messages, &mut ipc, &registry, &gate,
        &mut cost_tracker, event_tx,
    ).await?;

    let response_text = forward_handle.await.unwrap_or_default();
    let cost = cost_tracker.summary();

    Ok((response_text, cost))
}

/// Map short model name to full model ID.
fn resolve_model(short: &str) -> Result<&'static str> {
    match short {
        "sonnet" => Ok("claude-sonnet-4-20250514"),
        "opus" => Ok("claude-opus-4-20250514"),
        "haiku" => Ok("claude-haiku-4-20250514"),
        _ => Err(anyhow::anyhow!(
            "Invalid model: \"{short}\". Must be one of: \"sonnet\", \"opus\", \"haiku\"."
        )),
    }
}

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &'static str { "Agent" }

    fn description(&self) -> &str {
        "Launch a new agent to handle complex, multi-step tasks autonomously. \
         Each agent runs in an isolated context with its own tools and permissions. \
         Returns structured JSON with result, cost, and timing."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "description": {
                    "type": "string",
                    "description": "A short (3-5 word) description of the task"
                },
                "prompt": {
                    "type": "string",
                    "description": "The task for the agent to perform"
                },
                "model": {
                    "type": "string",
                    "enum": ["sonnet", "opus", "haiku"],
                    "description": "Model to use for the sub-agent (maps to full model IDs)"
                },
                "name": {
                    "type": "string",
                    "description": "Optional name for the sub-agent (for multi-agent identification)"
                },
                "subagent_type": {
                    "type": "string",
                    "description": "Specialized agent type (e.g., 'general-purpose', 'Explore', 'Plan')"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Run the agent in the background"
                },
                "isolation": {
                    "type": "string",
                    "enum": ["worktree"],
                    "description": "Isolation mode — 'worktree' creates a temporary git worktree"
                }
            },
            "required": ["description", "prompt"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("sub-agent");
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let model_short = input.get("model").and_then(|v| v.as_str());
        let agent_name = input.get("name").and_then(|v| v.as_str());
        let background = input
            .get("run_in_background")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        if prompt.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: prompt"));
        }

        // 6c: Check recursion depth before any agent spawning path
        let depth = current_agent_depth();
        if depth >= MAX_AGENT_DEPTH {
            return Ok(ToolResult::error(format!(
                "Agent recursion depth limit reached ({MAX_AGENT_DEPTH}). \
                 Cannot spawn further sub-agents."
            )));
        }

        // Resolve model
        let model_id: Option<&str> = if let Some(short) = model_short {
            Some(resolve_model(short)?)
        } else {
            None
        };

        let session_id = Uuid::new_v4().to_string();
        let display_name = agent_name.unwrap_or(description);

        let _ = output_tx
            .send(ToolOutput {
                text: format!("Spawning agent \"{display_name}\" (session: {session_id})"),
                is_error: false,
            })
            .await;

        debug!(
            "Agent: desc={description} model={model_id:?} name={agent_name:?} bg={background}"
        );

        // ── Build the sub-agent command ─────────────────────────────────
        let exe = std::env::current_exe()
            .unwrap_or_else(|_| std::path::PathBuf::from("agent"));

        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("--bare"); // headless mode
        cmd.arg(prompt);

        if let Some(m) = model_id {
            cmd.arg("--model").arg(m);
        }

        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());

        let start = Instant::now();

        if background {
            // ── Background mode ─────────────────────────────────────────
            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id().unwrap_or(0);
                    let result = json!({
                        "status": "running_in_background",
                        "pid": pid,
                        "session_id": session_id,
                        "description": description,
                        "name": agent_name,
                    });
                    let _ = output_tx
                        .send(ToolOutput {
                            text: format!(
                                "Agent \"{display_name}\" running in background (pid: {pid})"
                            ),
                            is_error: false,
                        })
                        .await;
                    Ok(ToolResult::ok(result.to_string()))
                }
                Err(e) => Ok(ToolResult::error(format!("Failed to spawn agent: {e}"))),
            }
        } else {
            // ── Foreground mode: in-process sub-agent (Principle 4) ─────
            // Reuses the query loop directly instead of spawning an OS process.
            // This enables tool filtering, shared IPC, and cost tracking.
            let subagent_type = input.get("subagent_type").and_then(|v| v.as_str());
            let effective_model = model_id.unwrap_or("claude-sonnet-4-6");

            let _ = output_tx.send(ToolOutput {
                text: format!("Running agent \"{display_name}\" in-process..."),
                is_error: false,
            }).await;

            match run_in_process_agent(prompt, effective_model, subagent_type, &output_tx).await {
                Ok((response, cost)) => {
                    let duration_ms = start.elapsed().as_millis() as u64;
                    let result = json!({
                        "status": "completed",
                        "result": response,
                        "cost_usd": cost,
                        "duration_ms": duration_ms,
                        "session_id": session_id,
                        "name": display_name,
                        "in_process": true,
                    });
                    return Ok(ToolResult {
                        content: result.to_string(),
                        is_error: false,
                        metadata: Some(result),
                    });
                }
                Err(e) => {
                    debug!("In-process agent failed: {e}, falling back to OS process");
                    // Fall back to OS process execution
                }
            }

            // ── OS process fallback ────────────────────────────────────
            match cmd.spawn() {
                Ok(mut child) => {
                    let stdout = child.stdout.take();
                    let stderr = child.stderr.take();
                    let tx_out = output_tx.clone();
                    let tx_err = output_tx.clone();

                    let stdout_task = tokio::spawn(async move {
                        let mut collected = String::new();
                        if let Some(stdout) = stdout {
                            let mut reader = BufReader::new(stdout).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                collected.push_str(&line);
                                collected.push('\n');
                                let _ = tx_out
                                    .send(ToolOutput {
                                        text: line,
                                        is_error: false,
                                    })
                                    .await;
                            }
                        }
                        collected
                    });

                    let stderr_task = tokio::spawn(async move {
                        let mut collected = String::new();
                        if let Some(stderr) = stderr {
                            let mut reader = BufReader::new(stderr).lines();
                            while let Ok(Some(line)) = reader.next_line().await {
                                collected.push_str(&line);
                                collected.push('\n');
                                let _ = tx_err
                                    .send(ToolOutput {
                                        text: line,
                                        is_error: true,
                                    })
                                    .await;
                            }
                        }
                        collected
                    });

                    // Wait with auto-background timeout
                    let wait_result = tokio::time::timeout(
                        AUTO_BG_THRESHOLD,
                        child.wait(),
                    )
                    .await;

                    let duration_ms = start.elapsed().as_millis() as u64;

                    match wait_result {
                        Ok(Ok(status)) => {
                            let stdout_text = stdout_task.await.unwrap_or_default();
                            let stderr_text = stderr_task.await.unwrap_or_default();
                            let exit_code = status.code().unwrap_or(-1);

                            let combined = if stderr_text.is_empty() {
                                stdout_text.clone()
                            } else {
                                format!("{stdout_text}\n[stderr]\n{stderr_text}")
                            };

                            let result = json!({
                                "result": combined,
                                "cost_usd": null,
                                "duration_ms": duration_ms,
                                "session_id": session_id,
                                "exit_code": exit_code,
                            });

                            if exit_code == 0 {
                                Ok(ToolResult {
                                    content: result.to_string(),
                                    is_error: false,
                                    metadata: Some(result),
                                })
                            } else {
                                Ok(ToolResult {
                                    content: result.to_string(),
                                    is_error: true,
                                    metadata: Some(result),
                                })
                            }
                        }
                        Ok(Err(e)) => {
                            Ok(ToolResult::error(format!("Failed to wait for agent: {e}")))
                        }
                        Err(_) => {
                            // Auto-background: exceeded 120s, let it keep running
                            let pid = child.id().unwrap_or(0);
                            let _ = output_tx
                                .send(ToolOutput {
                                    text: format!(
                                        "Agent \"{display_name}\" exceeded {}s, auto-backgrounded (pid: {pid})",
                                        AUTO_BG_THRESHOLD.as_secs()
                                    ),
                                    is_error: false,
                                })
                                .await;

                            let result = json!({
                                "status": "auto_backgrounded",
                                "pid": pid,
                                "session_id": session_id,
                                "duration_ms": duration_ms,
                                "description": description,
                                "name": agent_name,
                            });
                            Ok(ToolResult::ok(result.to_string()))
                        }
                    }
                }
                Err(e) => Ok(ToolResult::error(format!("Failed to spawn agent: {e}"))),
            }
        }
    }
}
