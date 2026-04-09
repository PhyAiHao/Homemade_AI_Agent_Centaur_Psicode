//! Session history — transcript recording for conversation resume.
//!
//! Mirrors `src/assistant/sessionHistory.ts`. Records all messages
//! in a session to `~/.agent/sessions/<id>/transcript.jsonl` for
//! crash recovery and /resume functionality.
#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncWriteExt;

use crate::query::message::{ConversationMessage, Role};

/// A transcript entry — one per message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TranscriptEntry {
    pub timestamp: u64,
    pub role: Role,
    pub content: serde_json::Value,
    /// Tool use ID if this is a tool result.
    #[serde(default)]
    pub tool_use_id: Option<String>,
    /// Model that generated this (for assistant messages).
    #[serde(default)]
    pub model: Option<String>,
    /// API usage (for assistant messages).
    #[serde(default)]
    pub usage: Option<serde_json::Value>,
}

/// Manages a session transcript file.
pub struct SessionTranscript {
    session_id: String,
    transcript_path: PathBuf,
}

impl SessionTranscript {
    /// Create a new session transcript.
    pub async fn new(session_id: &str) -> Result<Self> {
        let dir = crate::config::agent_dir()?
            .join("sessions")
            .join(session_id);
        tokio::fs::create_dir_all(&dir).await.ok();

        Ok(SessionTranscript {
            session_id: session_id.to_string(),
            transcript_path: dir.join("transcript.jsonl"),
        })
    }

    /// Append a message to the transcript.
    pub async fn record(&self, entry: &TranscriptEntry) -> Result<()> {
        let line = serde_json::to_string(entry)
            .context("Serializing transcript entry")?;

        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.transcript_path)
            .await
            .context("Opening transcript file")?;

        file.write_all(line.as_bytes()).await?;
        file.write_all(b"\n").await?;
        file.flush().await?;

        Ok(())
    }

    /// Record a conversation message.
    pub async fn record_message(&self, msg: &ConversationMessage) -> Result<()> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let entry = TranscriptEntry {
            timestamp: now,
            role: msg.role.clone(),
            content: msg.content.clone(),
            tool_use_id: None,
            model: None,
            usage: None,
        };

        self.record(&entry).await
    }

    /// Load all entries from a transcript file (for resume).
    pub async fn load(&self) -> Result<Vec<TranscriptEntry>> {
        if !self.transcript_path.exists() {
            return Ok(Vec::new());
        }

        let contents = tokio::fs::read_to_string(&self.transcript_path).await?;
        let entries: Vec<TranscriptEntry> = contents
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        Ok(entries)
    }

    /// List all session IDs available for resume.
    pub async fn list_sessions() -> Result<Vec<String>> {
        let sessions_dir = crate::config::agent_dir()?
            .join("sessions");

        if !sessions_dir.exists() {
            return Ok(Vec::new());
        }

        let mut sessions = Vec::new();
        let mut entries = tokio::fs::read_dir(&sessions_dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                if let Some(name) = entry.file_name().to_str() {
                    // Only include sessions with a transcript file
                    let transcript = entry.path().join("transcript.jsonl");
                    if transcript.exists() {
                        sessions.push(name.to_string());
                    }
                }
            }
        }

        // Sort by modification time (most recent first)
        sessions.sort();
        sessions.reverse();

        Ok(sessions)
    }

    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    pub fn path(&self) -> &PathBuf {
        &self.transcript_path
    }
}
