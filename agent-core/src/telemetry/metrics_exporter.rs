//! Metrics exporter — collects and exports metrics via HTTP.
//!
//! Mirrors `src/utils/telemetry/bigqueryExporter.ts`.
//! Collects token usage, cost, latency, and tool execution metrics.
//! Can export via HTTP POST to a configurable endpoint.
#![allow(dead_code)]

use serde::Serialize;
use std::collections::HashMap;
use tracing::debug;

/// A single metric data point.
#[derive(Debug, Clone, Serialize)]
pub struct MetricPoint {
    pub name: String,
    pub value: f64,
    pub labels: HashMap<String, String>,
    pub timestamp_ms: u64,
}

/// Collects metrics in memory for periodic export.
pub struct MetricsCollector {
    session_id: String,
    agent_version: String,
    points: Vec<MetricPoint>,
    /// Cumulative counters.
    counters: HashMap<String, f64>,
}

impl MetricsCollector {
    pub fn new(session_id: &str, agent_version: &str) -> Self {
        MetricsCollector {
            session_id: session_id.to_string(),
            agent_version: agent_version.to_string(),
            points: Vec::new(),
            counters: HashMap::new(),
        }
    }

    /// Record a metric data point.
    pub fn record(&mut self, name: &str, value: f64, labels: &[(&str, &str)]) {
        let mut label_map = HashMap::new();
        for (k, v) in labels {
            label_map.insert(k.to_string(), v.to_string());
        }

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        self.points.push(MetricPoint {
            name: name.to_string(),
            value,
            labels: label_map,
            timestamp_ms: ts,
        });

        // Update cumulative counter
        *self.counters.entry(name.to_string()).or_default() += value;
    }

    /// Record common API call metrics.
    pub fn record_api_call(
        &mut self,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        cache_read_tokens: u64,
        duration_ms: u64,
        cost_usd: f64,
    ) {
        let labels = [("model", model)];
        self.record("api.input_tokens", input_tokens as f64, &labels);
        self.record("api.output_tokens", output_tokens as f64, &labels);
        self.record("api.cache_read_tokens", cache_read_tokens as f64, &labels);
        self.record("api.duration_ms", duration_ms as f64, &labels);
        self.record("api.cost_usd", cost_usd, &labels);
        self.record("api.calls", 1.0, &labels);
    }

    /// Record tool execution metrics.
    pub fn record_tool_execution(
        &mut self,
        tool_name: &str,
        duration_ms: u64,
        success: bool,
    ) {
        let labels = [
            ("tool", tool_name),
            ("success", if success { "true" } else { "false" }),
        ];
        self.record("tool.executions", 1.0, &labels);
        self.record("tool.duration_ms", duration_ms as f64, &labels);
    }

    /// Get the current counter value for a metric.
    pub fn counter_value(&self, name: &str) -> f64 {
        self.counters.get(name).copied().unwrap_or(0.0)
    }

    /// Build a summary of collected metrics.
    pub fn summary(&self) -> MetricsSummary {
        MetricsSummary {
            session_id: self.session_id.clone(),
            total_api_calls: self.counter_value("api.calls") as u64,
            total_input_tokens: self.counter_value("api.input_tokens") as u64,
            total_output_tokens: self.counter_value("api.output_tokens") as u64,
            total_cost_usd: self.counter_value("api.cost_usd"),
            total_tool_executions: self.counter_value("tool.executions") as u64,
            data_points: self.points.len(),
        }
    }

    /// Build the export payload (for HTTP POST).
    pub fn build_export_payload(&self) -> ExportPayload {
        ExportPayload {
            session_id: self.session_id.clone(),
            agent_version: self.agent_version.clone(),
            resource_attributes: {
                let mut attrs = HashMap::new();
                attrs.insert("service.name".into(), "centaur-agent".into());
                attrs.insert("service.version".into(), self.agent_version.clone());
                attrs.insert("session.id".into(), self.session_id.clone());
                attrs
            },
            metrics: self.points.clone(),
        }
    }

    /// Export metrics via HTTP POST (async).
    pub async fn export_http(&self, endpoint: &str) -> Result<(), String> {
        let payload = self.build_export_payload();
        let body = serde_json::to_string(&payload)
            .map_err(|e| format!("Serialize error: {e}"))?;

        let client = reqwest::Client::new();
        let response = client.post(endpoint)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await
            .map_err(|e| format!("HTTP error: {e}"))?;

        if response.status().is_success() {
            debug!(endpoint, points = self.points.len(), "Metrics exported");
            Ok(())
        } else {
            Err(format!("HTTP {}", response.status()))
        }
    }

    /// Clear collected data points (keep counters).
    pub fn clear_points(&mut self) {
        self.points.clear();
    }
}

#[derive(Debug, Serialize)]
pub struct MetricsSummary {
    pub session_id: String,
    pub total_api_calls: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub total_tool_executions: u64,
    pub data_points: usize,
}

#[derive(Debug, Serialize)]
pub struct ExportPayload {
    pub session_id: String,
    pub agent_version: String,
    pub resource_attributes: HashMap<String, String>,
    pub metrics: Vec<MetricPoint>,
}
