//! Advanced prompt caching — mirrors `src/services/api/promptCacheBreakDetection.ts`
//! and cache_control placement from `src/services/api/claude.ts`.
//!
//! Features:
//! - Ephemeral cache_control markers on system prompt + last message
//! - 1h TTL for eligible users (latched at session start)
//! - Global scope for long-context models
//! - Cache break detection with cause classification
#![allow(dead_code)]

use serde_json::{json, Value};
use std::sync::Mutex;
use tracing::debug;

// ─── Constants ──────────────────────────────────────────────────────────────

/// Minimum cache read token drop to consider a cache break.
const MIN_CACHE_MISS_TOKENS: u64 = 2_000;
/// Drop percentage threshold (5%).
const CACHE_BREAK_PCT: f64 = 0.95;
/// 5-minute TTL in ms.
const TTL_5MIN_MS: u64 = 5 * 60 * 1000;
/// 1-hour TTL in ms.
const TTL_1HOUR_MS: u64 = 60 * 60 * 1000;

// ─── Cache control builder ──────────────────────────────────────────────────

/// Session-level prompt caching configuration.
#[derive(Debug, Clone)]
pub struct PromptCacheConfig {
    /// Whether prompt caching is enabled.
    pub enabled: bool,
    /// Whether 1h TTL is available (latched at session start).
    pub use_1h_ttl: bool,
    /// Whether to use global cache scope.
    pub global_scope: bool,
}

impl Default for PromptCacheConfig {
    fn default() -> Self {
        PromptCacheConfig {
            enabled: true,
            use_1h_ttl: false,
            global_scope: false,
        }
    }
}

/// Build a cache_control value based on config.
pub fn build_cache_control(config: &PromptCacheConfig) -> Value {
    let mut cc = json!({ "type": "ephemeral" });
    if config.use_1h_ttl {
        cc["ttl"] = json!("1h");
    }
    if config.global_scope {
        cc["scope"] = json!("global");
    }
    cc
}

/// Build system prompt blocks with cache_control on the last block.
pub fn build_system_prompt_blocks(
    system_prompt: &str,
    config: &PromptCacheConfig,
) -> Value {
    if !config.enabled {
        return Value::String(system_prompt.to_string());
    }
    let cc = build_cache_control(config);
    json!([{
        "type": "text",
        "text": system_prompt,
        "cache_control": cc,
    }])
}

/// Add cache_control to the last content block of the last message.
/// Skips thinking/redacted_thinking blocks (marker goes on last non-thinking block).
///
/// Only ONE message-level cache_control marker per request.
pub fn add_message_cache_markers(
    messages: &mut [Value],
    config: &PromptCacheConfig,
) {
    if !config.enabled || messages.is_empty() {
        return;
    }

    let cc = build_cache_control(config);

    if let Some(last_msg) = messages.last_mut() {
        if let Some(content) = last_msg.get_mut("content") {
            match content {
                Value::Array(blocks) if !blocks.is_empty() => {
                    // Find last non-thinking block
                    for i in (0..blocks.len()).rev() {
                        let block_type = blocks[i].get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("");
                        if block_type != "thinking" && block_type != "redacted_thinking" {
                            if let Value::Object(ref mut map) = blocks[i] {
                                map.insert("cache_control".to_string(), cc);
                            }
                            return;
                        }
                    }
                }
                Value::String(_) => {
                    let text = content.take();
                    *content = json!([{
                        "type": "text",
                        "text": text,
                        "cache_control": cc,
                    }]);
                }
                _ => {}
            }
        }
    }
}

// ─── Cache break detection ──────────────────────────────────────────────────

/// Cause of a detected cache break.
#[derive(Debug, Clone)]
pub enum CacheBreakCause {
    ModelChanged { from: String, to: String },
    SystemPromptChanged,
    ToolsChanged,
    FastModeToggled,
    PossibleTtlExpiry { ttl: &'static str },
    LikelyServerSide,
    Unknown,
}

impl std::fmt::Display for CacheBreakCause {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CacheBreakCause::ModelChanged { from, to } => write!(f, "Model changed: {from} → {to}"),
            CacheBreakCause::SystemPromptChanged => write!(f, "System prompt changed"),
            CacheBreakCause::ToolsChanged => write!(f, "Tool definitions changed"),
            CacheBreakCause::FastModeToggled => write!(f, "Fast mode toggled"),
            CacheBreakCause::PossibleTtlExpiry { ttl } => write!(f, "Possible {ttl} TTL expiry"),
            CacheBreakCause::LikelyServerSide => write!(f, "Likely server-side cache eviction"),
            CacheBreakCause::Unknown => write!(f, "Unknown cause"),
        }
    }
}

