//! EnterWorktreeTool — creates an isolated git worktree and switches into it.
//!
//! Mirrors `src/tools/EnterWorktreeTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{SharedState, WorktreeState};

pub struct EnterWorktreeTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for EnterWorktreeTool {
    fn name(&self) -> &'static str { "EnterWorktree" }

    fn description(&self) -> &str {
        "Create an isolated git worktree and switch the session into it. \
         Useful for parallel work without affecting the main checkout."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Worktree name (slug format, e.g., 'fix-auth-bug')"
                }
            }
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let name = input.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("worktree");

        // Validate slug format
        if !name.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_') {
            return Ok(ToolResult::error(
                "Worktree name must be alphanumeric with hyphens/underscores only"
            ));
        }

        let state = self.state.read().await;

        // Check not already in a worktree
        if state.worktree.is_some() {
            return Ok(ToolResult::error("Already in a worktree. Exit first."));
        }

        let original_cwd = state.cwd.clone();
        let session_id = state.session_id.clone();
        drop(state);

        // Find the git repo root
        let repo_root = find_git_root(&original_cwd).await
            .unwrap_or_else(|| original_cwd.clone());

        // Create the worktree
        let worktree_name = format!("{session_id}-{name}");
        let worktree_path = repo_root.join(".worktrees").join(&worktree_name);
        let branch_name = format!("worktree/{name}");

        let output = tokio::process::Command::new("git")
            .args(["worktree", "add", "-b", &branch_name,
                   worktree_path.to_str().unwrap_or(".")])
            .current_dir(&repo_root)
            .output()
            .await;

        match output {
            Ok(out) if out.status.success() => {
                let mut state = self.state.write().await;
                state.worktree = Some(WorktreeState {
                    name: name.to_string(),
                    path: worktree_path.clone(),
                    original_cwd: original_cwd.clone(),
                    original_root: repo_root.clone(),
                    branch: branch_name.clone(),
                });
                state.cwd = worktree_path.clone();

                let _ = output_tx.send(ToolOutput {
                    text: format!("Entered worktree: {}", worktree_path.display()),
                    is_error: false,
                }).await;

                Ok(ToolResult::ok(json!({
                    "worktree_path": worktree_path.to_string_lossy(),
                    "branch": branch_name,
                    "name": name,
                }).to_string()))
            }
            Ok(out) => {
                let stderr = String::from_utf8_lossy(&out.stderr);
                Ok(ToolResult::error(format!("git worktree add failed: {stderr}")))
            }
            Err(e) => Ok(ToolResult::error(format!("Failed to run git: {e}"))),
        }
    }
}

/// Walk up from `dir` to find a `.git` directory.
async fn find_git_root(dir: &std::path::Path) -> Option<PathBuf> {
    let mut current = dir.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}
