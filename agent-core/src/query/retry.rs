//! Retry logic — mirrors `src/services/api/withRetry.ts`.
//!
//! Provides exponential backoff, error classification, context overflow
//! recovery, fast mode cooldown, and persistent retry for unattended sessions.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

// ─── Constants (matching original) ──────────────────────────────────────────

pub const DEFAULT_MAX_RETRIES: u32 = 10;
pub const BASE_DELAY_MS: u64 = 500;
pub const MAX_DELAY_MS: u64 = 32_000;
pub const MAX_529_BEFORE_FALLBACK: u32 = 3;
/// Floor for max_tokens when recovering from context overflow.
pub const FLOOR_OUTPUT_TOKENS: u32 = 3_000;
/// Max output tokens recovery limit.
pub const MAX_OUTPUT_TOKENS_RECOVERY_LIMIT: u32 = 3;
/// Escalated max tokens for recovery.
pub const ESCALATED_MAX_TOKENS: u32 = 64_000;

// ── Fast mode cooldown constants ─────────────────────────────────────────

pub const SHORT_RETRY_THRESHOLD_MS: u64 = 20_000;
pub const MIN_COOLDOWN_MS: u64 = 10 * 60 * 1000;       // 10 min
pub const DEFAULT_COOLDOWN_MS: u64 = 30 * 60 * 1000;    // 30 min

// ── Persistent retry (unattended) ────────────────────────────────────────

pub const PERSISTENT_MAX_BACKOFF_MS: u64 = 5 * 60 * 1000;   // 5 min
pub const PERSISTENT_RESET_CAP_MS: u64 = 6 * 60 * 60 * 1000; // 6 hours

// ─── Error classification ───────────────────────────────────────────────────

/// Classify an error as retryable, non-retryable, or a specific recovery action.
#[derive(Debug, Clone, PartialEq)]
pub enum RetryDecision {
    /// Retry with backoff.
    Retry,
    /// Don't retry — terminal error.
    Fatal,
    /// Context overflow — adjust max_tokens and retry.
    ContextOverflow {
        input_tokens: u64,
        context_limit: u64,
    },
    /// Server overloaded (529) — may trigger fallback.
    Overloaded,
    /// Rate limited (429).
    RateLimited {
        retry_after_ms: Option<u64>,
    },
}

/// Classify an error string into a retry decision.
pub fn classify_error(error_msg: &str, stop_reason: &Option<String>) -> RetryDecision {
    let combined = format!(
        "{}{}",
        error_msg,
        stop_reason.as_deref().unwrap_or("")
    ).to_lowercase();

    // Context overflow — recoverable by reducing max_tokens
    if let Some(parsed) = parse_context_overflow(&combined) {
        return RetryDecision::ContextOverflow {
            input_tokens: parsed.0,
            context_limit: parsed.1,
        };
    }

    // Prompt too long
    if combined.contains("prompt_too_long") || combined.contains("prompt is too long") {
        return RetryDecision::ContextOverflow {
            input_tokens: 0,
            context_limit: 0,
        };
    }

    // 529 overloaded
    if combined.contains("overloaded") || combined.contains("529") {
        return RetryDecision::Overloaded;
    }

    // 429 rate limit
    if combined.contains("rate_limit") || combined.contains("429") {
        // Try to extract retry-after
        let retry_after = extract_retry_after(&combined);
        return RetryDecision::RateLimited {
            retry_after_ms: retry_after,
        };
    }

    // Retryable server errors
    if combined.contains("timeout") || combined.contains("408")
        || combined.contains("500") || combined.contains("502")
        || combined.contains("503") || combined.contains("server_error")
        || combined.contains("connection") || combined.contains("econnreset")
        || combined.contains("epipe")
    {
        return RetryDecision::Retry;
    }

    // 401 unauthorized — might be stale token
    if combined.contains("401") || combined.contains("unauthorized") {
        return RetryDecision::Retry;
    }

    // Everything else is fatal
    RetryDecision::Fatal
}

/// Parse context overflow error: "input length and `max_tokens` exceed context limit: X + Y > Z"
fn parse_context_overflow(msg: &str) -> Option<(u64, u64)> {
    // Pattern: "exceed context limit: <input> + <max_tokens> > <limit>"
    let marker = "exceed context limit:";
    let idx = msg.find(marker)?;
    let rest = &msg[idx + marker.len()..];
    let parts: Vec<&str> = rest.split_whitespace().collect();
    if parts.len() >= 5 {
        let input = parts[0].parse::<u64>().ok()?;
        // parts[1] is "+"
        // parts[2] is max_tokens
        // parts[3] is ">"
        let limit = parts[4].parse::<u64>().ok()?;
        Some((input, limit))
    } else {
        None
    }
}

