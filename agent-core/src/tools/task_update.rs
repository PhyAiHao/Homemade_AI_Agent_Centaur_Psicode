//! TaskUpdateTool — updates task fields.
//!
//! Mirrors `src/tools/TaskUpdateTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::state::{SharedState, TaskStatus};

pub struct TaskUpdateTool {
    pub state: SharedState,
}

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &'static str { "TaskUpdate" }

    fn description(&self) -> &str {
        "Update task fields including status, subject, description, owner, and dependencies."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "taskId": {
                    "type": "string",
                    "description": "Task ID to update"
                },
                "subject": {
                    "type": "string",
                    "description": "New subject"
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                },
                "activeForm": {
                    "type": "string",
                    "description": "Active spinner form"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "New status"
                },
                "addBlocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs this task blocks"
                },
                "addBlockedBy": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that block this task"
                },
                "owner": {
                    "type": "string",
                    "description": "New owner name"
                },
                "metadata": {
                    "type": "object",
                    "description": "Metadata to merge"
                }
            },
            "required": ["taskId"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let task_id = input.get("taskId")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if task_id.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: taskId"));
        }

        let mut state = self.state.write().await;
        let task = match state.tasks.get_mut(task_id) {
            Some(t) => t,
            None => return Ok(ToolResult::error(format!("Task not found: {task_id}"))),
        };

        let mut updated_fields = Vec::new();

        if let Some(subject) = input.get("subject").and_then(|v| v.as_str()) {
            task.subject = subject.to_string();
            updated_fields.push("subject");
        }
        if let Some(desc) = input.get("description").and_then(|v| v.as_str()) {
            task.description = desc.to_string();
            updated_fields.push("description");
        }
        if let Some(af) = input.get("activeForm").and_then(|v| v.as_str()) {
            task.active_form = Some(af.to_string());
            updated_fields.push("activeForm");
        }
        if let Some(status_str) = input.get("status").and_then(|v| v.as_str()) {
            task.status = match status_str {
                "pending" => TaskStatus::Pending,
                "in_progress" => TaskStatus::InProgress,
                "completed" => TaskStatus::Completed,
                "deleted" => TaskStatus::Deleted,
                _ => return Ok(ToolResult::error(format!("Invalid status: {status_str}"))),
            };
            updated_fields.push("status");
        }
        if let Some(owner) = input.get("owner").and_then(|v| v.as_str()) {
            task.owner = Some(owner.to_string());
            updated_fields.push("owner");
        }
        if let Some(blocks) = input.get("addBlocks").and_then(|v| v.as_array()) {
            for b in blocks {
                if let Some(id) = b.as_str() {
                    if !task.blocks.contains(&id.to_string()) {
                        task.blocks.push(id.to_string());
                    }
                }
            }
            updated_fields.push("blocks");
        }
        if let Some(blocked_by) = input.get("addBlockedBy").and_then(|v| v.as_array()) {
            for b in blocked_by {
                if let Some(id) = b.as_str() {
                    if !task.blocked_by.contains(&id.to_string()) {
                        task.blocked_by.push(id.to_string());
                    }
                }
            }
            updated_fields.push("blockedBy");
        }
        if let Some(meta) = input.get("metadata").and_then(|v| v.as_object()) {
            for (k, v) in meta {
                task.metadata.insert(k.clone(), v.clone());
            }
            updated_fields.push("metadata");
        }

        let subject = task.subject.clone();
        let _ = output_tx.send(ToolOutput {
            text: format!("Updated task {task_id} ({subject}): {}", updated_fields.join(", ")),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(json!({
            "id": task_id,
            "updated_fields": updated_fields,
        }).to_string()))
    }
}
