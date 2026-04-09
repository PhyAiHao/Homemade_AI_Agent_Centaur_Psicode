//! File history — backup and versioning of files edited by the agent.
//!
//! Mirrors `src/utils/fileHistory.ts`. Maintains backup snapshots of
//! files before edits for safe undo / checkpoint recovery.
#![allow(dead_code)]

use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Maximum number of snapshots to retain.
const MAX_SNAPSHOTS: usize = 100;

/// A backup of a single file.
#[derive(Debug, Clone)]
pub struct FileBackup {
    pub backup_path: PathBuf,
    pub version: u32,
    pub backup_time: u64,
}

/// A snapshot capturing file states at a point in time (tied to a message ID).
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub message_id: String,
    pub tracked_file_backups: HashMap<PathBuf, FileBackup>,
    pub timestamp: u64,
}

/// Manages file edit history and checkpointing.
pub struct FileHistory {
    snapshots: Vec<Snapshot>,
    tracked_files: HashSet<PathBuf>,
    snapshot_sequence: u32,
    backup_dir: PathBuf,
    enabled: bool,
}

impl FileHistory {
    /// Create a new file history manager.
    pub fn new(session_dir: &Path) -> Self {
        let backup_dir = session_dir.join("file_backups");
        let enabled = std::env::var("AGENT_FILE_HISTORY")
            .map(|v| v != "0" && v.to_lowercase() != "false")
            .unwrap_or(true);

        FileHistory {
            snapshots: Vec::new(),
            tracked_files: HashSet::new(),
            snapshot_sequence: 0,
            backup_dir,
            enabled,
        }
    }

    /// Track a file edit — backup the file before modification.
    pub async fn track_edit(&mut self, file_path: &Path, message_id: &str) -> Result<()> {
        if !self.enabled || !file_path.exists() {
            return Ok(());
        }

        // Create backup directory if needed
        tokio::fs::create_dir_all(&self.backup_dir).await?;

        // Read current content
        let content = tokio::fs::read(file_path).await?;

        // Generate backup filename
        let version = self.next_version(file_path);
        let file_name = file_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let backup_name = format!("{}.v{}.bak", file_name, version);
        let backup_path = self.backup_dir.join(&backup_name);

        // Write backup
        tokio::fs::write(&backup_path, &content).await?;

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let backup = FileBackup {
            backup_path: backup_path.clone(),
            version,
            backup_time: now,
        };

        // Add to current snapshot or create new one
        let canonical = file_path.canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());

        self.tracked_files.insert(canonical.clone());

        // Find or create snapshot for this message
        if let Some(snap) = self.snapshots.iter_mut()
            .find(|s| s.message_id == message_id)
        {
            snap.tracked_file_backups.insert(canonical, backup);
        } else {
            let mut backups = HashMap::new();
            backups.insert(canonical, backup);
            self.snapshots.push(Snapshot {
                message_id: message_id.to_string(),
                tracked_file_backups: backups,
                timestamp: now,
            });
            self.snapshot_sequence += 1;

            // Evict oldest if over limit
            if self.snapshots.len() > MAX_SNAPSHOTS {
                let removed = self.snapshots.remove(0);
                // Clean up backup files
                for backup in removed.tracked_file_backups.values() {
                    let _ = tokio::fs::remove_file(&backup.backup_path).await;
                }
            }
        }

        Ok(())
    }

    /// Get the latest backup for a file.
    pub fn latest_backup(&self, file_path: &Path) -> Option<&FileBackup> {
        let canonical = file_path.canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());

        self.snapshots.iter().rev()
            .find_map(|s| s.tracked_file_backups.get(&canonical))
    }

    /// Restore a file from its latest backup.
    pub async fn restore(&self, file_path: &Path) -> Result<bool> {
        if let Some(backup) = self.latest_backup(file_path) {
            if backup.backup_path.exists() {
                let content = tokio::fs::read(&backup.backup_path).await?;
                tokio::fs::write(file_path, content).await?;
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Get the next version number for a file.
    fn next_version(&self, file_path: &Path) -> u32 {
        let canonical = file_path.canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());

        self.snapshots.iter()
            .flat_map(|s| s.tracked_file_backups.get(&canonical))
            .map(|b| b.version)
            .max()
            .unwrap_or(0)
            + 1
    }

    /// Get the number of tracked files.
    pub fn tracked_file_count(&self) -> usize {
        self.tracked_files.len()
    }

    /// Get the number of snapshots.
    pub fn snapshot_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Is file history enabled?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }
}
