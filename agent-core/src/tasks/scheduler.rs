//! Task scheduler — runs cron jobs and scheduled tasks.

use tokio::sync::mpsc;
use std::time::Duration;

use crate::state::SharedState;

/// The task scheduler checks cron jobs on a periodic basis.
pub struct TaskScheduler {
    state: SharedState,
    check_interval: Duration,
    running: bool,
}

impl TaskScheduler {
    pub fn new(state: SharedState) -> Self {
        TaskScheduler {
            state,
            check_interval: Duration::from_secs(60),
            running: false,
        }
    }

    /// Start the scheduler loop (runs as a background tokio task).
    pub async fn run(&mut self, prompt_tx: mpsc::Sender<String>) {
        self.running = true;

        loop {
            if !self.running { break; }
            tokio::time::sleep(self.check_interval).await;

            let state = self.state.read().await;
            let jobs: Vec<_> = state.cron_jobs.values().cloned().collect();
            drop(state);

            for job in &jobs {
                if self.should_fire(job) {
                    let _ = prompt_tx.send(job.prompt.clone()).await;

                    if !job.recurring {
                        let mut state = self.state.write().await;
                        state.remove_cron_job(&job.id);
                    }
                }
            }
        }
    }

    /// Stop the scheduler.
    pub fn stop(&mut self) {
        self.running = false;
    }

    /// Simple cron check — in production, use the `cron` crate for full parsing.
    fn should_fire(&self, _job: &crate::state::CronJob) -> bool {
        // Simplified: always false until proper cron matching is implemented.
        // The `cron` crate in Cargo.toml can be used for this.
        false
    }
}
