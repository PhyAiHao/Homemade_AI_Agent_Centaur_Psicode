//! Activity manager — tracks user and CLI activity states.
//!
//! Mirrors `src/utils/activityManager.ts`. Provides deduplicating
//! tracking of user interactions and CLI operations.
#![allow(dead_code)]

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Instant;

/// Activity state snapshot.
#[derive(Debug, Clone)]
pub struct ActivityState {
    pub is_user_active: bool,
    pub is_cli_active: bool,
    pub active_operation_count: usize,
}

/// Tracks user and CLI activity for analytics and idle detection.
pub struct ActivityManager {
    inner: Mutex<ActivityInner>,
}

struct ActivityInner {
    /// Last time user interacted.
    last_user_activity: Option<Instant>,
    /// Currently active CLI operations (deduplicating set).
    active_operations: HashSet<String>,
    /// User activity dedup timeout (5 seconds).
    user_dedup_timeout: std::time::Duration,
}

impl ActivityManager {
    pub fn new() -> Self {
        ActivityManager {
            inner: Mutex::new(ActivityInner {
                last_user_activity: None,
                active_operations: HashSet::new(),
                user_dedup_timeout: std::time::Duration::from_secs(5),
            }),
        }
    }

    /// Record a user interaction (keystroke, mouse click, etc.).
    pub fn record_user_activity(&self) {
        let mut inner = self.inner.lock().unwrap();
        let now = Instant::now();

        // Dedup: only count if > 5s since last record
        if let Some(last) = inner.last_user_activity {
            if now.duration_since(last) < inner.user_dedup_timeout {
                return;
            }
        }

        inner.last_user_activity = Some(now);
    }

    /// Start tracking a CLI operation.
    pub fn start_cli_activity(&self, operation_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_operations.insert(operation_id.to_string());
    }

    /// Stop tracking a CLI operation.
    pub fn end_cli_activity(&self, operation_id: &str) {
        let mut inner = self.inner.lock().unwrap();
        inner.active_operations.remove(operation_id);
    }

    /// Run a tracked async operation.
    pub async fn track_operation<F, T>(&self, operation_id: &str, f: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        self.start_cli_activity(operation_id);
        let result = f.await;
        self.end_cli_activity(operation_id);
        result
    }

    /// Get current activity states.
    pub fn get_states(&self) -> ActivityState {
        let inner = self.inner.lock().unwrap();

        let is_user_active = inner.last_user_activity
            .map(|t| t.elapsed() < std::time::Duration::from_secs(30))
            .unwrap_or(false);

        ActivityState {
            is_user_active,
            is_cli_active: !inner.active_operations.is_empty(),
            active_operation_count: inner.active_operations.len(),
        }
    }

    /// Check if the CLI is idle (no active operations, no recent user activity).
    pub fn is_idle(&self) -> bool {
        let states = self.get_states();
        !states.is_user_active && !states.is_cli_active
    }
}

impl Default for ActivityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initial_state() {
        let mgr = ActivityManager::new();
        let state = mgr.get_states();
        assert!(!state.is_user_active);
        assert!(!state.is_cli_active);
        assert_eq!(state.active_operation_count, 0);
    }

    #[test]
    fn test_cli_activity() {
        let mgr = ActivityManager::new();
        mgr.start_cli_activity("tool_exec_1");
        assert!(mgr.get_states().is_cli_active);
        assert_eq!(mgr.get_states().active_operation_count, 1);

        mgr.start_cli_activity("tool_exec_2");
        assert_eq!(mgr.get_states().active_operation_count, 2);

        mgr.end_cli_activity("tool_exec_1");
        assert_eq!(mgr.get_states().active_operation_count, 1);

        mgr.end_cli_activity("tool_exec_2");
        assert!(!mgr.get_states().is_cli_active);
    }

    #[test]
    fn test_user_activity() {
        let mgr = ActivityManager::new();
        mgr.record_user_activity();
        assert!(mgr.get_states().is_user_active);
    }

    #[test]
    fn test_idle() {
        let mgr = ActivityManager::new();
        assert!(mgr.is_idle());

        mgr.start_cli_activity("op1");
        assert!(!mgr.is_idle());
    }
}
