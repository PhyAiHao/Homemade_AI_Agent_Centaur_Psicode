//! Task watcher — monitors the task list file for changes.

use std::path::PathBuf;
use tokio::sync::mpsc;

/// Watches the task list file and notifies on changes.
pub struct TaskWatcher {
    path: PathBuf,
    running: bool,
}

impl TaskWatcher {
    pub fn new(path: PathBuf) -> Self {
        TaskWatcher { path, running: false }
    }

    /// Start watching (polls for file modification time changes).
    pub async fn watch(&mut self, notify_tx: mpsc::Sender<()>) {
        self.running = true;
        let mut last_modified = tokio::fs::metadata(&self.path)
            .await
            .and_then(|m| m.modified())
            .ok();

        loop {
            if !self.running { break; }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;

            if let Ok(meta) = tokio::fs::metadata(&self.path).await {
                if let Ok(modified) = meta.modified() {
                    if last_modified.map(|lm| modified > lm).unwrap_or(true) {
                        last_modified = Some(modified);
                        let _ = notify_tx.send(()).await;
                    }
                }
            }
        }
    }

    pub fn stop(&mut self) {
        self.running = false;
    }
}
