//! Dangerous pattern detection — mirrors `src/utils/permissions/dangerousPatterns.ts`.
//!
//! Detects dangerous bash/shell permission rules that could allow
//! arbitrary code execution if approved.
#![allow(dead_code)]

/// Patterns considered dangerous for shell execution.
/// These are commands that can execute arbitrary code, escalate
/// privileges, or access network resources in unrestricted ways.
pub const DANGEROUS_BASH_PATTERNS: &[&str] = &[
    // Interpreters / code execution
    "python", "python3", "python2",
    "node", "deno", "tsx", "bun",
    "ruby", "perl", "php", "lua",
    // Package runners (can execute arbitrary packages)
    "npx", "bunx",
    "npm run", "yarn run", "pnpm run", "bun run",
    // Shells
    "bash", "sh", "zsh", "fish",
    // Dangerous builtins / utilities
    "eval", "exec", "env", "xargs", "sudo",
    // Network tools
    "curl", "wget",
    // Version control (can run hooks)
    "git",
    // Remote access
    "ssh",
];

/// Cross-platform code execution patterns (shared between Bash and PowerShell).
pub const CROSS_PLATFORM_CODE_EXEC: &[&str] = &[
    "python", "python3", "python2",
    "node", "deno", "tsx",
    "ruby", "perl", "php", "lua",
    "npx", "bunx",
    "npm run", "yarn run", "pnpm run", "bun run",
    "bash", "sh", "ssh",
];

/// Check if a Bash permission rule content is dangerous.
///
/// Returns true if the rule would allow executing dangerous commands.
/// Used to strip dangerous rules when entering auto mode.
pub fn is_dangerous_bash_permission(content: &str) -> bool {
    let content_lower = content.to_lowercase().trim().to_string();

    // Bare "Bash" or "Bash(*)" — allows everything
    if content_lower.is_empty() || content_lower == "*" {
        return true;
    }

    for pattern in DANGEROUS_BASH_PATTERNS {
        let p = pattern.to_lowercase();

        // Exact match
        if content_lower == p {
            return true;
        }

        // Prefix syntax: "pattern:*"
        if content_lower == format!("{p}:*") {
            return true;
        }

        // Trailing wildcard: "pattern*"
        if content_lower == format!("{p}*") {
            return true;
        }

        // Space wildcard: "pattern *"
        if content_lower == format!("{p} *") {
            return true;
        }

        // Dash wildcard: "pattern -..." ending with "*"
        if content_lower.starts_with(&format!("{p} -")) && content_lower.ends_with('*') {
            return true;
        }
    }

    false
}

/// Check if a tool + content pair represents a dangerous permission.
/// Used during permission setup to warn users.
pub fn is_dangerous_permission(tool_name: &str, content: Option<&str>) -> bool {
    match tool_name {
        "Bash" | "Shell" | "PowerShell" => {
            match content {
                None => true, // Tool-wide = dangerous
                Some(c) => is_dangerous_bash_permission(c),
            }
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bare_bash_is_dangerous() {
        assert!(is_dangerous_bash_permission(""));
        assert!(is_dangerous_bash_permission("*"));
    }

    #[test]
    fn test_exact_pattern() {
        assert!(is_dangerous_bash_permission("python"));
        assert!(is_dangerous_bash_permission("sudo"));
        assert!(is_dangerous_bash_permission("eval"));
    }

    #[test]
    fn test_prefix_syntax() {
        assert!(is_dangerous_bash_permission("git:*"));
        assert!(is_dangerous_bash_permission("npm run:*"));
    }

    #[test]
    fn test_wildcard_patterns() {
        assert!(is_dangerous_bash_permission("python *"));
        assert!(is_dangerous_bash_permission("node*"));
        assert!(is_dangerous_bash_permission("curl -X *"));
    }

    #[test]
    fn test_safe_commands() {
        assert!(!is_dangerous_bash_permission("ls"));
        assert!(!is_dangerous_bash_permission("cat"));
        assert!(!is_dangerous_bash_permission("echo hello"));
        assert!(!is_dangerous_bash_permission("cargo test"));
    }

    #[test]
    fn test_case_insensitive() {
        assert!(is_dangerous_bash_permission("Python"));
        assert!(is_dangerous_bash_permission("SUDO"));
    }
}
