//! Bash tool — subprocess execution with streaming output, background mode,
//! structured output, and security checks.
//!
//! Mirrors `src/tools/BashTool/` (16 files) from the TypeScript layer.
//!
//! Features:
//! - `run_in_background` param: spawn command in background, return task ID
//! - `dangerouslyDisableSandbox` param: bypass sandbox when true
//! - Output truncation: if output exceeds 100 KB, truncate
//! - Timeout in milliseconds (default: 120_000 ms = 2 minutes)
//! - Structured JSON output: stdout, stderr, exit_code
//! - Exit code interpretation in error messages

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::sync::mpsc::Sender;
use tokio::time::timeout;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};
use crate::bash_parser::security::{is_destructive, parse_for_security, has_dangerous_substitution, has_dangerous_chars};
use crate::bash_parser::ast::SecurityResult;
use crate::bash_parser::read_only::is_read_only_command;

/// Default timeout: 120 000 ms (2 minutes).
const DEFAULT_TIMEOUT_MS: u64 = 120_000;

/// Maximum output size in bytes before truncation (fallback if env not set).
const MAX_OUTPUT_BYTES: usize = 100 * 1024; // 100 KB

/// Global counter for background task IDs.
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

pub struct BashTool {
    sandbox: bool,
}

impl BashTool {
    pub fn new() -> Self {
        let sandbox = std::env::var("SANDBOX_MODE")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        BashTool { sandbox }
    }
}

/// Interpret common exit codes into human-readable messages.
fn interpret_exit_code(code: i32) -> &'static str {
    match code {
        0 => "success",
        1 => "general error",
        2 => "misuse of shell builtin",
        126 => "command not executable",
        127 => "command not found",
        128 => "invalid exit argument",
        130 => "terminated by Ctrl+C (SIGINT)",
        137 => "killed (SIGKILL, likely OOM)",
        139 => "segmentation fault (SIGSEGV)",
        141 => "broken pipe (SIGPIPE)",
        143 => "terminated (SIGTERM)",
        _ if code > 128 => "terminated by signal",
        _ => "non-zero exit",
    }
}

/// Truncate output if it exceeds the size limit.
fn maybe_truncate(output: &str) -> String {
    let bytes = output.len();
    if bytes > MAX_OUTPUT_BYTES {
        let truncated = &output[..MAX_OUTPUT_BYTES];
        // Find last newline to avoid cutting mid-line
        let cut = truncated.rfind('\n').unwrap_or(MAX_OUTPUT_BYTES);
        format!(
            "{}\n\n... [truncated, {} bytes total]",
            &output[..cut],
            bytes
        )
    } else {
        output.to_string()
    }
}

#[async_trait]
impl Tool for BashTool {
    fn name(&self) -> &'static str { "Bash" }

