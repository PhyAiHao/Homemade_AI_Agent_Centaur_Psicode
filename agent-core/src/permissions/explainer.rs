//! Permission explainer + path validation.
//!
//! Provides human-readable risk explanations for permission decisions
//! and validates file paths for safety (.git/, .claude/, UNC paths).
#![allow(dead_code)]

/// Risk levels for permission explanations.
#[derive(Debug, Clone, PartialEq)]
pub enum RiskLevel {
    /// Safe operation (read-only, no side effects).
    Safe,
    /// Low risk (file writes within project directory).
    Low,
    /// Medium risk (shell commands, network access).
    Medium,
    /// High risk (destructive commands, system modifications).
    High,
    /// Critical (credential access, privilege escalation).
    Critical,
}

impl RiskLevel {
    pub fn label(&self) -> &str {
        match self {
            Self::Safe => "Safe",
            Self::Low => "Low risk",
            Self::Medium => "Medium risk",
            Self::High => "High risk",
            Self::Critical => "Critical risk",
        }
    }

    pub fn color_hint(&self) -> &str {
        match self {
            Self::Safe => "green",
            Self::Low => "green",
            Self::Medium => "yellow",
            Self::High => "red",
            Self::Critical => "red",
        }
    }
}

/// Assess the risk level of a tool operation.
pub fn assess_risk(tool_name: &str, input_json: &str) -> (RiskLevel, String) {
    match tool_name {
        "FileRead" | "Glob" | "Grep" | "ToolSearch" | "Brief" | "Sleep"
        | "TaskGet" | "TaskList" | "TaskOutput" => {
            (RiskLevel::Safe, "Read-only operation".into())
        }

        "FileEdit" | "FileWrite" => {
            let path = super::extract_file_path(input_json).unwrap_or_else(|| "unknown".to_string());
            if is_sensitive_path(&path) {
                (RiskLevel::High, format!("Writing to sensitive path: {path}"))
            } else {
                (RiskLevel::Low, format!("File modification: {path}"))
            }
        }

        "Bash" | "Shell" | "PowerShell" => {
            let cmd = super::extract_command(input_json);
            if cmd.contains("sudo") || cmd.contains("su ") {
                (RiskLevel::Critical, "Privilege escalation detected".into())
            } else if cmd.contains("rm ") || cmd.contains("rmdir") || cmd.contains("del ") {
                (RiskLevel::High, "Destructive command detected".into())
            } else if cmd.contains("curl") || cmd.contains("wget") || cmd.contains("ssh") {
                (RiskLevel::Medium, "Network access".into())
            } else if cmd.contains("git push") || cmd.contains("git reset") {
                (RiskLevel::Medium, "Git operation with remote/destructive potential".into())
            } else {
                (RiskLevel::Medium, format!("Shell command: {}", truncate(&cmd, 60)))
            }
        }

        "Agent" => (RiskLevel::Medium, "Spawns a sub-agent".into()),
        "WebFetch" | "WebSearch" => (RiskLevel::Medium, "Network access".into()),

        _ => (RiskLevel::Low, format!("Tool: {tool_name}"))
    }
}

// ─── Path validation ────────────────────────────────────────────────────────

/// Sensitive paths that require extra confirmation.
const SENSITIVE_PATHS: &[&str] = &[
    ".git/",
    ".git\\",
    ".claude/",
    ".claude\\",
    ".env",
    ".ssh/",
    ".aws/",
    ".npmrc",
    ".pypirc",
    "credentials",
    "secrets",
    "id_rsa",
    "id_ed25519",
];

/// UNC path prefixes (Windows network paths — NTLM credential leak risk).
const UNC_PREFIXES: &[&str] = &["\\\\", "//"];

/// Check if a path is sensitive and requires extra confirmation.
pub fn is_sensitive_path(path: &str) -> bool {
    let path_lower = path.to_lowercase();

    // Check UNC paths (NTLM credential leak)
    for prefix in UNC_PREFIXES {
        if path_lower.starts_with(prefix) {
            return true;
        }
    }

    // Check sensitive patterns
    for pattern in SENSITIVE_PATHS {
        if path_lower.contains(&pattern.to_lowercase()) {
            return true;
        }
    }

    false
}

/// Validate a file path for permission checking.
/// Returns an error string if the path is dangerous.
pub fn validate_file_path(path: &str) -> Result<(), String> {
    // Block UNC paths (NTLM credential leak on Windows)
    for prefix in UNC_PREFIXES {
        if path.starts_with(prefix) {
            return Err(format!(
                "UNC paths ({prefix}...) are blocked to prevent NTLM credential leaks"
            ));
        }
    }

    // Block device paths
    let device_paths = ["/dev/zero", "/dev/random", "/dev/urandom", "/dev/stdin",
        "/dev/null", "/proc/self/fd/0", "/proc/self/fd/1", "/proc/self/fd/2"];
    for dev in &device_paths {
        if path == *dev {
            return Err(format!("Device path {dev} is blocked"));
        }
    }

    Ok(())
}

/// Check if a path is within the project directory.
pub fn is_within_project(path: &str, project_root: &str) -> bool {
    let abs_path = std::path::Path::new(path);
    let abs_root = std::path::Path::new(project_root);

    match (abs_path.canonicalize(), abs_root.canonicalize()) {
        (Ok(p), Ok(r)) => p.starts_with(r),
        _ => path.starts_with(project_root),
    }
}

// ─── Helpers ────────────────────────────────────────────────────────────────


fn truncate(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sensitive_paths() {
        assert!(is_sensitive_path(".git/config"));
        assert!(is_sensitive_path("/home/user/.ssh/id_rsa"));
        assert!(is_sensitive_path(".env"));
        assert!(is_sensitive_path("\\\\server\\share"));
        assert!(!is_sensitive_path("src/main.rs"));
    }

    #[test]
    fn test_risk_assessment() {
        let (level, _) = assess_risk("FileRead", r#"{"file_path":"src/main.rs"}"#);
        assert_eq!(level, RiskLevel::Safe);

        let (level, _) = assess_risk("Bash", r#"{"command":"sudo rm -rf /"}"#);
        assert_eq!(level, RiskLevel::Critical);

        let (level, _) = assess_risk("FileEdit", r#"{"file_path":".git/config"}"#);
        assert_eq!(level, RiskLevel::High);
    }

    #[test]
    fn test_validate_path() {
        assert!(validate_file_path("src/main.rs").is_ok());
        assert!(validate_file_path("\\\\server\\share").is_err());
        assert!(validate_file_path("/dev/zero").is_err());
    }
}
