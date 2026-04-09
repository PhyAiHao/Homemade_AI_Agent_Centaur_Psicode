//! Session tracing — typed span lifecycle API.
//!
//! Mirrors `src/utils/telemetry/sessionTracing.ts`.
//!
//! Provides a hierarchical span system with 6 span types:
//! - interaction: A full user turn (prompt → response)
//! - llm_request: A single API call within an interaction
//! - tool: A tool execution request
//! - tool_blocked: User permission prompt wait
//! - tool_execution: Actual tool execution
//! - hook: Stop/pre/post hook execution
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Span types matching the original.
#[derive(Debug, Clone, PartialEq)]
pub enum SpanType {
    Interaction,
    LlmRequest,
    Tool,
    ToolBlocked,
    ToolExecution,
    Hook,
}

impl SpanType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Interaction => "interaction",
            Self::LlmRequest => "llm_request",
            Self::Tool => "tool",
            Self::ToolBlocked => "tool.blocked_on_user",
            Self::ToolExecution => "tool.execution",
            Self::Hook => "hook",
        }
    }
}

/// A single trace span.
#[derive(Debug, Clone)]
pub struct Span {
    pub span_id: String,
    pub parent_id: Option<String>,
    pub span_type: SpanType,
    pub name: String,
    pub start_time: Instant,
    pub end_time: Option<Instant>,
    pub attributes: HashMap<String, serde_json::Value>,
    pub events: Vec<SpanEvent>,
}

/// An event within a span.
#[derive(Debug, Clone)]
pub struct SpanEvent {
    pub name: String,
    pub timestamp: Instant,
    pub attributes: HashMap<String, serde_json::Value>,
}

/// Thread-safe session trace manager.
pub struct SessionTracer {
    session_id: String,
    trace_id: String,
    spans: Arc<Mutex<HashMap<String, Span>>>,
    /// Stack of active span IDs for parent-child context.
    context_stack: Arc<Mutex<Vec<String>>>,
}

impl SessionTracer {
    pub fn new(session_id: &str) -> Self {
        SessionTracer {
            session_id: session_id.to_string(),
            trace_id: uuid::Uuid::new_v4().to_string(),
            spans: Arc::new(Mutex::new(HashMap::new())),
            context_stack: Arc::new(Mutex::new(Vec::new())),
        }
    }

    /// Get the current parent span ID (top of context stack).
    fn current_parent(&self) -> Option<String> {
        self.context_stack.lock().ok()
            .and_then(|stack| stack.last().cloned())
    }

    /// Start a span and push it as the current context.
    fn start_span_internal(&self, span_type: SpanType, name: &str) -> String {
        let span_id = uuid::Uuid::new_v4().to_string();
        let parent_id = self.current_parent();

        let span = Span {
            span_id: span_id.clone(),
            parent_id,
            span_type,
            name: name.to_string(),
            start_time: Instant::now(),
            end_time: None,
            attributes: HashMap::new(),
            events: Vec::new(),
        };

        if let Ok(mut spans) = self.spans.lock() {
            spans.insert(span_id.clone(), span);
        }
        if let Ok(mut stack) = self.context_stack.lock() {
            stack.push(span_id.clone());
        }

        // Bridge to Perfetto
        super::with_perfetto(|p| {
            let tid = match name {
                n if n.contains("API") || n.contains("llm") => super::perfetto::threads::API,
                n if n.contains("tool") || n.contains("Tool") => super::perfetto::threads::TOOLS,
                _ => super::perfetto::threads::MAIN,
            };
            p.begin(name, "session", tid, None);
        });

        span_id
    }

    /// End a span and pop it from the context stack.
    fn end_span_internal(&self, span_id: &str) {
        if let Ok(mut spans) = self.spans.lock() {
            if let Some(span) = spans.get_mut(span_id) {
                span.end_time = Some(Instant::now());

                // Bridge to Perfetto
                let name = span.name.clone();
                let duration_ms = span.start_time.elapsed().as_millis() as u64;
                super::with_perfetto(|p| {
                    let tid = match name.as_str() {
                        n if n.contains("API") || n.contains("llm") => super::perfetto::threads::API,
                        n if n.contains("tool") || n.contains("Tool") => super::perfetto::threads::TOOLS,
                        _ => super::perfetto::threads::MAIN,
                    };
                    p.end(&name, "session", tid, Some(serde_json::json!({
                        "duration_ms": duration_ms,
                    })));
                });
            }
        }
        if let Ok(mut stack) = self.context_stack.lock() {
            stack.retain(|id| id != span_id);
        }
    }

