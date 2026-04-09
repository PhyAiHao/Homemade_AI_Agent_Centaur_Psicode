//! Dream configuration — gate thresholds and feature flag.

/// Configuration for the auto-dream system.
#[derive(Debug, Clone)]
pub struct DreamConfig {
    /// Whether auto-dream is enabled.
    pub enabled: bool,
    /// Minimum hours since last consolidation before a new dream can run.
    pub min_hours: u32,
    /// Minimum new sessions accumulated before a dream triggers.
    pub min_sessions: u32,
}

impl Default for DreamConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_hours: 12,
            min_sessions: 5,
        }
    }
}
