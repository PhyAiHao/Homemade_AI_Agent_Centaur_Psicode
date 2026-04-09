//! Permission rule parser — parses/serializes `ToolName(content)` format.
//!
//! Mirrors `src/utils/permissions/permissionRuleParser.ts`.
//!
//! Format: `"ToolName"` or `"ToolName(content)"`.
//! Parentheses in content escaped: `\(`, `\)`. Backslashes: `\\`.
#![allow(dead_code)]

/// Parsed rule value.
#[derive(Debug, Clone, PartialEq)]
pub struct RuleValue {
    pub tool_name: String,
    pub content: Option<String>,
}

/// Parse a rule string like `"Bash(npm install)"` into a RuleValue.
pub fn parse_rule_string(s: &str) -> RuleValue {
    let s = s.trim();
    if let Some(paren_start) = s.find('(') {
        // Check that the string ends with ')'
        if s.ends_with(')') && paren_start < s.len() - 1 {
            let tool_name = s[..paren_start].to_string();
            let raw_content = &s[paren_start + 1..s.len() - 1];
            let content = unescape_content(raw_content);
            return RuleValue { tool_name, content: Some(content) };
        }
    }
    RuleValue { tool_name: s.to_string(), content: None }
}

/// Serialize a RuleValue back to string format.
pub fn serialize_rule(value: &RuleValue) -> String {
    match &value.content {
        None => value.tool_name.clone(),
        Some(content) => {
            let escaped = escape_content(content);
            format!("{}({})", value.tool_name, escaped)
        }
    }
}

fn unescape_content(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.peek() {
                Some('(') => { result.push('('); chars.next(); }
                Some(')') => { result.push(')'); chars.next(); }
                Some('\\') => { result.push('\\'); chars.next(); }
                _ => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}

fn escape_content(s: &str) -> String {
    s.replace('\\', "\\\\")
     .replace('(', "\\(")
     .replace(')', "\\)")
}

// ─── Shell rule matching ────────────────────────────────────────────────────

/// Discriminated shell permission rule types.
#[derive(Debug, Clone, PartialEq)]
pub enum ShellRule {
    /// Exact command match.
    Exact(String),
    /// Prefix match (legacy `:*` syntax, e.g., `npm:*` matches `npm install`).
    Prefix(String),
    /// Wildcard pattern (`*` matches any characters).
    Wildcard(String),
}

/// Parse a rule content string into a ShellRule.
pub fn parse_shell_rule(content: &str) -> ShellRule {
    // Check for :* suffix (legacy prefix syntax)
    if let Some(prefix) = content.strip_suffix(":*") {
        return ShellRule::Prefix(prefix.to_string());
    }

    // Check for unescaped * (wildcard)
    let has_wildcard = content.chars().enumerate().any(|(i, c)| {
        c == '*' && (i == 0 || content.as_bytes()[i - 1] != b'\\')
    });

    if has_wildcard {
        ShellRule::Wildcard(content.to_string())
    } else {
        ShellRule::Exact(content.to_string())
    }
}

/// Check if a command matches a shell rule.
pub fn shell_rule_matches(rule: &ShellRule, command: &str) -> bool {
    match rule {
        ShellRule::Exact(expected) => {
            command.eq_ignore_ascii_case(expected)
        }
        ShellRule::Prefix(prefix) => {
            let cmd_lower = command.to_lowercase();
            let prefix_lower = prefix.to_lowercase();
            cmd_lower == prefix_lower
                || cmd_lower.starts_with(&format!("{prefix_lower} "))
                || cmd_lower.starts_with(&format!("{prefix_lower}\t"))
        }
        ShellRule::Wildcard(pattern) => {
            wildcard_match(pattern, command)
        }
    }
}

/// Match a wildcard pattern against a string.
/// `*` matches any sequence (including empty), `\*` matches literal `*`.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    // Convert wildcard pattern to regex
    let mut regex_str = String::from("(?si)^"); // case-insensitive, dotall
    let mut chars = pattern.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if let Some(&next) = chars.peek() {
                    if next == '*' {
                        regex_str.push_str("\\*");
                        chars.next();
                    } else if next == '\\' {
                        regex_str.push_str("\\\\");
                        chars.next();
                    } else {
                        regex_str.push_str("\\\\");
                    }
                } else {
                    regex_str.push_str("\\\\");
                }
            }
            '*' => {
                regex_str.push_str(".*");
            }
            c => {
                regex_str.push_str(&regex::escape(&c.to_string()));
            }
        }
    }
    regex_str.push('$');

    regex::Regex::new(&regex_str)
        .map(|re| re.is_match(text))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple() {
        assert_eq!(parse_rule_string("Bash"), RuleValue {
            tool_name: "Bash".into(), content: None
        });
    }

    #[test]
    fn test_parse_with_content() {
        assert_eq!(parse_rule_string("Bash(npm install)"), RuleValue {
            tool_name: "Bash".into(), content: Some("npm install".into())
        });
    }

    #[test]
    fn test_parse_escaped_parens() {
        let input = "Bash(python -c print\\(1\\))";
        let result = parse_rule_string(input);
        assert_eq!(result.tool_name, "Bash");
        assert_eq!(result.content, Some("python -c print(1)".into()));
    }

    #[test]
    fn test_serialize_roundtrip() {
        let val = RuleValue { tool_name: "Bash".into(), content: Some("npm install".into()) };
        assert_eq!(serialize_rule(&val), "Bash(npm install)");
    }

    #[test]
    fn test_shell_exact() {
        let rule = parse_shell_rule("npm install");
        assert!(shell_rule_matches(&rule, "npm install"));
        assert!(!shell_rule_matches(&rule, "npm run test"));
    }

    #[test]
    fn test_shell_prefix() {
        let rule = parse_shell_rule("git:*");
        assert!(shell_rule_matches(&rule, "git"));
        assert!(shell_rule_matches(&rule, "git add ."));
        assert!(!shell_rule_matches(&rule, "github"));
    }

    #[test]
    fn test_shell_wildcard() {
        let rule = parse_shell_rule("npm run *");
        assert!(shell_rule_matches(&rule, "npm run test"));
        assert!(shell_rule_matches(&rule, "npm run build"));
        assert!(!shell_rule_matches(&rule, "npm install"));
    }
}
