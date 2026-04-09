//! Conversation history — JSONL persistence for prompt history.
//!
//! Mirrors `src/history.ts`. Stores user prompts as JSONL in
//! `~/.agent/history.jsonl` for Up-arrow recall and session resume.
#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

/// A single entry in the history file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    /// User-visible display text (for recall).
    pub display: String,
    /// Unix timestamp (ms).
    pub timestamp: u64,
    /// Project path (for scoping).
    pub project: String,
    /// Session ID (for grouping).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Pasted file contents (inline or hash-ref).
    #[serde(default)]
    pub pasted_contents: Vec<PastedContent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PastedContent {
    pub id: u32,
    #[serde(rename = "type")]
    pub content_type: String,  // "text" | "image"
    pub content: Option<String>,
    pub content_hash: Option<String>,
    pub media_type: Option<String>,
    pub filename: Option<String>,
}

/// Path to the global history file.
fn history_path() -> Result<PathBuf> {
    let dir = crate::config::agent_dir()?;
    Ok(dir.join("history.jsonl"))
}

/// Append a prompt to the history file.
pub async fn add_to_history(entry: &HistoryEntry) -> Result<()> {
    let path = history_path()?;

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.ok();
    }

    let line = serde_json::to_string(entry)
        .context("Serializing history entry")?;

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .context("Opening history file")?;

    file.write_all(line.as_bytes()).await?;
    file.write_all(b"\n").await?;
    file.flush().await?;

    Ok(())
}

/// Read history entries (most recent first, up to `limit`).
pub async fn read_history(limit: usize) -> Result<Vec<HistoryEntry>> {
    let path = history_path()?;

    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = tokio::fs::read_to_string(&path)
        .await
        .context("Reading history file")?;

    let entries: Vec<HistoryEntry> = contents
        .lines()
        .rev()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .take(limit)
        .collect();

    // Already reversed (most recent first)
    Ok(entries)
}

/// Read history entries for a specific project.
pub async fn read_project_history(project: &str, limit: usize) -> Result<Vec<HistoryEntry>> {
    let all = read_history(limit * 3).await?;  // over-fetch then filter
    Ok(all.into_iter()
        .filter(|e| e.project == project)
        .take(limit)
        .collect())
}

/// Remove the most recent entry (undo on interrupt).
pub async fn remove_last() -> Result<()> {
    let path = history_path()?;
    if !path.exists() {
        return Ok(());
    }

    let contents = tokio::fs::read_to_string(&path).await?;
    let mut lines: Vec<&str> = contents.lines().collect();

    if lines.is_empty() {
        return Ok(());
    }

    lines.pop(); // Remove last line
    let new_contents = lines.join("\n") + "\n";
    tokio::fs::write(&path, new_contents).await?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_entry_serialize() {
        let entry = HistoryEntry {
            display: "hello world".to_string(),
            timestamp: 1234567890,
            project: "/tmp/test".to_string(),
            session_id: Some("abc-123".to_string()),
            pasted_contents: Vec::new(),
        };
        let json = serde_json::to_string(&entry).unwrap();
        let parsed: HistoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.display, "hello world");
        assert_eq!(parsed.project, "/tmp/test");
    }
}
