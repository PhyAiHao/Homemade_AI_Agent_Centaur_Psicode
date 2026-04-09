//! Application state — shared mutable state accessible to all tools and the TUI.
//!
//! Mirrors the scattered React context providers in `src/context/` and
//! `src/tasks/`, consolidating them into a single `AppState` behind an
//! `Arc<RwLock<_>>` for safe concurrent access.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

// ─── Task System ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub subject: String,
    pub description: String,
    pub active_form: Option<String>,
    pub status: TaskStatus,
    pub owner: Option<String>,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
    pub metadata: HashMap<String, serde_json::Value>,
    pub output: Option<TaskOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

// ─── Todo System (legacy in-session todo list) ──────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub content: String,
    pub status: TodoStatus,
    pub active_form: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TodoStatus {
    Pending,
    InProgress,
    Completed,
}

// ─── Team System ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamState {
    pub name: String,
    pub description: Option<String>,
    pub lead_agent_id: String,
    pub members: Vec<TeamMember>,
    pub task_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub id: String,
    pub name: String,
    pub role: Option<String>,
    pub color: Option<String>,
    pub status: MemberStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MemberStatus {
    Active,
    Idle,
    Stopped,
}

// ─── Worktree ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeState {
    pub name: String,
    pub path: PathBuf,
    pub original_cwd: PathBuf,
    pub original_root: PathBuf,
    pub branch: String,
}

// ─── Cron Jobs ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub cron: String,
    pub prompt: String,
    pub recurring: bool,
    pub durable: bool,
    pub owner: Option<String>,
    pub human_schedule: String,
}

// ─── AppState ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct AppState {
    // Task system
    pub tasks: HashMap<String, Task>,

    // Todo list (legacy)
    pub todos: Vec<TodoItem>,

    // Team / multi-agent
    pub team: Option<TeamState>,
    pub agent_mailboxes: HashMap<String, Vec<String>>,

    // Worktree
    pub worktree: Option<WorktreeState>,

    // Plan mode
    pub plan_mode: bool,

    // Cron
    pub cron_jobs: HashMap<String, CronJob>,

    // Working directory (can change when entering worktree)
    pub cwd: PathBuf,
    pub project_root: Option<PathBuf>,

    // Session
    pub session_id: String,

    // Notifications
    pub notifications: Vec<Notification>,

    // Stats
    pub stats: SessionStats,

    // Message queue
    pub message_queue: Vec<QueuedMessage>,

    // File state cache — tracks file reads/writes for staleness detection
    pub file_state_cache: crate::file_state_cache::FileStateCache,
}

impl AppState {
    pub fn new(cwd: PathBuf) -> Self {
        AppState {
            tasks: HashMap::new(),
            todos: Vec::new(),
            team: None,
            agent_mailboxes: HashMap::new(),
            worktree: None,
            plan_mode: false,
            cron_jobs: HashMap::new(),
            cwd,
            project_root: None,
            session_id: Uuid::new_v4().to_string(),
            notifications: Vec::new(),
            stats: SessionStats::default(),
            message_queue: Vec::new(),
            file_state_cache: crate::file_state_cache::FileStateCache::new(),
        }
    }

    // ─── Task helpers ───────────────────────────────────────────────────

    pub fn create_task(&mut self, subject: String, description: String, active_form: Option<String>, metadata: HashMap<String, serde_json::Value>) -> String {
        let id = Uuid::new_v4().to_string();
        let task = Task {
            id: id.clone(),
            subject,
            description,
            active_form,
            status: TaskStatus::Pending,
            owner: None,
            blocks: Vec::new(),
            blocked_by: Vec::new(),
            metadata,
            output: None,
        };
        self.tasks.insert(id.clone(), task);
        id
    }

    pub fn get_task(&self, id: &str) -> Option<&Task> {
        self.tasks.get(id)
    }

    pub fn list_active_tasks(&self) -> Vec<&Task> {
        self.tasks.values()
            .filter(|t| t.status != TaskStatus::Deleted)
            .collect()
    }

    // ─── Cron helpers ───────────────────────────────────────────────────

    pub fn add_cron_job(&mut self, job: CronJob) -> String {
        let id = job.id.clone();
        self.cron_jobs.insert(id.clone(), job);
        id
    }

    pub fn remove_cron_job(&mut self, id: &str) -> Option<CronJob> {
        self.cron_jobs.remove(id)
    }

    pub fn list_cron_jobs(&self) -> Vec<&CronJob> {
        self.cron_jobs.values().collect()
    }

    // ─── Mailbox helpers (inter-agent messaging) ────────────────────────

    pub fn send_to_mailbox(&mut self, recipient: &str, message: String) {
        self.agent_mailboxes
            .entry(recipient.to_string())
            .or_default()
            .push(message);
    }

    pub fn drain_mailbox(&mut self, recipient: &str) -> Vec<String> {
        self.agent_mailboxes
            .remove(recipient)
            .unwrap_or_default()
    }
}

/// Thread-safe handle to the shared application state.
pub type SharedState = Arc<RwLock<AppState>>;

/// Create a new shared state instance.
pub fn new_shared_state(cwd: PathBuf) -> SharedState {
    Arc::new(RwLock::new(AppState::new(cwd)))
}

// ─── Notifications ──────────────────────────────────────────────────────────

/// A notification for the TUI status bar.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub id: String,
    pub message: String,
    pub level: NotificationLevel,
    pub timestamp: u64,
    pub dismissed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum NotificationLevel {
    Info,
    Warning,
    Error,
    Success,
}

// ─── Stats ──────────────────────────────────────────────────────────────────

/// Session statistics for display.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionStats {
    pub total_turns: u32,
    pub total_tool_calls: u32,
    pub total_tokens_in: u64,
    pub total_tokens_out: u64,
    pub total_cost_usd: f64,
    pub total_duration_ms: u64,
    pub files_modified: u32,
    pub lines_added: u64,
    pub lines_removed: u64,
}

// ─── Message Queue ──────────────────────────────────────────────────────────

/// Queued messages for deferred delivery (e.g., from hooks, teammates).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedMessage {
    pub id: String,
    pub content: String,
    pub source: String,
    pub timestamp: u64,
    pub is_meta: bool,
}

// Add these fields to AppState for the full S11A implementation.
// In a real incremental build we'd modify AppState directly,
// but since the struct is already defined above, we extend via
// a separate impl block.

impl AppState {
    /// Add a notification.
    pub fn add_notification(&mut self, message: String, level: NotificationLevel) -> String {
        let id = Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.notifications.push(Notification {
            id: id.clone(),
            message,
            level,
            timestamp,
            dismissed: false,
        });
        id
    }

    /// Dismiss a notification by ID.
    pub fn dismiss_notification(&mut self, id: &str) {
        if let Some(n) = self.notifications.iter_mut().find(|n| n.id == id) {
            n.dismissed = true;
        }
    }

    /// Update session stats via a closure.
    pub fn update_stats(&mut self, f: impl FnOnce(&mut SessionStats)) {
        f(&mut self.stats);
    }

    /// Enqueue a message for deferred delivery.
    pub fn enqueue_message(&mut self, content: String, source: String) -> String {
        let id = Uuid::new_v4().to_string();
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.message_queue.push(QueuedMessage {
            id: id.clone(),
            content,
            source,
            timestamp,
            is_meta: false,
        });
        id
    }

    /// Drain all queued messages.
    pub fn drain_message_queue(&mut self) -> Vec<QueuedMessage> {
        std::mem::take(&mut self.message_queue)
    }
}
