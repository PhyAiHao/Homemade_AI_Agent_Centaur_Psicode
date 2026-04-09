//! Token budget management — context window budget tracking and continuation logic.
//!
//! Mirrors `src/query/tokenBudget.ts`.
#![allow(dead_code)]

/// Budget decision from `check_budget()`.
#[derive(Debug, Clone, PartialEq)]
pub enum BudgetDecision {
    /// Continue execution — budget has room.
    Continue {
        /// Percentage of budget used (0.0–1.0).
        usage_pct: f64,
        /// Optional nudge message to inject.
        nudge: Option<String>,
    },
    /// Stop execution — budget exhausted or diminishing returns.
    Stop {
        reason: String,
    },
}

/// Tracks continuation state for the token budget system.
#[derive(Debug, Clone)]
pub struct BudgetTracker {
    /// How many times we've continued past the initial response.
    pub continuation_count: u32,
    /// Tokens produced in the last iteration.
    pub last_delta_tokens: u64,
    /// Global turn tokens at last check.
    pub last_global_turn_tokens: u64,
    /// Max allowed tokens per session.
    pub max_tokens: u64,
    /// Current cumulative tokens used.
    pub tokens_used: u64,
}

impl BudgetTracker {
    pub fn new(max_tokens: u64) -> Self {
        BudgetTracker {
            continuation_count: 0,
            last_delta_tokens: 0,
            last_global_turn_tokens: 0,
            max_tokens,
            tokens_used: 0,
        }
    }

    /// Record tokens from the latest turn.
    pub fn record_turn(&mut self, turn_tokens: u64) {
        let delta = turn_tokens.saturating_sub(self.last_global_turn_tokens);
        self.last_delta_tokens = delta;
        self.last_global_turn_tokens = turn_tokens;
        self.tokens_used = turn_tokens;
        self.continuation_count += 1;
    }

    /// Check if the budget allows continuation.
    pub fn check(&self) -> BudgetDecision {
        let usage_pct = self.tokens_used as f64 / self.max_tokens as f64;

        // Over 90% — stop
        if usage_pct >= 0.90 {
            return BudgetDecision::Stop {
                reason: format!(
                    "Token budget exhausted: {:.0}% used ({}/{})",
                    usage_pct * 100.0,
                    self.tokens_used,
                    self.max_tokens
                ),
            };
        }

        // Diminishing returns: 3+ continuations with < 500 tokens each
        if self.continuation_count >= 3 && self.last_delta_tokens < 500 {
            return BudgetDecision::Stop {
                reason: format!(
                    "Diminishing returns: {} continuations, last produced only {} tokens",
                    self.continuation_count, self.last_delta_tokens
                ),
            };
        }

        // Budget has room — continue with optional nudge
        let nudge = if usage_pct > 0.50 {
            Some(format!(
                "Continue your work: {:.0}% of budget used ({}/{} tokens)",
                usage_pct * 100.0,
                self.tokens_used,
                self.max_tokens
            ))
        } else {
            None
        };

        BudgetDecision::Continue { usage_pct, nudge }
    }

    /// Usage percentage (0.0–1.0).
    pub fn usage_pct(&self) -> f64 {
        self.tokens_used as f64 / self.max_tokens as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fresh_budget_continues() {
        let tracker = BudgetTracker::new(200_000);
        match tracker.check() {
            BudgetDecision::Continue { nudge, .. } => assert!(nudge.is_none()),
            _ => panic!("Expected continue"),
        }
    }

    #[test]
    fn test_over_90_stops() {
        let mut tracker = BudgetTracker::new(100_000);
        tracker.tokens_used = 91_000;
        match tracker.check() {
            BudgetDecision::Stop { reason } => assert!(reason.contains("exhausted")),
            _ => panic!("Expected stop"),
        }
    }

    #[test]
    fn test_diminishing_returns_stops() {
        let mut tracker = BudgetTracker::new(200_000);
        tracker.continuation_count = 4;
        tracker.last_delta_tokens = 100;
        tracker.tokens_used = 50_000;
        match tracker.check() {
            BudgetDecision::Stop { reason } => assert!(reason.contains("Diminishing")),
            _ => panic!("Expected stop"),
        }
    }

    #[test]
    fn test_half_budget_nudge() {
        let mut tracker = BudgetTracker::new(100_000);
        tracker.tokens_used = 60_000;
        tracker.continuation_count = 1;
        tracker.last_delta_tokens = 5000;
        match tracker.check() {
            BudgetDecision::Continue { nudge, .. } => assert!(nudge.is_some()),
            _ => panic!("Expected continue with nudge"),
        }
    }
}
