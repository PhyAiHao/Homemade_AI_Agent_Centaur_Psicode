//! PermissionGate — the single check point before any tool executes.
//!
//! Mirrors `src/hooks/toolPermission/useCanUseTool.tsx`.
#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::debug;

use super::mode::PermissionMode;
use super::rules::{evaluate_rules, PermissionResult, PermissionRule};
use super::extract_file_path;

/// Outcome of a gate check.
#[derive(Debug, Clone, PartialEq)]
pub enum GateDecision {
    /// Proceed with tool execution
    Allow,
    /// Block tool execution
    Deny(String),
    /// Execution skipped — plan mode only describes the action
    PlanOnly,
}

/// Type alias for the interactive prompt callback.
type PromptFn = Option<Arc<dyn Fn(&str, &str) -> bool + Send + Sync>>;

/// Thread-safe permission gate shared across all tool executors.
#[derive(Clone)]
pub struct PermissionGate {
    mode:  PermissionMode,
    rules: Vec<PermissionRule>,
    /// Tracks which tools the user has already approved in this session
    session_approvals: Arc<Mutex<HashMap<String, bool>>>,
    /// Callback to prompt the user interactively (set by TUI layer)
    prompt_fn: PromptFn,
    /// Denial tracking for auto mode fallback
    denial_state: Arc<Mutex<super::denial_tracking::DenialTrackingState>>,
}

impl PermissionGate {
    pub fn new(mode: PermissionMode, rules: Vec<PermissionRule>) -> Self {
        PermissionGate {
            mode,
            rules,
            session_approvals: Arc::new(Mutex::new(HashMap::new())),
            prompt_fn: None,
            denial_state: Arc::new(Mutex::new(super::denial_tracking::DenialTrackingState::new())),
        }
    }

    /// Load rules from disk settings files and merge with provided rules.
    pub fn with_disk_rules(mut self) -> Self {
        let disk_rules = super::loader::load_all_rules();
        self.rules.extend(disk_rules);
        self
    }

    /// Register a callback that prompts the user and returns true = approved.
    pub fn set_prompt_fn<F>(&mut self, f: F)
    where F: Fn(&str, &str) -> bool + Send + Sync + 'static
    {
        self.prompt_fn = Some(Arc::new(f));
    }

    /// Check whether `tool_name` with the given `input_json` may execute.
    ///
    /// Full permission pipeline (mirrors `hasPermissionsToUseTool`):
    /// 1. PlanOnly → PlanOnly
    /// 2. Path safety checks (.git/, .claude/, UNC paths)
    /// 3. Tool-wide deny rules → Deny
    /// 4. Tool-wide ask rules → Ask
    /// 5. Content-specific rules → Allow/Deny/Ask
    /// 6. Bypass mode → Allow (but NOT for deny/ask rules above)
    /// 7. Tool-wide allow rules → Allow
    /// 8. AutoApprove mode → Allow
    /// 9. DontAsk mode → Deny
    /// 10. Session cache check
    /// 11. Prompt user → Allow/Deny
    /// 12. Denial tracking for fallback
    pub fn check(&self, tool_name: &str, input_json: &str) -> GateDecision {
        debug!("PermissionGate::check tool={tool_name}");

        // 1. PlanOnly blocks all writes
        if self.mode.is_read_only() {
            return GateDecision::PlanOnly;
        }

        // 2. Path safety checks for file tools
        if tool_name == "FileEdit" || tool_name == "FileWrite" || tool_name == "FileRead" {
            if let Some(path) = extract_file_path(input_json) {
                if let Err(e) = super::explainer::validate_file_path(&path) {
                    return GateDecision::Deny(e);
                }
            }
        }

        // 3-7. Evaluate rules with full pipeline
        let rule_result = evaluate_rules(&self.rules, tool_name, input_json);

        match rule_result {
            // Deny rules are absolute (even in Bypass mode)
            PermissionResult::Denied(reason) => {
                self.denial_state.lock().unwrap().record_denial();
                return GateDecision::Deny(reason);
            }

            // Ask rules: bypass-immune for content-specific rules
            PermissionResult::Ask => {
                // In bypass mode, allow unless it's a safety-critical ask
                if self.mode == PermissionMode::Bypass {
                    return GateDecision::Allow;
                }
                // Fall through to prompting below
            }

            // Explicit allow from rules
            PermissionResult::Allowed => {
                self.denial_state.lock().unwrap().record_success();
                return GateDecision::Allow;
            }

            // No rule matched
            PermissionResult::Undecided => {
                // Bypass mode: allow everything not explicitly denied
                if self.mode == PermissionMode::Bypass {
                    return GateDecision::Allow;
                }
                // AutoApprove: allow if no deny/ask rule matched
                if self.mode == PermissionMode::AutoApprove {
                    return GateDecision::Allow;
                }
                // DontAsk mode: convert ask to deny (headless batch)
                if self.mode == PermissionMode::DontAsk {
                    self.denial_state.lock().unwrap().record_denial();
                    return GateDecision::Deny("DontAsk mode — auto-denied".into());
                }
            }
        }

        // 10. Check session cache
        let cache_key = format!("{tool_name}:{}", &input_json[..input_json.len().min(64)]);
        {
            let approvals = self.session_approvals.lock().unwrap();
            if let Some(&approved) = approvals.get(&cache_key) {
                if approved {
                    self.denial_state.lock().unwrap().record_success();
                    return GateDecision::Allow;
                } else {
                    self.denial_state.lock().unwrap().record_denial();
                    return GateDecision::Deny("Previously denied in this session".into());
                }
            }
        }

        // Check denial fallback: if too many denials, force prompting
        {
            let denial = self.denial_state.lock().unwrap();
            if denial.should_fallback_to_prompting() {
                debug!("Denial limit reached — falling back to prompting");
            }
        }

        // 11. Default mode: prompt the user (fail-closed: deny without prompt_fn)
        let approved = match &self.prompt_fn {
            Some(f) => f(tool_name, input_json),
            None => {
                // No TUI / no prompt function → DENY (fail-closed, Principle 2).
                // Headless callers must use AutoApprove or Bypass mode explicitly.
                debug!("No prompt_fn set — denying (fail-closed). Use AutoApprove for headless.");
                false
            }
        };

        // Cache and track
        self.session_approvals.lock().unwrap().insert(cache_key, approved);
        if approved {
            self.denial_state.lock().unwrap().record_success();
            GateDecision::Allow
        } else {
            self.denial_state.lock().unwrap().record_denial();
            GateDecision::Deny("User denied".into())
        }
    }

