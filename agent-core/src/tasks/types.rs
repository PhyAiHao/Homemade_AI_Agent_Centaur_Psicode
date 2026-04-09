//! Task types — local agent, shell, remote, in-process.

use serde::{Deserialize, Serialize};

/// Configuration for spawning a local agent task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalAgentConfig {
    pub prompt: String,
    pub model: Option<String>,
    pub working_dir: Option<String>,
    pub background: bool,
}

/// Configuration for a shell task (background command).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShellTaskConfig {
    pub command: String,
    pub working_dir: Option<String>,
    pub timeout_ms: Option<u64>,
}

/// Configuration for a remote agent task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteAgentConfig {
    pub session_url: String,
    pub prompt: String,
    pub auth_token: Option<String>,
}
