//! Permission rules — per-tool allow/deny/ask rules.
//!
//! Mirrors `src/utils/permissions/PermissionRule.ts` and `PermissionResult.ts`.
//! Extended with `Ask` effect and `content` field for `ToolName(content)` format.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use super::rule_parser::{parse_shell_rule, shell_rule_matches};

/// A single permission rule.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRule {
    /// Tool name this rule applies to. Supports "*" wildcard.
    pub tool:    String,
    /// Whether this rule grants, denies, or prompts.
    pub effect:  RuleEffect,
    /// Optional description shown to the user.
    pub reason:  Option<String>,
    /// Content pattern: for `ToolName(content)` format.
    /// None = tool-wide rule. Some = content-specific rule.
    pub content: Option<String>,
    /// Legacy: regex pattern match against input JSON.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuleEffect {
    Allow,
    Deny,
    /// Prompt the user (bypass-immune for content-specific rules).
    Ask,
}

/// Outcome of a permission check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PermissionResult {
    /// Tool may execute.
    Allowed,
    /// Tool was denied; contains the reason.
    Denied(String),
    /// User should be prompted.
    Ask,
    /// No rule matched — fall through to next check.
    Undecided,
}

impl PermissionRule {
    /// Check whether this rule matches the given tool name and input.
    ///
    /// For Bash tools with `content`, uses shell rule matching (exact/prefix/wildcard).
    /// For other tools, content is matched as substring of input_json.
    pub fn matches(&self, tool_name: &str, input_json: &str) -> bool {
        // Tool name match (wildcard or exact)
        if self.tool != "*" && self.tool != tool_name {
            return false;
        }

        // Tool-wide rule (no content, no pattern) matches everything
        if self.content.is_none() && self.pattern.is_none() {
            return true;
        }

        // Content-specific matching
        if let Some(ref content) = self.content {
            if tool_name == "Bash" || tool_name == "Shell" || tool_name == "PowerShell" {
                // Shell rule matching: extract command from input JSON
                let command = super::extract_command(input_json);
                let shell_rule = parse_shell_rule(content);
                return shell_rule_matches(&shell_rule, &command);
            } else {
                // For other tools: substring match on input
                return input_json.contains(content.as_str());
            }
        }

        // Legacy pattern matching (regex on input)
        if let Some(ref pat) = self.pattern {
            return input_json.contains(pat.as_str());
        }

        false
    }

    /// Whether this is a tool-wide rule (no content specifier).
    pub fn is_tool_wide(&self) -> bool {
        self.content.is_none() && self.pattern.is_none()
    }
}