    fn description(&self) -> &str {
        "Execute a bash shell command. Returns structured JSON with stdout, stderr, \
         and exit_code. Supports background execution and output truncation. \
         Destructive commands (rm -rf, dd, etc.) are blocked by default."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The bash command to execute"
                },
                "timeout": {
                    "type": "integer",
                    "description": "Timeout in milliseconds (default: 120000)"
                },
                "description": {
                    "type": "string",
                    "description": "Short description of what this command does"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "Spawn command in background and return immediately with a task ID"
                },
                "dangerouslyDisableSandbox": {
                    "type": "boolean",
                    "description": "Override sandbox mode for this command (use with caution)"
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, input: Value, output_tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let command = input["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("command is required"))?;
        let timeout_ms = input["timeout"].as_u64().unwrap_or(DEFAULT_TIMEOUT_MS);
        let run_bg = input["run_in_background"].as_bool().unwrap_or(false);
        let disable_sandbox = input["dangerouslyDisableSandbox"]
            .as_bool()
            .unwrap_or(false);

        debug!("Bash: {command} (timeout={timeout_ms}ms, bg={run_bg})");

        // ── Security check (full pipeline) ──────────────────────────────
        // 1. Check for dangerous characters (control chars, unicode whitespace)
        if has_dangerous_chars(command) {
            let msg = "Command blocked: contains dangerous control characters or unicode whitespace".to_string();
            let _ = output_tx.send(ToolOutput { text: msg.clone(), is_error: true }).await;
            return Ok(ToolResult::error(msg));
        }

        // 2. Check for dangerous substitutions ($(), ``, <(), etc.)
        if has_dangerous_substitution(command) {
            debug!("Command has dangerous substitution patterns — will require permission");
            // Don't block — just flag for permission system. The tool registry's
            // permission gate handles the actual allow/deny decision.
        }

        // 3. Parse for security: extract simple commands with trustworthy argv
        let security = parse_for_security(command);
        match &security {
            SecurityResult::Simple(cmds) => {
                // Check each extracted command for destructive operations
                for cmd in cmds {
                    // Check if command is destructive
                    let full_cmd = cmd.argv().join(" ");
                    if let Some(reason) = is_destructive(&full_cmd) {
                        let msg = format!(
                            "Command blocked (destructive: {reason:?}): {}", cmd.program
                        );
                        let _ = output_tx.send(ToolOutput { text: msg.clone(), is_error: true }).await;
                        return Ok(ToolResult::error(msg));
                    }
                }
                // If all commands are read-only, note it (could auto-approve via permission system)
                let all_read_only = cmds.iter().all(is_read_only_command);
                if all_read_only {
                    debug!("All commands are read-only — auto-approvable");
                }
            }
            SecurityResult::TooComplex(reason) => {
                debug!("Command too complex for security analysis: {reason}");
                // Fall through — permission system will prompt user
            }
            SecurityResult::ParseUnavailable(reason) => {
                debug!("Parse unavailable for security: {reason}");
                // Fall back to simple destructive check
                if let Some(reason) = is_destructive(command) {
                    let msg = format!("Command blocked (destructive: {reason:?}): {command}");
                    let _ = output_tx.send(ToolOutput { text: msg.clone(), is_error: true }).await;
                    return Ok(ToolResult::error(msg));
                }
            }
        }

        // ── Sandbox check ───────────────────────────────────────────────
        let effective_sandbox = self.sandbox && !disable_sandbox;
        if effective_sandbox && !is_read_only(command) {
            let msg = format!(
                "Command blocked in sandbox mode (write operation): {command}. \
                 Use dangerouslyDisableSandbox: true to override."
            );
            let _ = output_tx
                .send(ToolOutput { text: msg.clone(), is_error: true })
                .await;
            return Ok(ToolResult::error(msg));
        }

        // ── Background mode ─────────────────────────────────────────────
        if run_bg {
            let task_id = NEXT_TASK_ID.fetch_add(1, Ordering::SeqCst);
            let provider = crate::shell::BashProvider::default_posix();
            let exec_args = <crate::shell::BashProvider as crate::shell::ShellProvider>::build_exec_command(
                &provider, command, Some(&std::env::current_dir().unwrap_or_default()),
            );
            let mut cmd = tokio::process::Command::new(&exec_args[0]);
            cmd.args(&exec_args[1..]);
            for (k, v) in <crate::shell::BashProvider as crate::shell::ShellProvider>::get_env_overrides(&provider) {
                cmd.env(k, v);
            }
            cmd.stdout(Stdio::null());
            cmd.stderr(Stdio::null());

            match cmd.spawn() {
                Ok(child) => {
                    let pid = child.id().unwrap_or(0);
                    let result = json!({
                        "task_id": task_id,
                        "pid": pid,
                        "status": "running_in_background",
                        "command": command,
                    });
                    let msg = format!(
                        "Command running in background (task_id: {task_id}, pid: {pid})"
                    );
                    let _ = output_tx
                        .send(ToolOutput { text: msg, is_error: false })
                        .await;
                    return Ok(ToolResult::ok(result.to_string()));
                }
                Err(e) => {
                    return Ok(ToolResult::error(format!(
                        "Failed to spawn background command: {e}"
                    )));
                }
            }
        }

        // ── Foreground execution with timeout ───────────────────────────
        let timeout_dur = Duration::from_millis(timeout_ms);

        let result = timeout(
            timeout_dur,
            run_command(command, output_tx.clone()),
        )
        .await;

        match result {
            Ok(Ok((stdout, stderr, exit_code))) => {
                let truncated_stdout = maybe_truncate(&stdout);
                let truncated_stderr = maybe_truncate(&stderr);

                let structured = json!({
                    "stdout": truncated_stdout,
                    "stderr": truncated_stderr,
                    "exit_code": exit_code,
                });

                if exit_code != 0 {
                    let interpretation = interpret_exit_code(exit_code);
                    let msg = format!(
                        "Command exited with code {exit_code} ({interpretation})"
                    );
                    let _ = output_tx
                        .send(ToolOutput { text: msg, is_error: true })
                        .await;
                }

                Ok(ToolResult {
                    content: structured.to_string(),
                    is_error: exit_code != 0,
                    metadata: Some(structured),
                })
            }
            Ok(Err(e)) => Ok(ToolResult::error(format!("Command error: {e}"))),
            Err(_) => {
                let msg = format!(
                    "Command timed out after {}ms: {command}",
                    timeout_ms
                );
                let _ = output_tx
                    .send(ToolOutput { text: msg.clone(), is_error: true })
                    .await;
                Ok(ToolResult::error(msg))
            }
        }
    }
}

/// Run a command and return (stdout, stderr, exit_code).
async fn run_command(
    command: &str,
    output_tx: Sender<ToolOutput>,
) -> Result<(String, String, i32)> {
    let provider = crate::shell::BashProvider::default_posix();
    let exec_args = <crate::shell::BashProvider as crate::shell::ShellProvider>::build_exec_command(
        &provider, command, Some(&std::env::current_dir().unwrap_or_default()),
    );
    let mut cmd = tokio::process::Command::new(&exec_args[0]);
    cmd.args(&exec_args[1..]);
    for (k, v) in <crate::shell::BashProvider as crate::shell::ShellProvider>::get_env_overrides(&provider) {
        cmd.env(k, v);
    }
    let mut child = cmd
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take().expect("stdout pipe");
    let stderr = child.stderr.take().expect("stderr pipe");

    let tx_stdout = output_tx.clone();
    let tx_stderr = output_tx;

    // Stream stdout
    let stdout_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stdout).lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_stdout
                .send(ToolOutput { text: line.clone(), is_error: false })
                .await;
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    // Stream stderr
    let stderr_task = tokio::spawn(async move {
        let mut lines = BufReader::new(stderr).lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_stderr
                .send(ToolOutput { text: line.clone(), is_error: true })
                .await;
            buf.push_str(&line);
            buf.push('\n');
        }
        buf
    });

    let status = child.wait().await?;
    let stdout_out = stdout_task.await.unwrap_or_default();
    let stderr_out = stderr_task.await.unwrap_or_default();

    let exit_code = status.code().unwrap_or(-1);

    Ok((stdout_out, stderr_out, exit_code))
}

/// Rough heuristic: is this command likely read-only?
fn is_read_only(cmd: &str) -> bool {
    let read_only_prefixes = [
        "ls", "cat", "head", "tail", "grep", "find", "echo", "pwd",
        "whoami", "date", "which", "type", "file", "stat", "wc",
        "diff", "git log", "git status", "git diff", "git show",
        "cargo check", "cargo test", "python", "node --version",
    ];
    let cmd_trim = cmd.trim();
    read_only_prefixes.iter().any(|p| cmd_trim.starts_with(p))
}
