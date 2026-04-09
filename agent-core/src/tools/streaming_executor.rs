//! Streaming tool executor — manages concurrent tool execution during API streaming.
//!
//! Mirrors `src/services/tools/StreamingToolExecutor.ts`.
//!
//! Tools start executing the moment their `tool_use` block finishes streaming,
//! while the API may still be streaming subsequent blocks. The executor manages:
//!
//! - **Concurrency**: Read-only tools (FileRead, Grep, Glob, etc.) run in parallel.
//!   Write tools (Bash, FileEdit, FileWrite) get exclusive access.
//! - **Ordering**: Results are buffered and yielded in insertion order — tool N's
//!   results are not yielded until all tools before it have been yielded.
//! - **Sibling abort**: If a Bash tool errors, it cancels all sibling tools via
//!   a child abort handle. Other tool errors are independent.
#![allow(dead_code)]

use std::collections::VecDeque;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex, Notify};

use crate::permissions::gate::PermissionGate;
use crate::query::abort::AbortHandle;

use super::{ToolOutput, ToolResult, ToolRegistry};
use crate::query::message::ToolUseBlock;
use crate::query::query_loop::QueryEvent;

// ─── Concurrency classification ─────────────────────────────────────────────

/// Tools that are safe to execute concurrently (read-only, no side effects).
const CONCURRENCY_SAFE: &[&str] = &[
    "FileRead", "Glob", "Grep", "WebSearch", "WebFetch", "ToolSearch",
    "TaskGet", "TaskList", "TaskOutput", "Brief", "Sleep", "ConfigTool",
];

fn is_concurrency_safe(tool_name: &str) -> bool {
    CONCURRENCY_SAFE.contains(&tool_name)
}

/// Only Bash errors cascade-cancel siblings.
fn is_bash_tool(tool_name: &str) -> bool {
    tool_name == "Bash" || tool_name == "PowerShell"
}

// ─── Tracked tool ───────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum ToolStatus {
    Queued,
    Executing,
    Completed,
    Yielded,
}

struct TrackedTool {
    id: String,
    block: ToolUseBlock,
    status: ToolStatus,
    concurrency_safe: bool,
    result: Option<ToolResult>,
    /// Pending progress events (streaming output) to yield.
    pending_progress: Vec<QueryEvent>,
    /// Handle to await completion.
    completion_notify: Arc<Notify>,
}

// ─── StreamingToolExecutor ──────────────────────────────────────────────────

/// Manages concurrent tool execution with in-order result delivery.
pub struct StreamingToolExecutor {
    tools: Arc<Mutex<VecDeque<TrackedTool>>>,
    registry: ToolRegistry,
    gate: PermissionGate,
    event_tx: mpsc::Sender<QueryEvent>,
    /// Child abort handle — Bash errors cancel siblings.
    sibling_abort: AbortHandle,
    /// Parent abort handle — user Ctrl-C cancels everything.
    parent_abort: AbortHandle,
    discarded: bool,
}

impl StreamingToolExecutor {
    pub fn new(
        registry: ToolRegistry,
        gate: PermissionGate,
        event_tx: mpsc::Sender<QueryEvent>,
        parent_abort: AbortHandle,
    ) -> Self {
        StreamingToolExecutor {
            tools: Arc::new(Mutex::new(VecDeque::new())),
            registry,
            gate,
            event_tx,
            sibling_abort: parent_abort.create_child(),
            parent_abort,
            discarded: false,
        }
    }

    /// Discard the executor (e.g., on streaming fallback).
    pub fn discard(&mut self) {
        self.discarded = true;
        self.sibling_abort.abort(crate::query::abort::AbortReason::Custom(
            "Executor discarded".into()
        ));
    }

    /// Add a tool for execution. Called during API streaming as each
    /// tool_use block completes.
    pub async fn add_tool(&self, block: ToolUseBlock) {
        if self.discarded {
            return;
        }

        let concurrency_safe = is_concurrency_safe(&block.name);
        let notify = Arc::new(Notify::new());
        let tracked = TrackedTool {
            id: block.id.clone(),
            block,
            status: ToolStatus::Queued,
            concurrency_safe,
            result: None,
            pending_progress: Vec::new(),
            completion_notify: notify,
        };

        self.tools.lock().await.push_back(tracked);
        self.process_queue().await;
    }

