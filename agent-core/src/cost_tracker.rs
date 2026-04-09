//! Cost tracker — token counting and USD cost tracking per model.
//!
//! Mirrors `src/cost-tracker.ts`.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Instant;

/// Per-model usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_input_tokens: u64,
    pub cache_creation_input_tokens: u64,
    pub web_search_requests: u64,
    pub cost_usd: f64,
}

/// Raw usage data from a single API response.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ApiUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    #[serde(default)]
    pub web_search_requests: u64,
}

/// Pricing table entry (per million tokens).
#[derive(Debug, Clone)]
struct ModelPricing {
    input_per_m: f64,
    output_per_m: f64,
    cache_read_per_m: f64,
    cache_write_per_m: f64,
}

/// Per-model pricing lookup.
fn get_pricing(model: &str) -> ModelPricing {
    // Canonical pricing as of 2026
    if model.contains("opus") {
        ModelPricing {
            input_per_m: 15.0,
            output_per_m: 75.0,
            cache_read_per_m: 1.5,
            cache_write_per_m: 18.75,
        }
    } else if model.contains("haiku") {
        ModelPricing {
            input_per_m: 0.80,
            output_per_m: 4.0,
            cache_read_per_m: 0.08,
            cache_write_per_m: 1.0,
        }
    } else {
        // Sonnet (default)
        ModelPricing {
            input_per_m: 3.0,
            output_per_m: 15.0,
            cache_read_per_m: 0.30,
            cache_write_per_m: 3.75,
        }
    }
}

/// Calculate USD cost for a usage snapshot.
fn calculate_cost(model: &str, usage: &ApiUsage) -> f64 {
    let p = get_pricing(model);
    let m = 1_000_000.0;

    (usage.input_tokens as f64 / m) * p.input_per_m
        + (usage.output_tokens as f64 / m) * p.output_per_m
        + (usage.cache_read_input_tokens as f64 / m) * p.cache_read_per_m
        + (usage.cache_creation_input_tokens as f64 / m) * p.cache_write_per_m
}

/// Tracks cumulative usage and cost across all API calls in a session.
#[derive(Debug)]
pub struct CostTracker {
    /// Per-model usage accumulation.
    model_usage: HashMap<String, ModelUsage>,
    /// Session start time.
    start_time: Instant,
    /// Total API call duration (summed, not wall-clock).
    total_api_duration_ms: u64,
    /// Lines of code changed (for session summary).
    lines_added: u64,
    lines_removed: u64,
}

impl CostTracker {
    pub fn new() -> Self {
        CostTracker {
            model_usage: HashMap::new(),
            start_time: Instant::now(),
            total_api_duration_ms: 0,
            lines_added: 0,
            lines_removed: 0,
        }
    }

    /// Record usage from a single API response.
    pub fn record(&mut self, model: &str, usage: ApiUsage, api_duration_ms: u64) {
        let cost = calculate_cost(model, &usage);
        let entry = self.model_usage
            .entry(model.to_string())
            .or_default();

        entry.input_tokens += usage.input_tokens;
        entry.output_tokens += usage.output_tokens;
        entry.cache_read_input_tokens += usage.cache_read_input_tokens;
        entry.cache_creation_input_tokens += usage.cache_creation_input_tokens;
        entry.web_search_requests += usage.web_search_requests;
        entry.cost_usd += cost;

        self.total_api_duration_ms += api_duration_ms;
    }

    /// Record code changes.
    pub fn record_code_changes(&mut self, added: u64, removed: u64) {
        self.lines_added += added;
        self.lines_removed += removed;
    }

    // ─── Accessors ──────────────────────────────────────────────────────

    pub fn total_cost(&self) -> f64 {
        self.model_usage.values().map(|u| u.cost_usd).sum()
    }

    pub fn total_input_tokens(&self) -> u64 {
        self.model_usage.values().map(|u| u.input_tokens).sum()
    }

    pub fn total_output_tokens(&self) -> u64 {
        self.model_usage.values().map(|u| u.output_tokens).sum()
    }

    pub fn total_cache_read_tokens(&self) -> u64 {
        self.model_usage.values().map(|u| u.cache_read_input_tokens).sum()
    }

    pub fn total_cache_creation_tokens(&self) -> u64 {
        self.model_usage.values().map(|u| u.cache_creation_input_tokens).sum()
    }

    pub fn total_web_searches(&self) -> u64 {
        self.model_usage.values().map(|u| u.web_search_requests).sum()
    }

    pub fn total_duration_ms(&self) -> u128 {
        self.start_time.elapsed().as_millis()
    }

    pub fn total_api_duration_ms(&self) -> u64 {
        self.total_api_duration_ms
    }

    pub fn lines_added(&self) -> u64 {
        self.lines_added
    }

    pub fn lines_removed(&self) -> u64 {
        self.lines_removed
    }

    pub fn model_usage(&self) -> &HashMap<String, ModelUsage> {
        &self.model_usage
    }

    /// Check if the cumulative cost exceeds a USD budget.
    pub fn exceeds_budget(&self, max_usd: f64) -> bool {
        self.total_cost() >= max_usd
    }

    /// Format a human-readable summary for the session.
    pub fn summary(&self) -> String {
        let cost = self.total_cost();
        let input = self.total_input_tokens();
        let output = self.total_output_tokens();
        let cache_r = self.total_cache_read_tokens();
        let dur = self.total_duration_ms() / 1000;

        format!(
            "Cost: ${cost:.4} | Tokens: {input} in / {output} out \
             ({cache_r} cache read) | Duration: {dur}s | \
             Code: +{} -{} lines",
            self.lines_added, self.lines_removed
        )
    }
}

impl Default for CostTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cost_calculation() {
        let usage = ApiUsage {
            input_tokens: 1_000_000,
            output_tokens: 100_000,
            cache_read_input_tokens: 500_000,
            ..Default::default()
        };
        let cost = calculate_cost("claude-sonnet-4-6", &usage);
        // $3.00 input + $1.50 output + $0.15 cache read = $4.65
        assert!((cost - 4.65).abs() < 0.01);
    }

    #[test]
    fn test_tracker_accumulation() {
        let mut tracker = CostTracker::new();
        tracker.record("claude-sonnet-4-6", ApiUsage {
            input_tokens: 100,
            output_tokens: 50,
            ..Default::default()
        }, 500);
        tracker.record("claude-sonnet-4-6", ApiUsage {
            input_tokens: 200,
            output_tokens: 100,
            ..Default::default()
        }, 300);

        assert_eq!(tracker.total_input_tokens(), 300);
        assert_eq!(tracker.total_output_tokens(), 150);
        assert_eq!(tracker.total_api_duration_ms(), 800);
    }

    #[test]
    fn test_budget_check() {
        let mut tracker = CostTracker::new();
        tracker.record("claude-sonnet-4-6", ApiUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
            ..Default::default()
        }, 1000);
        // $3 + $15 = $18
        assert!(tracker.exceeds_budget(10.0));
        assert!(!tracker.exceeds_budget(20.0));
    }
}
