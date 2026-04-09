//! Swarm execution — backends for running multiple agents.
//!
//! Mirrors `src/utils/swarm/` (9 files + backends/).
//! Provides different execution strategies: in-process, tmux, iTerm.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Swarm execution backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmBackend {
    /// Run agents in the same process (tokio tasks).
    InProcess,
    /// Run agents in tmux panes.
    Tmux,
    /// Run agents in iTerm2 tabs (macOS only).
    Iterm,
}

/// Configuration for a swarm execution.
#[derive(Debug, Clone)]
pub struct SwarmConfig {
    pub backend: SwarmBackend,
    pub max_concurrent: usize,
    pub working_dir: PathBuf,
    pub model: String,
}

impl Default for SwarmConfig {
    fn default() -> Self {
        SwarmConfig {
            backend: SwarmBackend::InProcess,
            max_concurrent: 4,
            working_dir: std::env::current_dir().unwrap_or_default(),
            model: "claude-sonnet-4-6".to_string(),
        }
    }
}

/// Detect the best available swarm backend.
pub fn detect_backend() -> SwarmBackend {
    // Check for tmux
    if std::env::var("TMUX").is_ok() {
        return SwarmBackend::Tmux;
    }

    // Check for iTerm2 (macOS)
    if cfg!(target_os = "macos")
        && std::env::var("ITERM_SESSION_ID").is_ok() {
            return SwarmBackend::Iterm;
        }

    SwarmBackend::InProcess
}

/// Spawn an agent in the configured backend.
pub async fn spawn_agent(
    config: &SwarmConfig,
    name: &str,
    prompt: &str,
) -> Result<SpawnedAgent> {
    match config.backend {
        SwarmBackend::InProcess => spawn_in_process(config, name, prompt).await,
        SwarmBackend::Tmux => spawn_tmux(config, name, prompt).await,
        SwarmBackend::Iterm => spawn_in_process(config, name, prompt).await, // Fallback
    }
}

/// A spawned agent handle.
#[derive(Debug)]
pub struct SpawnedAgent {
    pub name: String,
    pub pid: Option<u32>,
    pub backend: SwarmBackend,
}

async fn spawn_in_process(config: &SwarmConfig, name: &str, prompt: &str) -> Result<SpawnedAgent> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "agent".into());
    let child = tokio::process::Command::new(&exe)
        .arg("--bare")
        .arg("--model").arg(&config.model)
        .arg("--dir").arg(&config.working_dir)
        .arg(prompt)
        .spawn()?;

    Ok(SpawnedAgent {
        name: name.to_string(),
        pid: child.id(),
        backend: SwarmBackend::InProcess,
    })
}

async fn spawn_tmux(config: &SwarmConfig, name: &str, prompt: &str) -> Result<SpawnedAgent> {
    let exe = std::env::current_exe().unwrap_or_else(|_| "agent".into());
    let cmd = format!("{} --bare --model {} --dir {} '{}'",
        exe.display(), config.model, config.working_dir.display(), prompt.replace('\'', "'\\''"));

    tokio::process::Command::new("tmux")
        .args(["split-window", "-h", "-t", ".", &cmd])
        .output()
        .await?;

    Ok(SpawnedAgent {
        name: name.to_string(),
        pid: None,
        backend: SwarmBackend::Tmux,
    })
}
