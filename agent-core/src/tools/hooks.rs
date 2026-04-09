//! Tool hooks — pre/post execution hooks for tool calls.
//!
//! Mirrors `src/hooks/` and `src/schemas/hooks.ts`.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// When a hook fires relative to tool execution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum HookTiming {
    Pre,
    Post,
}

/// A configured hook that fires before or after a specific tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookDef {
    /// Tool name pattern (exact name or "*" for all tools).
    pub tool: String,

    /// When to fire.
    pub when: HookTiming,

    /// Shell command to execute.
    pub command: String,

    /// If true, a failing pre-hook blocks the tool from running.
    #[serde(default)]
    pub blocking: bool,
}

impl HookDef {
    /// Check if this hook applies to the given tool name.
    pub fn matches(&self, tool_name: &str) -> bool {
        self.tool == "*" || self.tool == tool_name
    }
}

/// Validate a set of hook definitions.
pub fn validate_hooks(hooks: &[HookDef]) -> Result<(), String> {
    for (i, hook) in hooks.iter().enumerate() {
        if hook.command.trim().is_empty() {
            return Err(format!("Hook {i}: command cannot be empty"));
        }
        if hook.tool.trim().is_empty() {
            return Err(format!("Hook {i}: tool pattern cannot be empty"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_matches_exact() {
        let hook = HookDef {
            tool: "Bash".to_string(),
            when: HookTiming::Pre,
            command: "echo ok".to_string(),
            blocking: false,
        };
        assert!(hook.matches("Bash"));
        assert!(!hook.matches("FileRead"));
    }

    #[test]
    fn hook_matches_wildcard() {
        let hook = HookDef {
            tool: "*".to_string(),
            when: HookTiming::Post,
            command: "echo done".to_string(),
            blocking: false,
        };
        assert!(hook.matches("Bash"));
        assert!(hook.matches("FileRead"));
        assert!(hook.matches("Agent"));
    }

    #[test]
    fn validate_rejects_empty_command() {
        let hooks = vec![HookDef {
            tool: "Bash".to_string(),
            when: HookTiming::Pre,
            command: "  ".to_string(),
            blocking: true,
        }];
        assert!(validate_hooks(&hooks).is_err());
    }
}