/// Snapshot of prompt state for cache break detection.
#[derive(Debug, Clone)]
pub struct PromptStateSnapshot {
    pub system_hash: u64,
    pub tools_hash: u64,
    pub model: String,
    pub fast_mode: bool,
    pub timestamp_ms: u64,
}

impl PromptStateSnapshot {
    pub fn new(system_prompt: &str, tools_json: &[Value], model: &str, fast_mode: bool) -> Self {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;

        let mut h = DefaultHasher::new();
        system_prompt.hash(&mut h);
        let system_hash = h.finish();

        let mut h2 = DefaultHasher::new();
        for t in tools_json {
            t.to_string().hash(&mut h2);
        }
        let tools_hash = h2.finish();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        PromptStateSnapshot {
            system_hash,
            tools_hash,
            model: model.to_string(),
            fast_mode,
            timestamp_ms: now,
        }
    }
}

/// Cache break detector — tracks state across API calls.
pub struct CacheBreakDetector {
    prev_state: Mutex<Option<PromptStateSnapshot>>,
    prev_cache_read: Mutex<Option<u64>>,
}

impl CacheBreakDetector {
    pub fn new() -> Self {
        CacheBreakDetector {
            prev_state: Mutex::new(None),
            prev_cache_read: Mutex::new(None),
        }
    }

    /// Record the prompt state before an API call.
    pub fn record_state(&self, snapshot: PromptStateSnapshot) {
        *self.prev_state.lock().unwrap() = Some(snapshot);
    }

    /// Check for cache break after receiving response.
    /// Returns the detected cause if a break occurred.
    pub fn check_response(
        &self,
        cache_read_tokens: u64,
        _cache_creation_tokens: u64,
        current_state: &PromptStateSnapshot,
    ) -> Option<CacheBreakCause> {
        let prev_read = {
            let mut prev = self.prev_cache_read.lock().unwrap();
            let old = *prev;
            *prev = Some(cache_read_tokens);
            old
        };

        let prev_state = self.prev_state.lock().unwrap().clone();

        // No baseline yet — skip
        let prev_read = prev_read?;
        if prev_read == 0 {
            return None;
        }

        // Check for cache break: read tokens dropped significantly
        let drop = prev_read.saturating_sub(cache_read_tokens);
        if cache_read_tokens as f64 >= prev_read as f64 * CACHE_BREAK_PCT || drop < MIN_CACHE_MISS_TOKENS {
            return None; // No break detected
        }

        debug!(
            prev_read,
            cache_read_tokens,
            drop,
            "Cache break detected"
        );

        // Classify cause
        if let Some(ref prev) = prev_state {
            if prev.model != current_state.model {
                return Some(CacheBreakCause::ModelChanged {
                    from: prev.model.clone(),
                    to: current_state.model.clone(),
                });
            }
            if prev.system_hash != current_state.system_hash {
                return Some(CacheBreakCause::SystemPromptChanged);
            }
            if prev.tools_hash != current_state.tools_hash {
                return Some(CacheBreakCause::ToolsChanged);
            }
            if prev.fast_mode != current_state.fast_mode {
                return Some(CacheBreakCause::FastModeToggled);
            }
            // Time-based TTL expiry
            let gap_ms = current_state.timestamp_ms.saturating_sub(prev.timestamp_ms);
            if gap_ms > TTL_1HOUR_MS {
                return Some(CacheBreakCause::PossibleTtlExpiry { ttl: "1h" });
            }
            if gap_ms > TTL_5MIN_MS {
                return Some(CacheBreakCause::PossibleTtlExpiry { ttl: "5min" });
            }
            return Some(CacheBreakCause::LikelyServerSide);
        }

        Some(CacheBreakCause::Unknown)
    }

    /// Reset after compaction (baseline is invalidated).
    pub fn reset(&self) {
        *self.prev_state.lock().unwrap() = None;
        *self.prev_cache_read.lock().unwrap() = None;
    }
}

impl Default for CacheBreakDetector {
    fn default() -> Self {
        Self::new()
    }
}
