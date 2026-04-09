//! Consolidation lock — prevents concurrent dreams using a lock file.
//!
//! The lock file's mtime IS the timestamp of the last successful consolidation.
//! This is an elegant single-file approach: one stat() call gives both
//! "is locked" and "when last consolidated".

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tracing::{debug, warn};

/// How long before a lock is considered stale (holder probably crashed).
const STALE_THRESHOLD_MS: u64 = 60 * 60 * 1000; // 1 hour

pub struct ConsolidationLock {
    path: PathBuf,
}

impl ConsolidationLock {
    pub fn new(memory_dir: &Path) -> Self {
        Self {
            path: memory_dir.join(".consolidate-lock"),
        }
    }

    /// Read the mtime of the lock file as "last consolidated at" (millis since epoch).
    /// Returns 0 if the file doesn't exist.
    pub fn read_last_consolidated_at(&self) -> u64 {
        match fs::metadata(&self.path) {
            Ok(meta) => meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
            Err(_) => 0,
        }
    }

    /// Try to acquire the lock. Returns the prior mtime (for rollback) on success,
    /// or None if the lock is held by a live process.
    pub fn try_acquire(&self) -> Option<u64> {
        let prior_mtime = self.read_last_consolidated_at();

        // Check if lock exists and is held by a live process
        if self.path.exists() {
            if let Ok(content) = fs::read_to_string(&self.path) {
                if let Ok(pid) = content.trim().parse::<u32>() {
                    if is_pid_alive(pid) {
                        let age_ms = now_millis().saturating_sub(prior_mtime);
                        if age_ms < STALE_THRESHOLD_MS {
                            debug!(pid = pid, "Lock held by live process");
                            return None; // Lock is active
                        }
                        // Lock is stale — holder probably crashed
                        warn!(pid = pid, age_ms = age_ms, "Stealing stale lock");
                    }
                }
            }
        }

        // Write our PID to the lock file
        let our_pid = std::process::id();
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Err(e) = fs::write(&self.path, our_pid.to_string()) {
            warn!("Failed to write lock file: {e}");
            return None;
        }

        // Verify we won the race (re-read)
        match fs::read_to_string(&self.path) {
            Ok(content) if content.trim() == our_pid.to_string() => {
                debug!("Lock acquired (prior mtime: {prior_mtime})");
                Some(prior_mtime)
            }
            _ => {
                debug!("Lost lock race");
                None
            }
        }
    }

    /// Record a successful consolidation by updating the lock file's mtime to now.
    /// Preserves the PID so concurrent-lock detection still works.
    pub fn record_consolidation(&self) {
        let our_pid = std::process::id();
        let _ = fs::write(&self.path, our_pid.to_string());
        debug!("Consolidation recorded (lock mtime updated, pid={our_pid})");
    }

    /// Rollback the lock to the prior mtime (used when dream fails or is killed).
    pub fn rollback(&self, prior_mtime: u64) {
        if prior_mtime == 0 {
            // No prior consolidation — remove the lock file entirely
            let _ = fs::remove_file(&self.path);
            debug!("Lock rolled back (file removed)");
        } else {
            // Restore the prior mtime
            let _ = fs::write(&self.path, "");
            let time = UNIX_EPOCH + Duration::from_millis(prior_mtime);
            let _ = filetime::set_file_mtime(
                &self.path,
                filetime::FileTime::from_system_time(time),
            );
            debug!("Lock rolled back to prior mtime {prior_mtime}");
        }
    }

    /// Count session transcript files modified since `since_ms` (millis since epoch).
    pub fn count_sessions_since(&self, since_ms: u64, transcript_dir: &Path) -> u32 {
        if !transcript_dir.exists() {
            return 0;
        }

        let since_time = UNIX_EPOCH + Duration::from_millis(since_ms);
        let mut count = 0u32;

        if let Ok(entries) = fs::read_dir(transcript_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    if let Ok(meta) = path.metadata() {
                        if let Ok(modified) = meta.modified() {
                            if modified > since_time {
                                count += 1;
                            }
                        }
                    }
                }
            }
        }

        count
    }
}

fn is_pid_alive(pid: u32) -> bool {
    use nix::sys::signal;
    use nix::unistd::Pid;
    signal::kill(Pid::from_raw(pid as i32), None).is_ok()
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_millis() as u64
}