    /// Process the queue — start executing tools that can run.
    async fn process_queue(&self) {
        let mut tools = self.tools.lock().await;

        for i in 0..tools.len() {
            if tools[i].status != ToolStatus::Queued {
                continue;
            }

            let can_execute = self.can_execute_tool(&tools, tools[i].concurrency_safe);
            if !can_execute {
                if !tools[i].concurrency_safe {
                    break; // Non-concurrent tool blocks the queue
                }
                continue;
            }

            // Start execution
            tools[i].status = ToolStatus::Executing;
            let block = tools[i].block.clone();
            let tool_id = tools[i].id.clone();
            let notify = tools[i].completion_notify.clone();

            let registry = self.registry.clone_for_concurrent();
            let gate = self.gate.clone();
            let event_tx = self.event_tx.clone();
            let tools_arc = self.tools.clone();
            let sibling_abort = self.sibling_abort.clone();
            let parent_abort = self.parent_abort.clone();

            tokio::spawn(async move {
                // Emit ToolStart
                let _ = event_tx.send(QueryEvent::ToolStart {
                    id: block.id.clone(),
                    name: block.name.clone(),
                }).await;

                // Execute with output forwarding
                let (tool_tx, mut tool_rx) = mpsc::channel::<ToolOutput>(64);
                let event_tx2 = event_tx.clone();
                let tools_arc2 = tools_arc.clone();
                let tid = tool_id.clone();

                // Forward streaming output + buffer as pending progress
                let fwd = tokio::spawn(async move {
                    while let Some(output) = tool_rx.recv().await {
                        let _ = event_tx2.send(QueryEvent::ToolOutput(output.clone())).await;
                        // Also store in pending_progress for ordered yield
                        let mut tools = tools_arc2.lock().await;
                        if let Some(t) = tools.iter_mut().find(|t| t.id == tid) {
                            t.pending_progress.push(QueryEvent::ToolOutput(output));
                        }
                    }
                });

                let result = tokio::select! {
                    r = registry.execute_with_permission(
                        &block.name, block.input.clone(), &gate, tool_tx,
                    ) => {
                        r.unwrap_or_else(|e| ToolResult::error(format!("Tool error: {e}")))
                    }
                    _ = parent_abort.cancelled() => {
                        ToolResult::error("Interrupted by user".to_string())
                    }
                    _ = sibling_abort.cancelled() => {
                        ToolResult::error("Cancelled (sibling tool error)".to_string())
                    }
                };

                let _ = fwd.await;

                // If Bash errored, cascade to siblings
                if result.is_error && is_bash_tool(&block.name) {
                    sibling_abort.abort(crate::query::abort::AbortReason::SiblingError);
                }

                // Emit ToolDone
                let _ = event_tx.send(QueryEvent::ToolDone {
                    id: block.id.clone(),
                    result: result.clone(),
                }).await;

                // Mark completed
                {
                    let mut tools = tools_arc.lock().await;
                    if let Some(t) = tools.iter_mut().find(|t| t.id == tool_id) {
                        t.result = Some(result);
                        t.status = ToolStatus::Completed;
                    }
                }

                notify.notify_waiters();
            });
        }
    }

    /// Check if a tool with the given concurrency classification can execute.
    fn can_execute_tool(&self, tools: &VecDeque<TrackedTool>, is_safe: bool) -> bool {
        let executing: Vec<&TrackedTool> = tools.iter()
            .filter(|t| t.status == ToolStatus::Executing)
            .collect();
        executing.is_empty() || (is_safe && executing.iter().all(|t| t.concurrency_safe))
    }

    /// Yield completed results in order (non-blocking).
    /// Returns the tool results ready to be sent back to the API.
    pub async fn get_completed_results(&self) -> Vec<crate::query::message::ToolResultBlock> {
        let mut results = Vec::new();
        let mut tools = self.tools.lock().await;

        for tool in tools.iter_mut() {
            match tool.status {
                ToolStatus::Completed => {
                    if let Some(ref result) = tool.result {
                        results.push(crate::query::message::ToolResultBlock {
                            tool_use_id: tool.id.clone(),
                            content: result.content.clone(),
                            is_error: result.is_error,
                        });
                        tool.status = ToolStatus::Yielded;
                    }
                }
                ToolStatus::Executing | ToolStatus::Queued => {
                    break; // Must yield in order
                }
                ToolStatus::Yielded => continue,
            }
        }

        // Remove yielded tools from the front
        while tools.front().map(|t| t.status == ToolStatus::Yielded).unwrap_or(false) {
            tools.pop_front();
        }

        results
    }

    /// Wait for all remaining tools to complete and yield their results.
    pub async fn get_remaining_results(&self) -> Vec<crate::query::message::ToolResultBlock> {
        loop {
            // Try to get completed results first
            let completed = self.get_completed_results().await;
            if !completed.is_empty() {
                return completed;
            }

            // Check if anything is still pending
            let tools = self.tools.lock().await;
            let has_pending = tools.iter().any(|t| {
                t.status == ToolStatus::Queued || t.status == ToolStatus::Executing
            });
            if !has_pending {
                return Vec::new(); // All done
            }

            // Wait for any tool to complete
            let notify = tools.iter()
                .find(|t| t.status == ToolStatus::Executing)
                .map(|t| t.completion_notify.clone());
            drop(tools);

            if let Some(notify) = notify {
                notify.notified().await;
                // Process queue for more work
                self.process_queue().await;
            } else {
                // Process queue for queued items
                self.process_queue().await;
                tokio::task::yield_now().await;
            }
        }
    }

    /// Drain ALL remaining results (blocking until all tools finish).
    pub async fn drain_all_results(&self) -> Vec<crate::query::message::ToolResultBlock> {
        let mut all_results = Vec::new();
        loop {
            let batch = self.get_remaining_results().await;
            if batch.is_empty() {
                break;
            }
            all_results.extend(batch);
        }
        all_results
    }

    /// Check if there are any tools still queued or executing.
    pub async fn has_pending(&self) -> bool {
        let tools = self.tools.lock().await;
        tools.iter().any(|t| t.status == ToolStatus::Queued || t.status == ToolStatus::Executing)
    }
}
