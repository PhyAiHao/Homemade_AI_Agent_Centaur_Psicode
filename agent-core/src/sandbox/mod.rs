//! Sandbox — restricted execution environment for untrusted operations.
//!
//! Mirrors `src/utils/sandbox/`. Provides a sandboxed execution context
//! that restricts file system access, network, and process spawning.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Sandbox configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Whether sandbox mode is active.
    pub enabled: bool,
    /// Allowed directories for file operations.
    pub allowed_dirs: Vec<PathBuf>,
    /// Whether network access is allowed.
    pub allow_network: bool,
    /// Whether process spawning is allowed.
    pub allow_spawn: bool,
    /// Maximum file size for writes (bytes).
    pub max_file_size: u64,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        SandboxConfig {
            enabled: false,
            allowed_dirs: Vec::new(),
            allow_network: true,
            allow_spawn: true,
            max_file_size: 100 * 1024 * 1024, // 100MB
        }
    }
}

impl SandboxConfig {
    /// Load from environment variable.
    pub fn from_env() -> Self {
        let enabled = std::env::var("SANDBOX_MODE")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        SandboxConfig {
            enabled,
            ..Default::default()
        }
    }

    /// Check if a path is allowed.
    pub fn is_path_allowed(&self, path: &std::path::Path) -> bool {
        if !self.enabled || self.allowed_dirs.is_empty() {
            return true;
        }

        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        self.allowed_dirs.iter().any(|dir| {
            let dir_canonical = dir.canonicalize().unwrap_or_else(|_| dir.clone());
            canonical.starts_with(&dir_canonical)
        })
    }

    /// Check if a command is allowed in sandbox mode.
    pub fn is_command_allowed(&self, command: &str) -> bool {
        if !self.enabled {
            return true;
        }
        if !self.allow_spawn {
            return false;
        }
        // Block known dangerous commands in sandbox
        let blocked = ["rm", "sudo", "su", "chmod", "chown", "mkfs", "dd", "kill"];
        let cmd_name = command.split_whitespace().next().unwrap_or("");
        !blocked.iter().any(|b| cmd_name.ends_with(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_disabled() {
        let config = SandboxConfig::default();
        assert!(config.is_path_allowed(std::path::Path::new("/etc/passwd")));
        assert!(config.is_command_allowed("rm -rf /"));
    }

    #[test]
    fn test_sandbox_blocks_commands() {
        let config = SandboxConfig { enabled: true, allow_spawn: true, ..Default::default() };
        assert!(!config.is_command_allowed("sudo cat /etc/shadow"));
        assert!(!config.is_command_allowed("rm -rf /"));
        assert!(config.is_command_allowed("ls -la"));
        assert!(config.is_command_allowed("git status"));
    }
}
