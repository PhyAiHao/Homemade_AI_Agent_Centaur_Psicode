//! Perfetto trace writer — produces Chrome Trace Event JSON.
//!
//! Mirrors `src/utils/telemetry/perfettoTracing.ts`.
//!
//! Output format: Chrome Trace Event Format (viewable at ui.perfetto.dev).
//! Writes to `~/.claude/traces/trace-<session-id>.json`.
//!
//! Event types:
//! - B/E: Begin/End duration events (API calls, tool executions)
//! - i: Instant events (markers, state changes)
//! - C: Counter events (token usage, cost)
//! - M: Metadata events (process/thread names)
#![allow(dead_code)]

use anyhow::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Instant;

/// Maximum events before half-eviction.
const MAX_EVENTS: usize = 100_000;

/// A Chrome Trace Event.
#[derive(Debug, Clone, Serialize)]
pub struct TraceEvent {
    /// Event name.
    pub name: String,
    /// Category.
    pub cat: String,
    /// Phase: B (begin), E (end), i (instant), C (counter), M (metadata).
    pub ph: String,
    /// Timestamp in microseconds.
    pub ts: u64,
    /// Process ID (agent hierarchy level).
    pub pid: u32,
    /// Thread ID (operation type).
    pub tid: u32,
    /// Arguments.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<serde_json::Value>,
    /// Duration (for complete events, phase X).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dur: Option<u64>,
}

/// Thread IDs for different operation types.
pub mod threads {
    pub const MAIN: u32 = 0;
    pub const API: u32 = 1;
    pub const TOOLS: u32 = 2;
    pub const USER_INPUT: u32 = 3;
    pub const HOOKS: u32 = 4;
    pub const COMPACT: u32 = 5;
    pub const MEMORY: u32 = 6;
}

/// Perfetto trace file writer.
pub struct PerfettoTracer {
    events: Vec<TraceEvent>,
    output_path: PathBuf,
    start_time: Instant,
    session_id: String,
    next_pid: u32,
}

impl PerfettoTracer {
    /// Create a new Perfetto tracer for the given session.
    pub fn new(session_id: &str) -> Result<Self> {
        let traces_dir = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".claude")
            .join("traces");
        std::fs::create_dir_all(&traces_dir)?;

        let output_path = traces_dir.join(format!("trace-{session_id}.json"));

        let mut tracer = PerfettoTracer {
            events: Vec::with_capacity(1000),
            output_path,
            start_time: Instant::now(),
            session_id: session_id.to_string(),
            next_pid: 1,
        };

        // Emit metadata events
        tracer.emit_metadata(0, threads::MAIN, "Main Agent");
        tracer.emit_metadata(0, threads::API, "API Calls");
        tracer.emit_metadata(0, threads::TOOLS, "Tool Execution");
        tracer.emit_metadata(0, threads::USER_INPUT, "User Input");
        tracer.emit_metadata(0, threads::HOOKS, "Hooks");

