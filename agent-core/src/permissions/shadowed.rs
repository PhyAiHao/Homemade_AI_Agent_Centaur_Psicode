//! Shadowed rule detection — mirrors `src/utils/permissions/shadowedRuleDetection.ts`.
//!
//! Detects unreachable allow rules that are blocked by broader deny/ask rules.
#![allow(dead_code)]

use super::rules::{PermissionRule, RuleEffect};

/// A rule that can never take effect because a broader rule blocks it.
#[derive(Debug, Clone)]
pub struct UnreachableRule {
    /// The rule that is unreachable.
    pub rule: PermissionRule,
    /// The broader rule that shadows it.
    pub shadowed_by: PermissionRule,
    /// Type of shadow.
    pub shadow_type: ShadowType,
    /// Suggested fix.
    pub fix: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ShadowType {
    /// A deny rule completely blocks this allow rule.
    DenyShadow,
    /// An ask rule forces prompting, making this allow rule unreachable.
    AskShadow,
}

/// Detect unreachable allow rules in the rule list.
pub fn detect_unreachable_rules(rules: &[PermissionRule]) -> Vec<UnreachableRule> {
    let mut unreachable = Vec::new();

    for rule in rules {
        // Only check content-specific allow rules
        if rule.effect != RuleEffect::Allow || rule.is_tool_wide() {
            continue;
        }

        // Check if any tool-wide deny rule shadows this
        let deny_shadow = rules.iter().find(|r| {
            r.effect == RuleEffect::Deny
                && r.is_tool_wide()
                && r.tool == rule.tool
        });

        if let Some(blocker) = deny_shadow {
            unreachable.push(UnreachableRule {
                rule: rule.clone(),
                shadowed_by: blocker.clone(),
                shadow_type: ShadowType::DenyShadow,
                fix: format!(
                    "Remove the deny rule for '{}' or make it content-specific",
                    rule.tool
                ),
            });
            continue;
        }

        // Check if any tool-wide ask rule shadows this
        let ask_shadow = rules.iter().find(|r| {
            r.effect == RuleEffect::Ask
                && r.is_tool_wide()
                && r.tool == rule.tool
        });

        if let Some(blocker) = ask_shadow {
            unreachable.push(UnreachableRule {
                rule: rule.clone(),
                shadowed_by: blocker.clone(),
                shadow_type: ShadowType::AskShadow,
                fix: format!(
                    "The ask rule for '{}' overrides this allow rule. \
                     Remove the ask rule or make it content-specific",
                    rule.tool
                ),
            });
        }
    }

    unreachable
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deny_shadows_allow() {
        let rules = vec![
            PermissionRule {
                tool: "Bash".into(), effect: RuleEffect::Deny,
                reason: None, content: None, pattern: None,
            },
            PermissionRule {
                tool: "Bash".into(), effect: RuleEffect::Allow,
                reason: None, content: Some("ls".into()), pattern: None,
            },
        ];
        let unreachable = detect_unreachable_rules(&rules);
        assert_eq!(unreachable.len(), 1);
        assert_eq!(unreachable[0].shadow_type, ShadowType::DenyShadow);
    }

    #[test]
    fn test_no_shadow_when_different_tool() {
        let rules = vec![
            PermissionRule {
                tool: "Bash".into(), effect: RuleEffect::Deny,
                reason: None, content: None, pattern: None,
            },
            PermissionRule {
                tool: "FileRead".into(), effect: RuleEffect::Allow,
                reason: None, content: Some("/tmp".into()), pattern: None,
            },
        ];
        let unreachable = detect_unreachable_rules(&rules);
        assert!(unreachable.is_empty());
    }

    #[test]
    fn test_ask_shadows_allow() {
        let rules = vec![
            PermissionRule {
                tool: "Bash".into(), effect: RuleEffect::Ask,
                reason: None, content: None, pattern: None,
            },
            PermissionRule {
                tool: "Bash".into(), effect: RuleEffect::Allow,
                reason: None, content: Some("npm install".into()), pattern: None,
            },
        ];
        let unreachable = detect_unreachable_rules(&rules);
        assert_eq!(unreachable.len(), 1);
        assert_eq!(unreachable[0].shadow_type, ShadowType::AskShadow);
    }
}