/// Extract retry-after value from error message (seconds → ms).
fn extract_retry_after(msg: &str) -> Option<u64> {
    // Look for "retry-after: X" or "retry_after: X"
    for prefix in &["retry-after:", "retry_after:", "retry-after="] {
        if let Some(idx) = msg.find(prefix) {
            let rest = &msg[idx + prefix.len()..];
            let num_str: String = rest.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(seconds) = num_str.parse::<u64>() {
                return Some(seconds * 1000);
            }
        }
    }
    None
}

/// Compute retry delay with exponential backoff + jitter.
pub fn compute_delay(attempt: u32, persistent: bool) -> Duration {
    let max = if persistent { PERSISTENT_MAX_BACKOFF_MS } else { MAX_DELAY_MS };
    let base = BASE_DELAY_MS.saturating_mul(2u64.pow(attempt.saturating_sub(1)));
    let capped = base.min(max);
    let jitter = (capped as f64 * 0.25 * pseudo_random()) as u64;
    Duration::from_millis(capped + jitter)
}

/// Compute max_tokens override for context overflow recovery.
pub fn compute_overflow_recovery_max_tokens(
    input_tokens: u64,
    context_limit: u64,
    thinking_budget: Option<u32>,
) -> u32 {
    let headroom = context_limit.saturating_sub(input_tokens).saturating_sub(1000);
    let base = headroom.max(FLOOR_OUTPUT_TOKENS as u64) as u32;
    if let Some(budget) = thinking_budget {
        base.max(budget + 1)
    } else {
        base
    }
}

fn pseudo_random() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos % 1000) as f64 / 1000.0
}

// ─── Fast mode cooldown ─────────────────────────────────────────────────────

/// Runtime state for fast mode.
pub struct FastModeState {
    active: AtomicBool,
    cooldown_until_ms: AtomicU64,
}

impl FastModeState {
    pub fn new(active: bool) -> Self {
        FastModeState {
            active: AtomicBool::new(active),
            cooldown_until_ms: AtomicU64::new(0),
        }
    }

    pub fn is_active(&self) -> bool {
        if !self.active.load(Ordering::Relaxed) {
            return false;
        }
        // Check if cooldown expired
        let until = self.cooldown_until_ms.load(Ordering::Relaxed);
        if until > 0 {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            if now < until {
                return false; // still in cooldown
            }
            // Cooldown expired — reactivate
            self.cooldown_until_ms.store(0, Ordering::Relaxed);
        }
        true
    }

    pub fn enter_cooldown(&self, duration_ms: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.cooldown_until_ms.store(now + duration_ms, Ordering::Relaxed);
    }

    pub fn disable_permanently(&self) {
        self.active.store(false, Ordering::Relaxed);
    }
}

impl Default for FastModeState {
    fn default() -> Self {
        Self::new(false)
    }
}

// ─── Retry context (mutable state across retries) ───────────────────────────

/// Mutable state carried across retry attempts within a single API call.
#[derive(Debug, Clone)]
pub struct RetryContext {
    /// Override max_tokens (set on context overflow recovery).
    pub max_tokens_override: Option<u32>,
    /// Whether fast mode is active for this request.
    pub fast_mode: bool,
    /// Consecutive 529 errors.
    pub consecutive_529s: u32,
    /// Current attempt number.
    pub attempt: u32,
}

impl RetryContext {
    pub fn new(fast_mode: bool) -> Self {
        RetryContext {
            max_tokens_override: None,
            fast_mode,
            consecutive_529s: 0,
            attempt: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_overloaded() {
        assert_eq!(
            classify_error("server overloaded", &None),
            RetryDecision::Overloaded
        );
    }

    #[test]
    fn test_classify_context_overflow() {
        let msg = "input length and `max_tokens` exceed context limit: 150000 + 16384 > 200000";
        match classify_error(msg, &None) {
            RetryDecision::ContextOverflow { input_tokens, context_limit } => {
                assert_eq!(input_tokens, 150000);
                assert_eq!(context_limit, 200000);
            }
            other => panic!("Expected ContextOverflow, got {:?}", other),
        }
    }

    #[test]
    fn test_classify_fatal() {
        assert_eq!(
            classify_error("invalid api key", &None),
            RetryDecision::Fatal
        );
    }

    #[test]
    fn test_overflow_recovery() {
        let max = compute_overflow_recovery_max_tokens(150_000, 200_000, None);
        assert_eq!(max, 49_000); // 200000 - 150000 - 1000
    }

    #[test]
    fn test_fast_mode_cooldown() {
        let state = FastModeState::new(true);
        assert!(state.is_active());
        state.enter_cooldown(1_000_000); // 1000 seconds
        assert!(!state.is_active());
    }

    #[test]
    fn test_delay_increases() {
        let d1 = compute_delay(1, false);
        let d2 = compute_delay(3, false);
        assert!(d2 > d1);
    }
}
