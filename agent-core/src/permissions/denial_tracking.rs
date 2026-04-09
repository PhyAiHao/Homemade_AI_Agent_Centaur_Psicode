//! Denial tracking — mirrors `src/utils/permissions/denialTracking.ts`.
//!
//! Tracks consecutive and total permission denials. When limits are
//! reached, falls back to user prompting (interactive) or aborts (headless).
#![allow(dead_code)]

/// Denial limits matching the original.
pub const MAX_CONSECUTIVE_DENIALS: u32 = 3;
pub const MAX_TOTAL_DENIALS: u32 = 20;

/// Mutable denial tracking state.
#[derive(Debug, Clone, Default)]
pub struct DenialTrackingState {
    pub consecutive_denials: u32,
    pub total_denials: u32,
}

impl DenialTrackingState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a permission denial.
    pub fn record_denial(&mut self) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
    }

    /// Record a permission approval (resets consecutive counter).
    pub fn record_success(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Check if denial limits have been reached and we should fall back
    /// to prompting instead of auto-denying.
    pub fn should_fallback_to_prompting(&self) -> bool {
        self.consecutive_denials >= MAX_CONSECUTIVE_DENIALS
            || self.total_denials >= MAX_TOTAL_DENIALS
    }

    /// Reset both counters (called after total limit fallback).
    pub fn reset(&mut self) {
        self.consecutive_denials = 0;
        self.total_denials = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consecutive_limit() {
        let mut state = DenialTrackingState::new();
        state.record_denial();
        state.record_denial();
        assert!(!state.should_fallback_to_prompting());
        state.record_denial();
        assert!(state.should_fallback_to_prompting());
    }

    #[test]
    fn test_success_resets_consecutive() {
        let mut state = DenialTrackingState::new();
        state.record_denial();
        state.record_denial();
        state.record_success();
        assert_eq!(state.consecutive_denials, 0);
        assert_eq!(state.total_denials, 2);
    }

    #[test]
    fn test_total_limit() {
        let mut state = DenialTrackingState::new();
        for _ in 0..20 {
            state.record_denial();
            state.record_success(); // reset consecutive each time
        }
        assert!(state.should_fallback_to_prompting()); // total = 20
    }
}
