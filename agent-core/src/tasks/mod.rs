//! Tasks system — full task lifecycle management.
//!
//! Mirrors `src/tasks/` (9 subdirs). Provides task types (local agent,
//! shell, remote), a file-based task list, navigation, and scheduling.
#![allow(dead_code)]

pub mod types;
pub mod runner;
pub mod scheduler;
pub mod watcher;


use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::util::now_ms;

/// Persistent task list stored on disk for cross-session visibility.
#[derive(Debug, Serialize, Deserialize)]
pub struct TaskList {
    pub id: String,
    pub tasks: Vec<TaskEntry>,
    pub path: PathBuf,
}

/// A single task entry in the persistent list.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskEntry {
    pub id: String,
    pub kind: TaskKind,
    pub subject: String,
    pub status: TaskEntryStatus,
    pub owner: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    LocalAgent,
    LocalShell,
    RemoteAgent,
    InProcessTeammate,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskEntryStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl TaskList {
    /// Load a task list from disk.
    pub async fn load(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = tokio::fs::read_to_string(path).await?;
            let list: TaskList = serde_json::from_str(&contents)?;
            Ok(list)
        } else {
            Ok(TaskList {
                id: uuid::Uuid::new_v4().to_string(),
                tasks: Vec::new(),
                path: path.to_path_buf(),
            })
        }
    }

    /// Save the task list to disk.
    pub async fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let contents = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&self.path, contents).await?;
        Ok(())
    }

    /// Add a task.
    pub fn add(&mut self, entry: TaskEntry) {
        self.tasks.push(entry);
    }

    /// Update task status.
    pub fn update_status(&mut self, task_id: &str, status: TaskEntryStatus) {
        if let Some(task) = self.tasks.iter_mut().find(|t| t.id == task_id) {
            task.status = status;
            task.updated_at = now_ms();
        }
    }

    /// Get active (non-completed) tasks.
    pub fn active_tasks(&self) -> Vec<&TaskEntry> {
        self.tasks.iter()
            .filter(|t| t.status != TaskEntryStatus::Completed && t.status != TaskEntryStatus::Cancelled)
            .collect()
    }
}

