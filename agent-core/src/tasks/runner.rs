//! Task runner — executes tasks and tracks their lifecycle.

use anyhow::Result;
use std::collections::HashMap;
use tokio::process::Child;


/// Manages running tasks and their processes.
pub struct TaskRunner {
    /// Active child processes by task ID.
    processes: HashMap<String, Child>,
}

impl TaskRunner {
    pub fn new() -> Self {
        TaskRunner { processes: HashMap::new() }
    }

    /// Start a shell task.
    pub async fn start_shell(&mut self, task_id: &str, command: &str, cwd: Option<&str>) -> Result<()> {
        let mut cmd = tokio::process::Command::new("sh");
        cmd.args(["-c", command]);
        if let Some(dir) = cwd {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn()?;
        self.processes.insert(task_id.to_string(), child);
        Ok(())
    }

    /// Start a local agent task.
    pub async fn start_agent(&mut self, task_id: &str, prompt: &str, model: Option<&str>) -> Result<()> {
        let exe = std::env::current_exe().unwrap_or_else(|_| "agent".into());
        let mut cmd = tokio::process::Command::new(&exe);
        cmd.arg("--bare").arg(prompt);
        if let Some(m) = model {
            cmd.arg("--model").arg(m);
        }
        let child = cmd.spawn()?;
        self.processes.insert(task_id.to_string(), child);
        Ok(())
    }

    /// Stop a running task.
    pub async fn stop(&mut self, task_id: &str) -> Result<bool> {
        if let Some(mut child) = self.processes.remove(task_id) {
            child.kill().await?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Check if a task is still running.
    pub fn is_running(&mut self, task_id: &str) -> bool {
        if let Some(child) = self.processes.get_mut(task_id) {
            child.try_wait().ok().flatten().is_none()
        } else {
            false
        }
    }

    /// Get the number of running tasks.
    pub fn running_count(&self) -> usize {
        self.processes.len()
    }
}

impl Default for TaskRunner {
    fn default() -> Self { Self::new() }
}
