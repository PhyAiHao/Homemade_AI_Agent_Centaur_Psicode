//! Stop hooks — conditions that cause the query loop to stop.
//!
//! Mirrors `src/query/stopHooks.ts`.
#![allow(dead_code)]

use tracing::{info, warn};

/// Result of running stop hooks.
#[derive(Debug, Clone)]
pub struct StopHookResult {
    /// Whether any hook prevents continuation.
    pub prevent_continuation: bool,
    /// Errors from hooks that should be injected as context.
    pub blocking_errors: Vec<String>,
    /// Hook execution details for logging.
    pub details: Vec<HookDetail>,
}

#[derive(Debug, Clone)]
pub struct HookDetail {
    pub command: String,
    pub duration_ms: u64,
    pub success: bool,
    pub output: Option<String>,
}

/// A configured stop hook.
#[derive(Debug, Clone)]
pub struct StopHook {
    /// Shell command to execute.
    pub command: String,
    /// If true, failure prevents continuation.
    pub blocking: bool,
}

/// Execute all stop hooks and collect results.
pub async fn execute_stop_hooks(hooks: &[StopHook]) -> StopHookResult {
    let mut result = StopHookResult {
        prevent_continuation: false,
        blocking_errors: Vec::new(),
        details: Vec::new(),
    };

    for hook in hooks {
        let start = std::time::Instant::now();

        let output = tokio::process::Command::new("sh")
            .args(["-c", &hook.command])
            .output()
            .await;

        let duration_ms = start.elapsed().as_millis() as u64;

        match output {
            Ok(out) => {
                let success = out.status.success();
                let stdout = String::from_utf8_lossy(&out.stdout).to_string();
                let stderr = String::from_utf8_lossy(&out.stderr).to_string();

                let output_text = if stderr.is_empty() {
                    stdout.clone()
                } else {
                    format!("{stdout}\n{stderr}")
                };

                result.details.push(HookDetail {
                    command: hook.command.clone(),
                    duration_ms,
                    success,
                    output: if output_text.trim().is_empty() {
                        None
                    } else {
                        Some(output_text.clone())
                    },
                });

                if !success && hook.blocking {
                    info!(
                        command = %hook.command,
                        "Blocking stop hook failed — preventing continuation"
                    );
                    result.prevent_continuation = true;
                    result.blocking_errors.push(format!(
                        "Stop hook `{}` failed: {}",
                        hook.command,
                        output_text.trim()
                    ));
                }
            }
            Err(e) => {
                warn!(command = %hook.command, error = %e, "Stop hook execution failed");
                result.details.push(HookDetail {
                    command: hook.command.clone(),
                    duration_ms,
                    success: false,
                    output: Some(format!("Failed to execute: {e}")),
                });

                if hook.blocking {
                    result.prevent_continuation = true;
                    result.blocking_errors.push(format!(
                        "Stop hook `{}` failed to execute: {e}",
                        hook.command
                    ));
                }
            }
        }
    }

    result
}

/// Standard stop conditions checked in the query loop.
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Model naturally finished (end_turn, no tool calls).
    Completed,
    /// Max turn limit reached.
    MaxTurns(u32),
    /// USD budget exceeded.
    MaxBudget(f64),
    /// Token budget exhausted.
    TokenBudget(String),
    /// User abort (Ctrl-C).
    Aborted,
    /// API error.
    ApiError(String),
    /// Stop hook prevented continuation.
    HookPrevented(String),
}

impl StopReason {
    pub fn is_error(&self) -> bool {
        matches!(self, StopReason::ApiError(_))
    }

    pub fn display(&self) -> String {
        match self {
            StopReason::Completed => "Completed".to_string(),
            StopReason::MaxTurns(n) => format!("Max turns ({n}) reached"),
            StopReason::MaxBudget(usd) => format!("Budget limit (${usd:.2}) reached"),
            StopReason::TokenBudget(s) => s.clone(),
            StopReason::Aborted => "Aborted by user".to_string(),
            StopReason::ApiError(e) => format!("API error: {e}"),
            StopReason::HookPrevented(s) => format!("Stopped by hook: {s}"),
        }
    }
}