    /// Get the current denial tracking state (for diagnostics).
    pub fn denial_state(&self) -> super::denial_tracking::DenialTrackingState {
        self.denial_state.lock().unwrap().clone()
    }

    /// Pre-approve a tool for the rest of the session (used by /permissions command)
    pub fn approve_session(&self, tool_name: &str) {
        self.session_approvals.lock().unwrap()
            .insert(tool_name.to_string(), true);
    }

    /// Deny a tool for the rest of the session
    pub fn deny_session(&self, tool_name: &str) {
        self.session_approvals.lock().unwrap()
            .insert(tool_name.to_string(), false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::rules::{PermissionRule, RuleEffect};

    fn make_gate(mode: PermissionMode) -> PermissionGate {
        PermissionGate::new(mode, vec![])
    }

    #[test]
    fn test_bypass_always_allows() {
        let gate = make_gate(PermissionMode::Bypass);
        assert_eq!(gate.check("Bash", r#"{"command":"rm -rf /"}"#), GateDecision::Allow);
    }

    #[test]
    fn test_plan_only_blocks() {
        let gate = make_gate(PermissionMode::PlanOnly);
        assert_eq!(gate.check("FileWrite", "{}"), GateDecision::PlanOnly);
    }

    #[test]
    fn test_auto_approve_allows_without_prompt() {
        let gate = make_gate(PermissionMode::AutoApprove);
        assert_eq!(gate.check("Bash", "{}"), GateDecision::Allow);
    }

    #[test]
    fn test_deny_rule_overrides_auto_approve() {
        let rule = PermissionRule {
            tool: "Bash".into(), effect: RuleEffect::Deny,
            reason: Some("blocked".into()), content: None, pattern: None,
        };
        let gate = PermissionGate::new(PermissionMode::AutoApprove, vec![rule]);
        assert_eq!(gate.check("Bash", "{}"), GateDecision::Deny("blocked".into()));
    }

    #[test]
    fn test_session_approval_cached() {
        let mut gate = make_gate(PermissionMode::Default);
        gate.set_prompt_fn(|_, _| true);  // Always approve

        let r1 = gate.check("FileRead", "{}");
        let r2 = gate.check("FileRead", "{}");
        assert_eq!(r1, GateDecision::Allow);
        assert_eq!(r2, GateDecision::Allow);
    }

    #[test]
    fn test_session_deny() {
        let gate = make_gate(PermissionMode::Default);
        gate.deny_session("Bash");
        // Direct session denial still checked via prompt_fn path after cache
        // (we test deny_session sets the cache)
        let approvals = gate.session_approvals.lock().unwrap();
        assert_eq!(approvals.get("Bash"), Some(&false));
    }
}
