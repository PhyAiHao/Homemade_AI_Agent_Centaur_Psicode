//! ExitWorktreeTool — exits and optionally removes a git worktree.
//!
//! Mirrors `src/tools/ExitWorktreeTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::SharedState;

pub struct ExitWorktreeTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &'static str { "ExitWorktree" }

    fn description(&self) -> &str {
        "Exit the current worktree session. Choose 'keep' to preserve or 'remove' to clean up."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "Whether to keep or remove the worktree"
                },
                "discard_changes": {
                    "type": "boolean",
                    "description": "Required true if removing a dirty worktree"
                }
            },
            "required": ["action"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let action = input.get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("keep");
        let discard_changes = input.get("discard_changes")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut state = self.state.write().await;

        let wt = match state.worktree.take() {
            Some(w) => w,
            None => return Ok(ToolResult::error("Not in a worktree.")),
        };

        // Restore original CWD
        state.cwd = wt.original_cwd.clone();

        if action == "remove" {
            // Check for dirty state
            let status = tokio::process::Command::new("git")
                .args(["status", "--porcelain"])
                .current_dir(&wt.path)
                .output()
                .await;

            let is_dirty = status.map(|o| !o.stdout.is_empty()).unwrap_or(false);

            if is_dirty && !discard_changes {
                // Restore worktree state — can't remove dirty without explicit flag
                state.worktree = Some(wt);
                return Ok(ToolResult::error(
                    "Worktree has uncommitted changes. Set discard_changes=true to remove anyway."
                ));
            }

            // Remove the worktree
            let _ = tokio::process::Command::new("git")
                .args(["worktree", "remove", "--force",
                       wt.path.to_str().unwrap_or(".")])
                .current_dir(&wt.original_root)
                .output()
                .await;

            // Delete the branch
            let _ = tokio::process::Command::new("git")
                .args(["branch", "-D", &wt.branch])
                .current_dir(&wt.original_root)
                .output()
                .await;

            let _ = output_tx.send(ToolOutput {
                text: format!("Removed worktree \"{}\" and branch {}", wt.name, wt.branch),
                is_error: false,
            }).await;

            Ok(ToolResult::ok(json!({
                "action": "removed",
                "name": wt.name,
                "restored_cwd": wt.original_cwd.to_string_lossy(),
            }).to_string()))
        } else {
            let _ = output_tx.send(ToolOutput {
                text: format!("Exited worktree \"{}\" (kept at {})", wt.name, wt.path.display()),
                is_error: false,
            }).await;

            Ok(ToolResult::ok(json!({
                "action": "kept",
                "name": wt.name,
                "worktree_path": wt.path.to_string_lossy(),
                "restored_cwd": wt.original_cwd.to_string_lossy(),
            }).to_string()))
        }
    }
}
