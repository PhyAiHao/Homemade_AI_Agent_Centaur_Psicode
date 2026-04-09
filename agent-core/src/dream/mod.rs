#![allow(dead_code)] // Dream feature — wired but not all paths called yet
//! Dream — automatic background memory consolidation.
//!
//! Like sleeping, "dreaming" consolidates short-term session experiences
//! into long-term organized memories. After enough sessions accumulate
//! (default: 5 sessions over 24 hours), a background agent:
//!
//!   1. Orients — reads existing memory files + MEMORY.md index
//!   2. Gathers — scans recent session transcripts for new signal
//!   3. Consolidates — writes/updates topic memory files
//!   4. Prunes — maintains MEMORY.md index (max 200 lines)
//!
//! The dream runs as a fire-and-forget background task, invisible to the
//! user's conversation. It can be killed from the TUI via `/dream stop`.

pub mod config;
pub mod lock;
pub mod prompt;
pub mod task;

use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, info, warn};

use crate::ipc::IpcClient;

use self::config::DreamConfig;
use self::lock::ConsolidationLock;

/// State tracked across the session for dream gating.
pub struct DreamState {
    /// Configuration (min hours, min sessions, enabled).
    config: DreamConfig,
    /// Lock file manager.
    lock: ConsolidationLock,
    /// Memory directory root.
    memory_dir: PathBuf,
    /// Session transcript directory.
    transcript_dir: PathBuf,
    /// Last time we scanned for eligible sessions (anti-spam throttle).
    last_scan_at: u64,
    /// Whether a dream is currently running.
    is_running: bool,
}

/// Minimum interval between session scans (anti-spam).
const SESSION_SCAN_INTERVAL_MS: u64 = 10 * 60 * 1000; // 10 minutes

impl DreamState {
    pub fn new(memory_dir: PathBuf, transcript_dir: PathBuf) -> Self {
        let lock = ConsolidationLock::new(&memory_dir);
        Self {
            config: DreamConfig::default(),
            lock,
            memory_dir,
            transcript_dir,
            last_scan_at: 0,
            is_running: false,
        }
    }

    /// Check all gates and run dream if eligible. Called after each turn.
    pub async fn maybe_dream(&mut self, ipc: &mut IpcClient) {
        if self.is_running || !self.config.enabled {
            return;
        }

        // Gate 1: Time gate (cheapest check)
        let last_consolidated = self.lock.read_last_consolidated_at();
        let now_ms = now_millis();
        let hours_since = (now_ms.saturating_sub(last_consolidated)) as f64 / 3_600_000.0;

        if hours_since < self.config.min_hours as f64 {
            debug!(hours_since = hours_since, min = self.config.min_hours,
                   "Dream time gate: not enough time elapsed");
            return;
        }

        // Gate 2: Scan throttle (anti-spam)
        if now_ms.saturating_sub(self.last_scan_at) < SESSION_SCAN_INTERVAL_MS {
            return;
        }
        self.last_scan_at = now_ms;

        // Gate 3: Session gate (count new sessions since last dream)
        let sessions_since = self.lock.count_sessions_since(
            last_consolidated, &self.transcript_dir
        );
        if sessions_since < self.config.min_sessions {
            debug!(sessions = sessions_since, min = self.config.min_sessions,
                   "Dream session gate: not enough sessions");
            return;
        }

        // Gate 4: Lock gate (prevent concurrent dreams)
        let prior_mtime = match self.lock.try_acquire() {
            Some(mtime) => mtime,
            None => {
                debug!("Dream lock gate: lock held by another process");
                return;
            }
        };

        // All gates passed — run the dream
        info!(hours_since = hours_since, sessions = sessions_since,
              "Dream gates passed — starting consolidation");

        self.is_running = true;
        let result = self.run_dream(ipc, sessions_since).await;

        match result {
            Ok(()) => {
                info!("Dream completed successfully");
                self.lock.record_consolidation();
            }
            Err(e) => {
                warn!("Dream failed: {e}");
                self.lock.rollback(prior_mtime);
            }
        }

        self.is_running = false;
    }

    /// Execute the consolidation via IPC to the Python brain.
    async fn run_dream(&self, ipc: &mut IpcClient, sessions_reviewed: u32) -> Result<()> {
        let consolidation_prompt = prompt::build_consolidation_prompt(
            &self.memory_dir,
            &self.transcript_dir,
            sessions_reviewed,
        );

        info!("Dream: sending consolidation prompt to brain ({} chars)", consolidation_prompt.len());

        // Send as a special compact/memory request through IPC
        let mut payload = std::collections::HashMap::new();
        payload.insert("prompt".to_string(), serde_json::Value::String(consolidation_prompt));
        payload.insert("memory_dir".to_string(), serde_json::Value::String(self.memory_dir.to_string_lossy().to_string()));
        payload.insert("transcript_dir".to_string(), serde_json::Value::String(self.transcript_dir.to_string_lossy().to_string()));
        payload.insert("sessions_reviewed".to_string(), serde_json::json!(sessions_reviewed));

        let request = crate::ipc::IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
            request_id: crate::ipc::IpcClient::new_request_id(),
            action: "dream_consolidate".to_string(),
            payload,
        });

        let response = ipc.request(request).await?;

        // Validate the response — don't silently ignore failures
        match &response {
            crate::ipc::IpcMessage::MemoryResponse(mem_resp) => {
                if !mem_resp.ok {
                    let err_msg = mem_resp.error.as_deref().unwrap_or("unknown error");
                    anyhow::bail!("Dream consolidation failed: {err_msg}");
                }
                let ops = mem_resp.payload.get("operations_applied")
                    .and_then(|v| v.as_u64()).unwrap_or(0);
                info!("Dream consolidation succeeded: {ops} memory operations applied");
            }
            other => {
                warn!("Dream got unexpected response type: {:?}", other);
                anyhow::bail!("Dream consolidation returned unexpected IPC message type");
            }
        }
        Ok(())
    }

    /// Force-trigger a dream (for `/dream` command).
    pub async fn force_dream(&mut self, ipc: &mut IpcClient) -> Result<String> {
        if self.is_running {
            return Ok("A dream is already running.".to_string());
        }

        let prior_mtime = match self.lock.try_acquire() {
            Some(m) => m,
            None => return Ok("Dream lock held by another process. Try again later.".to_string()),
        };

        let sessions = self.lock.count_sessions_since(0, &self.transcript_dir);

        self.is_running = true;
        let result = self.run_dream(ipc, sessions).await;

        match result {
            Ok(()) => {
                self.lock.record_consolidation();
                self.is_running = false;
                Ok(format!("Dream completed. Reviewed {} sessions, memories consolidated.", sessions))
            }
            Err(e) => {
                self.lock.rollback(prior_mtime);
                self.is_running = false;
                Ok(format!("Dream failed: {e}"))
            }
        }
    }

    pub fn is_running(&self) -> bool {
        self.is_running
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}
