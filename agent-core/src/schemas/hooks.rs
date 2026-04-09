//! Hook schema — validated types for pre/post tool hooks.
//!
//! Mirrors `src/schemas/hooks.ts`.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// A hook definition — shell command run before or after tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookDefinition {
    /// Tool name this hook applies to. "*" means all tools.
    pub tool:    String,
    /// Hook timing: "pre" (before) or "post" (after) tool execution.
    pub when:    HookTiming,
    /// Shell command to execute.
    pub command: String,
    /// If true, tool execution is blocked when this hook exits non-zero.
    #[serde(default)]
    pub blocking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HookTiming {
    Pre,
    Post,
}

/// Validate a list of hook definitions.
pub fn validate_hooks(hooks: &[HookDefinition]) -> Result<(), String> {
    for hook in hooks {
        if hook.command.trim().is_empty() {
            return Err(format!("Hook for '{}' has empty command", hook.tool));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hook_deserialization() {
        let json = r#"{
            "tool": "Bash",
            "when": "pre",
            "command": "echo before",
            "blocking": true
        }"#;
        let hook: HookDefinition = serde_json::from_str(json).unwrap();
        assert_eq!(hook.tool, "Bash");
        assert_eq!(hook.when, HookTiming::Pre);
        assert!(hook.blocking);
    }

    #[test]
    fn test_validate_hooks_rejects_empty_command() {
        let hooks = vec![HookDefinition {
            tool: "Bash".into(),
            when: HookTiming::Pre,
            command: "  ".into(),
            blocking: false,
        }];
        assert!(validate_hooks(&hooks).is_err());
    }
}