    /// Set an attribute on a span.
    pub fn set_attribute(&self, span_id: &str, key: &str, value: impl Into<serde_json::Value>) {
        if let Ok(mut spans) = self.spans.lock() {
            if let Some(span) = spans.get_mut(span_id) {
                span.attributes.insert(key.to_string(), value.into());
            }
        }
    }

    /// Add an event to a span.
    pub fn add_event(&self, span_id: &str, name: &str, attrs: HashMap<String, serde_json::Value>) {
        if let Ok(mut spans) = self.spans.lock() {
            if let Some(span) = spans.get_mut(span_id) {
                span.events.push(SpanEvent {
                    name: name.to_string(),
                    timestamp: Instant::now(),
                    attributes: attrs,
                });
            }
        }
    }

    // ─── High-level span API (matching original) ────────────────────

    /// Start an interaction span (full user turn).
    pub fn start_interaction(&self, turn: u32) -> String {
        let id = self.start_span_internal(SpanType::Interaction, "interaction");
        self.set_attribute(&id, "turn", serde_json::json!(turn));
        id
    }

    /// End an interaction span.
    pub fn end_interaction(&self, span_id: &str, stop_reason: &str) {
        self.set_attribute(span_id, "stop_reason", serde_json::json!(stop_reason));
        self.end_span_internal(span_id);
    }

    /// Start an LLM request span.
    pub fn start_llm_request(&self, model: &str, turn: u32) -> String {
        let id = self.start_span_internal(SpanType::LlmRequest, "llm_request");
        self.set_attribute(&id, "model", serde_json::json!(model));
        self.set_attribute(&id, "turn", serde_json::json!(turn));
        id
    }

    /// End an LLM request span.
    pub fn end_llm_request(&self, span_id: &str, input_tokens: u64, output_tokens: u64) {
        self.set_attribute(span_id, "input_tokens", serde_json::json!(input_tokens));
        self.set_attribute(span_id, "output_tokens", serde_json::json!(output_tokens));
        self.end_span_internal(span_id);
    }

    /// Start a tool span.
    pub fn start_tool(&self, tool_name: &str, tool_id: &str) -> String {
        let id = self.start_span_internal(SpanType::Tool, &format!("tool:{tool_name}"));
        self.set_attribute(&id, "tool_name", serde_json::json!(tool_name));
        self.set_attribute(&id, "tool_id", serde_json::json!(tool_id));
        id
    }

    /// End a tool span.
    pub fn end_tool(&self, span_id: &str, success: bool) {
        self.set_attribute(span_id, "success", serde_json::json!(success));
        self.end_span_internal(span_id);
    }

    /// Start a tool-blocked-on-user span (permission prompt).
    pub fn start_tool_blocked(&self, tool_name: &str) -> String {
        self.start_span_internal(SpanType::ToolBlocked, &format!("tool.blocked:{tool_name}"))
    }

    /// End a tool-blocked span.
    pub fn end_tool_blocked(&self, span_id: &str, approved: bool) {
        self.set_attribute(span_id, "approved", serde_json::json!(approved));
        self.end_span_internal(span_id);
    }

    /// Start a tool execution span.
    pub fn start_tool_execution(&self, tool_name: &str) -> String {
        self.start_span_internal(SpanType::ToolExecution, &format!("tool.exec:{tool_name}"))
    }

    /// End a tool execution span.
    pub fn end_tool_execution(&self, span_id: &str) {
        self.end_span_internal(span_id);
    }

    /// Start a hook span.
    pub fn start_hook(&self, hook_name: &str) -> String {
        self.start_span_internal(SpanType::Hook, &format!("hook:{hook_name}"))
    }

    /// End a hook span.
    pub fn end_hook(&self, span_id: &str) {
        self.end_span_internal(span_id);
    }

    // ─── Queries ────────────────────────────────────────────────────

    /// Get all completed spans.
    pub fn completed_spans(&self) -> Vec<Span> {
        self.spans.lock().ok()
            .map(|spans| spans.values()
                .filter(|s| s.end_time.is_some())
                .cloned()
                .collect())
            .unwrap_or_default()
    }

    /// Get total span count.
    pub fn span_count(&self) -> usize {
        self.spans.lock().ok().map(|s| s.len()).unwrap_or(0)
    }

    /// Cleanup stale spans (older than 30 minutes).
    pub fn cleanup_stale(&self) {
        let cutoff = std::time::Duration::from_secs(30 * 60);
        if let Ok(mut spans) = self.spans.lock() {
            spans.retain(|_, span| {
                span.start_time.elapsed() < cutoff
            });
        }
    }
}
