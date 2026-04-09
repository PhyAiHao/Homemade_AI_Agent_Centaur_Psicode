//! IDE integration — discovers running IDE instances via lockfiles and
//! connects to their WebSocket bridge for bidirectional communication.
//!
//! The VS Code extension writes a lockfile to `~/.centaur-psicode/ide/`
//! with its WebSocket port. This module reads those lockfiles, validates
//! the connection, and provides a channel for sending/receiving messages.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

/// Lockfile written by the IDE extension.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IdeLockfile {
    pub workspace_folders: Vec<String>,
    pub port: u16,
    pub pid: u32,
    pub ide_name: String,
    pub transport: String,
    #[serde(default)]
    pub created_at: String,
}

/// A discovered IDE instance.
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct IdeInstance {
    pub lockfile: IdeLockfile,
    pub lockfile_path: PathBuf,
}

/// Directory where IDE lockfiles are stored.
fn lockfile_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".centaur-psicode")
        .join("ide")
}

/// Discover all running IDE instances that match the given workspace.
pub fn detect_ides(workspace: &Path) -> Vec<IdeInstance> {
    let dir = lockfile_dir();
    if !dir.exists() {
        return Vec::new();
    }

    let workspace_str = workspace.to_string_lossy().to_string();
    let mut instances = Vec::new();

    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => {
            warn!("Failed to read IDE lockfile dir {}: {e}", dir.display());
            return Vec::new();
        }
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }

        match read_and_validate_lockfile(&path, &workspace_str) {
            Ok(Some(instance)) => {
                info!(
                    ide = %instance.lockfile.ide_name,
                    port = instance.lockfile.port,
                    "Found IDE instance"
                );
                instances.push(instance);
            }
            Ok(None) => {
                // Lockfile exists but doesn't match workspace or PID is dead
                debug!("Skipping lockfile {}: no match", path.display());
            }
            Err(e) => {
                debug!("Failed to read lockfile {}: {e}", path.display());
                // Clean up stale lockfile
                let _ = std::fs::remove_file(&path);
            }
        }
    }

    instances
}

/// Read a lockfile, validate the PID is alive and workspace matches.
fn read_and_validate_lockfile(
    path: &Path,
    workspace: &str,
) -> Result<Option<IdeInstance>> {
    let content = std::fs::read_to_string(path)
        .context("Reading lockfile")?;
    let lockfile: IdeLockfile = serde_json::from_str(&content)
        .context("Parsing lockfile JSON")?;

    // Check PID is still alive
    if !is_pid_alive(lockfile.pid) {
        // Process is dead — remove stale lockfile
        let _ = std::fs::remove_file(path);
        return Ok(None);
    }

    // Check workspace matches
    let matches = lockfile.workspace_folders.iter().any(|folder| {
        workspace.starts_with(folder) || folder.starts_with(workspace)
    });

    if !matches {
        return Ok(None);
    }

    // Validate port is reachable
    if !is_port_open(lockfile.port) {
        return Ok(None);
    }

    Ok(Some(IdeInstance {
        lockfile,
        lockfile_path: path.to_path_buf(),
    }))
}

/// Check if a process with the given PID is alive.
fn is_pid_alive(pid: u32) -> bool {
    use nix::sys::signal;
    use nix::unistd::Pid;
    // Sending signal 0 checks if the process exists without actually signaling it
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

/// Check if a TCP port is accepting connections on localhost.
fn is_port_open(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(
        &std::net::SocketAddr::from(([127, 0, 0, 1], port)),
        std::time::Duration::from_millis(500),
    )
    .is_ok()
}

/// Clean up lockfiles for processes that no longer exist.
pub fn cleanup_stale_lockfiles() {
    let dir = lockfile_dir();
    if !dir.exists() {
        return;
    }

    if let Ok(entries) = std::fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if let Ok(content) = std::fs::read_to_string(&path) {
                if let Ok(lockfile) = serde_json::from_str::<IdeLockfile>(&content) {
                    if !is_pid_alive(lockfile.pid) {
                        info!("Removing stale lockfile: {}", path.display());
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
        }
    }
}
