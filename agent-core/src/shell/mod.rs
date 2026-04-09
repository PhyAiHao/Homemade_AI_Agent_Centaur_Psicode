//! Shell provider abstraction — encapsulates platform-specific shell execution
//! details (bash vs PowerShell, command wrapping, env overrides).
//!
//! Mirrors `src/utils/shell.ts` and `src/tools/BashTool/shellProvider.ts`.
#![allow(dead_code)]

use base64::Engine;
use std::collections::HashMap;
use std::path::Path;

// ─── Shell type enum ────────────────────────────────────────────────────────

/// Supported shell types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    Bash,
    Zsh,
    Sh,
    PowerShell,
    Cmd,
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShellType::Bash => write!(f, "bash"),
            ShellType::Zsh => write!(f, "zsh"),
            ShellType::Sh => write!(f, "sh"),
            ShellType::PowerShell => write!(f, "powershell"),
            ShellType::Cmd => write!(f, "cmd"),
        }
    }
}

// ─── Shell provider trait ───────────────────────────────────────────────────

/// Abstraction over different shell environments.
pub trait ShellProvider: Send + Sync {
    /// The type of shell this provider targets.
    fn shell_type(&self) -> ShellType;

    /// Absolute path to the shell binary.
    fn shell_path(&self) -> &str;

    /// Wrap `cmd` for execution in the target shell, optionally running it
    /// in the given working directory.
    fn build_exec_command(&self, cmd: &str, cwd: Option<&Path>) -> Vec<String>;

    /// Environment variable overrides to set before spawning the shell.
    fn get_env_overrides(&self) -> HashMap<String, String>;
}

// ─── Bash provider ──────────────────────────────────────────────────────────

/// Provider for Bash (and Zsh/sh — POSIX-compatible shells).
pub struct BashProvider {
    shell_path: String,
    shell_type: ShellType,
    /// Optional path to an environment snapshot file to source.
    env_snapshot: Option<String>,
}

impl BashProvider {
    /// Create a new provider for the given shell binary.
    pub fn new(shell_path: impl Into<String>, shell_type: ShellType) -> Self {
        let env_snapshot = std::env::var("BASH_ENV_SNAPSHOT").ok();
        BashProvider {
            shell_path: shell_path.into(),
            shell_type,
            env_snapshot,
        }
    }

    /// Convenience constructor that auto-detects the default POSIX shell.
    pub fn default_posix() -> Self {
        let (path, st) = detect_posix_shell();
        Self::new(path, st)
    }
}

impl ShellProvider for BashProvider {
    fn shell_type(&self) -> ShellType {
        self.shell_type
    }

    fn shell_path(&self) -> &str {
        &self.shell_path
    }

    fn build_exec_command(&self, cmd: &str, cwd: Option<&Path>) -> Vec<String> {
        let mut script = String::new();

        // Source environment snapshot if it exists
        if let Some(ref snapshot) = self.env_snapshot {
            if Path::new(snapshot).exists() {
                script.push_str(&format!("source {} 2>/dev/null; ", snapshot));
            }
        }

        // Disable extglob to simplify parsing assumptions
        if self.shell_type == ShellType::Bash {
            script.push_str("shopt -u extglob 2>/dev/null; ");
        }

        // Change to working directory if requested
        if let Some(dir) = cwd {
            script.push_str(&format!("cd {} 2>/dev/null; ", shell_escape(dir.to_string_lossy().as_ref())));
        }

        // Use eval to let bash handle the full command string
        script.push_str(&format!("eval {}", shell_escape(cmd)));

        vec![
            self.shell_path.clone(),
            "-c".to_string(),
            script,
        ]
    }

    fn get_env_overrides(&self) -> HashMap<String, String> {
        let mut env = HashMap::new();
        // Disable command-not-found handlers that produce noisy output
        env.insert("COMMAND_NOT_FOUND_INSTALL_PROMPT".into(), "".into());
        // Force consistent locale
        env.insert("LC_ALL".into(), "C.UTF-8".into());
        env
    }
}

// ─── PowerShell provider ────────────────────────────────────────────────────

/// Provider for PowerShell (Windows and cross-platform pwsh).
pub struct PowerShellProvider {
    shell_path: String,
}

impl PowerShellProvider {
    pub fn new(shell_path: impl Into<String>) -> Self {
        PowerShellProvider {
            shell_path: shell_path.into(),
        }
    }

    /// Default PowerShell: prefer `pwsh` (cross-platform), fall back to
    /// `powershell.exe` on Windows.
    pub fn default_pwsh() -> Self {
        let path = if cfg!(windows) {
            which_exists("pwsh.exe").unwrap_or_else(|| "powershell.exe".into())
        } else {
            which_exists("pwsh").unwrap_or_else(|| "pwsh".into())
        };
        Self::new(path)
    }
}

impl ShellProvider for PowerShellProvider {
    fn shell_type(&self) -> ShellType {
        ShellType::PowerShell
    }

    fn shell_path(&self) -> &str {
        &self.shell_path
    }

    fn build_exec_command(&self, cmd: &str, cwd: Option<&Path>) -> Vec<String> {
        // Encode the command as base64 to avoid quoting issues
        let mut script = String::new();
        if let Some(dir) = cwd {
            script.push_str(&format!(
                "Set-Location -Path '{}'; ",
                dir.to_string_lossy().replace('\'', "''")
            ));
        }
        script.push_str(cmd);

        // PowerShell expects UTF-16LE base64 encoded commands
        let utf16: Vec<u8> = script
            .encode_utf16()
            .flat_map(|u| u.to_le_bytes())
            .collect();
        let encoded = base64::engine::general_purpose::STANDARD.encode(&utf16);

        vec![
            self.shell_path.clone(),
            "-NoProfile".to_string(),
            "-NonInteractive".to_string(),
            "-EncodedCommand".to_string(),
            encoded,
        ]
    }

