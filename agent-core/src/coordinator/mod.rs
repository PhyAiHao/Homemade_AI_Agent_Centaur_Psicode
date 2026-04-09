//! Coordinator — multi-agent coordination mode.
//!
//! Mirrors `src/coordinator/coordinatorMode.ts`. Manages the lifecycle
//! of coordinated agent sessions, worker tools, and context injection.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::state::SharedState;

/// Coordinator mode configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoordinatorConfig {
    /// Whether coordinator mode is active.
    pub enabled: bool,
    /// Maximum concurrent workers.
    pub max_workers: usize,
    /// Worker model override.
    pub worker_model: Option<String>,
    /// Coordinator prompt prefix.
    pub prompt_prefix: Option<String>,
}

impl Default for CoordinatorConfig {
    fn default() -> Self {
        CoordinatorConfig {
            enabled: false,
            max_workers: 4,
            worker_model: None,
            prompt_prefix: None,
        }
    }
}

/// A coordinated worker agent.
#[derive(Debug, Clone)]
pub struct WorkerAgent {
    pub id: String,
    pub name: String,
    pub status: WorkerStatus,
    pub task_description: String,
    pub pid: Option<u32>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum WorkerStatus {
    Pending,
    Running,
    Completed,
    Failed(String),
}

/// The coordinator manages a pool of worker agents.
pub struct Coordinator {
    pub config: CoordinatorConfig,
    pub workers: Vec<WorkerAgent>,
    state: SharedState,
}

impl Coordinator {
    pub fn new(config: CoordinatorConfig, state: SharedState) -> Self {
        Coordinator {
            config,
            workers: Vec::new(),
            state,
        }
    }

    /// Spawn a new worker agent with a task.
    pub async fn spawn_worker(&mut self, name: &str, task: &str) -> Result<String> {
        if self.workers.iter().filter(|w| w.status == WorkerStatus::Running).count()
            >= self.config.max_workers
        {
            anyhow::bail!("Max workers ({}) reached", self.config.max_workers);
        }

        let id = uuid::Uuid::new_v4().to_string();

        let exe = std::env::current_exe().unwrap_or_else(|_| "agent".into());
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("--bare").arg(task);
        if let Some(ref model) = self.config.worker_model {
            cmd.arg("--model").arg(model);
        }

        let child = cmd.spawn()?;
        let pid = child.id();

        self.workers.push(WorkerAgent {
            id: id.clone(),
            name: name.to_string(),
            status: WorkerStatus::Running,
            task_description: task.to_string(),
            pid,
        });

        Ok(id)
    }

    /// Get the status of all workers.
    pub fn worker_status(&self) -> Vec<&WorkerAgent> {
        self.workers.iter().collect()
    }

    /// Get a worker by ID.
    pub fn get_worker(&self, id: &str) -> Option<&WorkerAgent> {
        self.workers.iter().find(|w| w.id == id)
    }

    /// Mark a worker as completed.
    pub fn complete_worker(&mut self, id: &str) {
        if let Some(w) = self.workers.iter_mut().find(|w| w.id == id) {
            w.status = WorkerStatus::Completed;
        }
    }

    /// Mark a worker as failed.
    pub fn fail_worker(&mut self, id: &str, reason: &str) {
        if let Some(w) = self.workers.iter_mut().find(|w| w.id == id) {
            w.status = WorkerStatus::Failed(reason.to_string());
        }
    }

    /// Count running workers.
    pub fn running_count(&self) -> usize {
        self.workers.iter().filter(|w| w.status == WorkerStatus::Running).count()
    }

    /// Is coordinator mode active?
    pub fn is_active(&self) -> bool {
        self.config.enabled
    }
}