        Ok(tracer)
    }

    pub fn output_path(&self) -> &Path {
        &self.output_path
    }

    fn timestamp_us(&self) -> u64 {
        self.start_time.elapsed().as_micros() as u64
    }

    fn push_event(&mut self, event: TraceEvent) {
        if self.events.len() >= MAX_EVENTS {
            // Half-eviction: remove oldest 50%
            let keep_from = self.events.len() / 2;
            self.events.drain(..keep_from);
        }
        self.events.push(event);
    }

    // ─── Emit helpers ───────────────────────────────────────────────

    fn emit_metadata(&mut self, pid: u32, tid: u32, name: &str) {
        self.push_event(TraceEvent {
            name: "thread_name".into(),
            cat: "__metadata".into(),
            ph: "M".into(),
            ts: 0,
            pid,
            tid,
            args: Some(serde_json::json!({"name": name})),
            dur: None,
        });
    }

    /// Begin a duration span.
    pub fn begin(&mut self, name: &str, cat: &str, tid: u32, args: Option<serde_json::Value>) -> u64 {
        let ts = self.timestamp_us();
        self.push_event(TraceEvent {
            name: name.into(),
            cat: cat.into(),
            ph: "B".into(),
            ts,
            pid: 0,
            tid,
            args,
            dur: None,
        });
        ts
    }

    /// End a duration span.
    pub fn end(&mut self, name: &str, cat: &str, tid: u32, args: Option<serde_json::Value>) {
        self.push_event(TraceEvent {
            name: name.into(),
            cat: cat.into(),
            ph: "E".into(),
            ts: self.timestamp_us(),
            pid: 0,
            tid,
            args,
            dur: None,
        });
    }

    /// Emit an instant event.
    pub fn instant(&mut self, name: &str, cat: &str, tid: u32, args: Option<serde_json::Value>) {
        self.push_event(TraceEvent {
            name: name.into(),
            cat: cat.into(),
            ph: "i".into(),
            ts: self.timestamp_us(),
            pid: 0,
            tid,
            args,
            dur: None,
        });
    }

    /// Emit a counter event.
    pub fn counter(&mut self, name: &str, values: serde_json::Value) {
        self.push_event(TraceEvent {
            name: name.into(),
            cat: "counter".into(),
            ph: "C".into(),
            ts: self.timestamp_us(),
            pid: 0,
            tid: threads::MAIN,
            args: Some(values),
            dur: None,
        });
    }

    // ─── High-level span API ────────────────────────────────────────

    /// Start an API call span.
    pub fn start_api_call(&mut self, model: &str, turn: u32) -> u64 {
        self.begin("API Call", "api", threads::API, Some(serde_json::json!({
            "model": model,
            "turn": turn,
        })))
    }

    /// End an API call span with usage stats.
    pub fn end_api_call(&mut self, input_tokens: u64, output_tokens: u64, duration_ms: u64) {
        self.end("API Call", "api", threads::API, Some(serde_json::json!({
            "input_tokens": input_tokens,
            "output_tokens": output_tokens,
            "duration_ms": duration_ms,
        })));
    }

    /// Start a tool execution span.
    pub fn start_tool(&mut self, tool_name: &str, tool_id: &str) -> u64 {
        self.begin(tool_name, "tool", threads::TOOLS, Some(serde_json::json!({
            "tool_id": tool_id,
        })))
    }

    /// End a tool execution span.
    pub fn end_tool(&mut self, tool_name: &str, success: bool, duration_ms: u64) {
        self.end(tool_name, "tool", threads::TOOLS, Some(serde_json::json!({
            "success": success,
            "duration_ms": duration_ms,
        })));
    }

    /// Start a user input wait span.
    pub fn start_user_input(&mut self) -> u64 {
        self.begin("User Input", "user", threads::USER_INPUT, None)
    }

    /// End a user input wait span.
    pub fn end_user_input(&mut self) {
        self.end("User Input", "user", threads::USER_INPUT, None);
    }

    /// Start an interaction span (full user turn).
    pub fn start_interaction(&mut self, turn: u32) -> u64 {
        self.begin("Interaction", "interaction", threads::MAIN, Some(serde_json::json!({
            "turn": turn,
        })))
    }

    /// End an interaction span.
    pub fn end_interaction(&mut self, stop_reason: &str) {
        self.end("Interaction", "interaction", threads::MAIN, Some(serde_json::json!({
            "stop_reason": stop_reason,
        })));
    }

    /// Record token usage as a counter.
    pub fn record_tokens(&mut self, input: u64, output: u64, cost_usd: f64) {
        self.counter("tokens", serde_json::json!({
            "input_tokens": input,
            "output_tokens": output,
            "cost_usd": cost_usd,
        }));
    }

    /// Register a sub-agent in the trace.
    pub fn register_agent(&mut self, agent_name: &str) -> u32 {
        let pid = self.next_pid;
        self.next_pid += 1;
        self.push_event(TraceEvent {
            name: "process_name".into(),
            cat: "__metadata".into(),
            ph: "M".into(),
            ts: 0,
            pid,
            tid: 0,
            args: Some(serde_json::json!({"name": agent_name})),
            dur: None,
        });
        pid
    }

    // ─── Output ─────────────────────────────────────────────────────

    /// Flush all events to the output file.
    pub fn flush(&mut self) -> Result<()> {
        if self.events.is_empty() {
            return Ok(());
        }
        let json = serde_json::to_string_pretty(&self.events)?;
        let content = format!("[{}]", &json[1..json.len()-1]); // unwrap the outer array
        std::fs::write(&self.output_path, content)?;
        tracing::info!(
            path = %self.output_path.display(),
            events = self.events.len(),
            "Perfetto trace written"
        );
        Ok(())
    }

    /// Number of recorded events.
    pub fn event_count(&self) -> usize {
        self.events.len()
    }
}

impl Drop for PerfettoTracer {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}
