//! Telemetry events — structured event types for analytics and OTEL logging.
//!
//! Mirrors `src/utils/telemetry/events.ts`, `pluginTelemetry.ts`, and
//! `skillLoadedEvent.ts`.
#![allow(dead_code)]

use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::util::now_ms;

/// Global event sequence number.
static SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// A telemetry event with structured properties.
#[derive(Debug, Clone, Serialize)]
pub struct TelemetryEvent {
    pub name: String,
    pub timestamp_ms: u64,
    pub sequence: u64,
    pub properties: HashMap<String, serde_json::Value>,
}

impl TelemetryEvent {
    pub fn new(name: &str) -> Self {
        TelemetryEvent {
            name: name.to_string(),
            timestamp_ms: now_ms(),
            sequence: SEQUENCE.fetch_add(1, Ordering::SeqCst),
            properties: HashMap::new(),
        }
    }

    pub fn prop(mut self, key: &str, value: impl Into<serde_json::Value>) -> Self {
        self.properties.insert(key.to_string(), value.into());
        self
    }
}

/// Log a telemetry event.
///
/// Routes to both the tracing subscriber (always) and the metrics collector
/// (when enabled).
pub fn log_event(event: TelemetryEvent) {
    tracing::info!(
        event = %event.name,
        seq = event.sequence,
        properties = ?event.properties,
        "telemetry_event"
    );

    // Also record as a metric if applicable
    match event.name.as_str() {
        "api_call" => {
            if let (Some(_tokens), Some(cost)) = (
                event.properties.get("input_tokens").and_then(|v| v.as_u64()),
                event.properties.get("cost_usd").and_then(|v| v.as_f64()),
            ) {
                super::record_metric("api.calls", 1.0, &[]);
                super::record_metric("api.cost_usd", cost, &[]);
            }
        }
        "tool_execution" => {
            super::record_metric("tool.executions", 1.0, &[]);
        }
        _ => {}
    }
}

// ─── Common event constructors ──────────────────────────────────────────────

pub fn session_start(model: &str, session_id: &str) -> TelemetryEvent {
    TelemetryEvent::new("session_start")
        .prop("model", model)
        .prop("session_id", session_id)
        .prop("agent_version", env!("CARGO_PKG_VERSION"))
        .prop("os", std::env::consts::OS)
        .prop("arch", std::env::consts::ARCH)
}

pub fn session_end(session_id: &str, turns: u32, cost_usd: f64, duration_ms: u64) -> TelemetryEvent {
    TelemetryEvent::new("session_end")
        .prop("session_id", session_id)
        .prop("turns", turns)
        .prop("cost_usd", cost_usd)
        .prop("duration_ms", duration_ms)
}

pub fn tool_execution(tool_name: &str, duration_ms: u64, success: bool) -> TelemetryEvent {
    TelemetryEvent::new("tool_execution")
        .prop("tool", tool_name)
        .prop("duration_ms", duration_ms)
        .prop("success", success)
}

pub fn api_call(
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
    cache_read_tokens: u64,
    duration_ms: u64,
    cost_usd: f64,
) -> TelemetryEvent {
    TelemetryEvent::new("api_call")
        .prop("model", model)
        .prop("input_tokens", input_tokens)
        .prop("output_tokens", output_tokens)
        .prop("cache_read_tokens", cache_read_tokens)
        .prop("duration_ms", duration_ms)
        .prop("cost_usd", cost_usd)
}

pub fn compact_event(
    trigger: &str,
    messages_before: usize,
    messages_after: usize,
    tokens_before: u64,
) -> TelemetryEvent {
    TelemetryEvent::new("compact")
        .prop("trigger", trigger)
        .prop("messages_before", messages_before)
        .prop("messages_after", messages_after)
        .prop("tokens_before", tokens_before)
}

pub fn permission_decision(
    tool_name: &str,
    decision: &str,
    mode: &str,
) -> TelemetryEvent {
    TelemetryEvent::new("permission_decision")
        .prop("tool", tool_name)
        .prop("decision", decision)
        .prop("mode", mode)
}

pub fn error_event(error_type: &str, message: &str) -> TelemetryEvent {
    TelemetryEvent::new("error")
        .prop("error_type", error_type)
        .prop("message", message)
}

// ─── Plugin telemetry ───────────────────────────────────────────────────────

/// Log that a plugin was enabled for this session.
/// Privacy: hash-based plugin ID, redacted name for third-party.
pub fn plugin_enabled(plugin_name: &str, scope: &str) -> TelemetryEvent {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    plugin_name.hash(&mut hasher);
    let hashed_id = format!("{:x}", hasher.finish());

    TelemetryEvent::new("plugin_enabled_for_session")
        .prop("plugin_id_hash", hashed_id)
        .prop("scope", scope)
}

/// Log a plugin load error.
pub fn plugin_load_error(plugin_name: &str, error: &str) -> TelemetryEvent {
    TelemetryEvent::new("plugin_load_failed")
        .prop("plugin_name", plugin_name)
        .prop("error", error)
}

// ─── Skill telemetry ────────────────────────────────────────────────────────

/// Log that skills were loaded at session startup.
pub fn skills_loaded(skill_names: &[&str]) -> TelemetryEvent {
    TelemetryEvent::new("skills_loaded")
        .prop("count", skill_names.len())
        .prop("names", serde_json::json!(skill_names))
}

/// Log that a specific skill was invoked.
pub fn skill_invoked(skill_name: &str) -> TelemetryEvent {
    TelemetryEvent::new("skill_invoked")
        .prop("skill_name", skill_name)
}

// ─── MCP telemetry ──────────────────────────────────────────────────────────

/// Log MCP server connection.
pub fn mcp_server_connected(server_name: &str, tool_count: usize) -> TelemetryEvent {
    TelemetryEvent::new("mcp_server_connected")
        .prop("server_name", server_name)
        .prop("tool_count", tool_count)
}

/// Log MCP tool execution.
pub fn mcp_tool_call(server_name: &str, tool_name: &str, duration_ms: u64, success: bool) -> TelemetryEvent {
    TelemetryEvent::new("mcp_tool_call")
        .prop("server_name", server_name)
        .prop("tool_name", tool_name)
        .prop("duration_ms", duration_ms)
        .prop("success", success)
}