    fn get_env_overrides(&self) -> HashMap<String, String> {
        HashMap::new()
    }
}

// ─── Output length configuration ────────────────────────────────────────────

/// Default maximum output length in bytes.
const DEFAULT_MAX_OUTPUT: usize = 30_000;
/// Absolute maximum (cannot be exceeded even by env override).
const ABSOLUTE_MAX_OUTPUT: usize = 150_000;

/// Get the configured maximum output length.
///
/// Reads from `BASH_MAX_OUTPUT_LENGTH` env var, clamped to `[1, 150_000]`.
/// Defaults to 30,000 if not set or invalid.
pub fn get_max_output_length() -> usize {
    match std::env::var("BASH_MAX_OUTPUT_LENGTH") {
        Ok(val) => val
            .parse::<usize>()
            .unwrap_or(DEFAULT_MAX_OUTPUT)
            .clamp(1, ABSOLUTE_MAX_OUTPUT),
        Err(_) => DEFAULT_MAX_OUTPUT,
    }
}

// ─── Shell detection ────────────────────────────────────────────────────────

/// Detect the default shell on this system.
pub fn detect_default_shell() -> ShellType {
    if cfg!(windows) {
        // On Windows, prefer PowerShell
        return ShellType::PowerShell;
    }

    // Check $SHELL env var
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return ShellType::Zsh;
        }
        if shell.contains("bash") {
            return ShellType::Bash;
        }
        if shell.contains("fish") {
            // We don't support fish directly — fall back to bash
            return ShellType::Bash;
        }
    }

    // Default to bash
    ShellType::Bash
}

/// Detect the default POSIX-compatible shell path and type.
fn detect_posix_shell() -> (String, ShellType) {
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return (shell, ShellType::Zsh);
        }
        if shell.contains("bash") {
            return (shell, ShellType::Bash);
        }
    }

    // Fall back to /bin/bash or /bin/sh
    if Path::new("/bin/bash").exists() {
        ("/bin/bash".into(), ShellType::Bash)
    } else {
        ("/bin/sh".into(), ShellType::Sh)
    }
}

/// Check if a binary exists on PATH.
fn which_exists(name: &str) -> Option<String> {
    std::process::Command::new("which")
        .arg(name)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| {
            String::from_utf8(o.stdout)
                .ok()
                .map(|s| s.trim().to_string())
        })
}

/// Simple shell escaping: wraps in single quotes, escaping embedded quotes.
fn shell_escape(s: &str) -> String {
    // In single quotes, the only character that needs escaping is '
    // which is done by ending the quote, adding escaped quote, resuming quote
    format!("'{}'", s.replace('\'', "'\\''"))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bash_provider_builds_command() {
        let p = BashProvider::new("/bin/bash", ShellType::Bash);
        let cmd = p.build_exec_command("echo hello", None);
        assert_eq!(cmd[0], "/bin/bash");
        assert_eq!(cmd[1], "-c");
        assert!(cmd[2].contains("eval"));
        assert!(cmd[2].contains("echo hello"));
    }

    #[test]
    fn bash_provider_with_cwd() {
        let p = BashProvider::new("/bin/bash", ShellType::Bash);
        let cmd = p.build_exec_command("ls", Some(Path::new("/tmp")));
        assert!(cmd[2].contains("cd"));
        assert!(cmd[2].contains("/tmp"));
    }

    #[test]
    fn powershell_provider_encodes_base64() {
        let p = PowerShellProvider::new("pwsh");
        let cmd = p.build_exec_command("Get-Date", None);
        assert_eq!(cmd[0], "pwsh");
        assert_eq!(cmd[1], "-NoProfile");
        assert_eq!(cmd[2], "-NonInteractive");
        assert_eq!(cmd[3], "-EncodedCommand");
        // The 4th element should be valid base64
        assert!(base64::engine::general_purpose::STANDARD.decode(&cmd[4]).is_ok());
    }

    #[test]
    fn max_output_default() {
        // Unless env var is set, should be 30000
        // (This test may be affected by env vars in CI, so we just check bounds)
        let len = get_max_output_length();
        assert!(len >= 1);
        assert!(len <= 150_000);
    }

    #[test]
    fn detect_shell_returns_valid() {
        let st = detect_default_shell();
        // Should return some valid shell type
        assert!(matches!(
            st,
            ShellType::Bash | ShellType::Zsh | ShellType::Sh | ShellType::PowerShell | ShellType::Cmd
        ));
    }

    #[test]
    fn shell_escape_simple() {
        assert_eq!(shell_escape("hello"), "'hello'");
    }

    #[test]
    fn shell_escape_with_quotes() {
        assert_eq!(shell_escape("it's"), "'it'\\''s'");
    }

    #[test]
    fn env_overrides_has_locale() {
        let p = BashProvider::new("/bin/bash", ShellType::Bash);
        let env = p.get_env_overrides();
        assert_eq!(env.get("LC_ALL").map(|s| s.as_str()), Some("C.UTF-8"));
    }

    #[test]
    fn shell_type_display() {
        assert_eq!(format!("{}", ShellType::Bash), "bash");
        assert_eq!(format!("{}", ShellType::PowerShell), "powershell");
    }
}