/// Evaluate a list of rules against a tool invocation.
///
/// Follows the original's evaluation order:
/// 1. Tool-wide deny rules → Denied
/// 2. Tool-wide ask rules → Ask
/// 3. Content-specific rules → Allow/Deny/Ask
/// 4. No match → Undecided
pub fn evaluate_rules(
    rules: &[PermissionRule],
    tool_name: &str,
    input_json: &str,
) -> PermissionResult {
    // Phase 1: Check tool-wide deny rules first
    for rule in rules {
        if rule.matches(tool_name, input_json) && rule.is_tool_wide() && rule.effect == RuleEffect::Deny {
            return PermissionResult::Denied(
                rule.reason.clone().unwrap_or_else(|| format!("Denied by rule for {}", rule.tool))
            );
        }
    }

    // Phase 2: Check tool-wide ask rules
    for rule in rules {
        if rule.matches(tool_name, input_json) && rule.is_tool_wide() && rule.effect == RuleEffect::Ask {
            return PermissionResult::Ask;
        }
    }

    // Phase 3: Check content-specific rules (in order)
    for rule in rules {
        if !rule.is_tool_wide() && rule.matches(tool_name, input_json) {
            return match rule.effect {
                RuleEffect::Allow => PermissionResult::Allowed,
                RuleEffect::Deny => PermissionResult::Denied(
                    rule.reason.clone().unwrap_or_else(|| format!("Denied by content rule for {}", rule.tool))
                ),
                RuleEffect::Ask => PermissionResult::Ask,
            };
        }
    }

    // Phase 4: Check tool-wide allow rules
    for rule in rules {
        if rule.matches(tool_name, input_json) && rule.is_tool_wide() && rule.effect == RuleEffect::Allow {
            return PermissionResult::Allowed;
        }
    }

    PermissionResult::Undecided
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow_rule(tool: &str) -> PermissionRule {
        PermissionRule { tool: tool.into(), effect: RuleEffect::Allow, reason: None, content: None, pattern: None }
    }

    fn deny_rule(tool: &str, reason: &str) -> PermissionRule {
        PermissionRule {
            tool: tool.into(), effect: RuleEffect::Deny,
            reason: Some(reason.into()), content: None, pattern: None,
        }
    }

    fn content_allow(tool: &str, content: &str) -> PermissionRule {
        PermissionRule {
            tool: tool.into(), effect: RuleEffect::Allow,
            reason: None, content: Some(content.into()), pattern: None,
        }
    }

    #[test]
    fn test_allow_specific_tool() {
        let rules = vec![allow_rule("Bash")];
        assert_eq!(evaluate_rules(&rules, "Bash", "{}"), PermissionResult::Allowed);
    }

    #[test]
    fn test_deny_specific_tool() {
        let rules = vec![deny_rule("Bash", "not allowed")];
        assert_eq!(
            evaluate_rules(&rules, "Bash", "{}"),
            PermissionResult::Denied("not allowed".into())
        );
    }

    #[test]
    fn test_wildcard_allow() {
        let rules = vec![allow_rule("*")];
        assert_eq!(evaluate_rules(&rules, "FileRead", "{}"), PermissionResult::Allowed);
    }

    #[test]
    fn test_undecided_when_no_match() {
        let rules = vec![allow_rule("Bash")];
        assert_eq!(evaluate_rules(&rules, "FileWrite", "{}"), PermissionResult::Undecided);
    }

    #[test]
    fn test_content_specific_bash_rule() {
        let rules = vec![content_allow("Bash", "npm install")];
        assert_eq!(
            evaluate_rules(&rules, "Bash", r#"{"command":"npm install"}"#),
            PermissionResult::Allowed
        );
        assert_eq!(
            evaluate_rules(&rules, "Bash", r#"{"command":"rm -rf /"}"#),
            PermissionResult::Undecided
        );
    }

    #[test]
    fn test_content_prefix_bash_rule() {
        let rules = vec![content_allow("Bash", "git:*")];
        assert_eq!(
            evaluate_rules(&rules, "Bash", r#"{"command":"git add ."}"#),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn test_deny_overrides_content_allow() {
        let rules = vec![
            deny_rule("Bash", "blocked"),
            content_allow("Bash", "ls"),
        ];
        // Tool-wide deny should win
        assert_eq!(
            evaluate_rules(&rules, "Bash", r#"{"command":"ls"}"#),
            PermissionResult::Denied("blocked".into())
        );
    }

    #[test]
    fn test_ask_rule() {
        let rules = vec![PermissionRule {
            tool: "Bash".into(), effect: RuleEffect::Ask,
            reason: None, content: None, pattern: None,
        }];
        assert_eq!(evaluate_rules(&rules, "Bash", "{}"), PermissionResult::Ask);
    }

    #[test]
    fn test_pattern_matching() {
        let rule = PermissionRule {
            tool: "Bash".into(), effect: RuleEffect::Deny,
            reason: Some("rm blocked".into()),
            content: None,
            pattern: Some("rm -rf".into()),
        };
        assert_eq!(
            evaluate_rules(&[rule], "Bash", r#"{"command":"rm -rf /"}"#),
            PermissionResult::Denied("rm blocked".into())
        );
    }
}
