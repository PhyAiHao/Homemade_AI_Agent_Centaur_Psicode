//! Remote session manager — manages remote agent sessions.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::util::now_ms;

/// A remote session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSession {
    pub id: String,
    pub url: String,
    pub status: RemoteSessionStatus,
    pub created_at: u64,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum RemoteSessionStatus {
    Connecting,
    Connected,
    Disconnected,
    Error(String),
}

/// Manages multiple remote sessions.
pub struct RemoteSessionManager {
    sessions: HashMap<String, RemoteSession>,
}

impl RemoteSessionManager {
    pub fn new() -> Self {
        RemoteSessionManager { sessions: HashMap::new() }
    }

    /// Create a new remote session.
    pub async fn create(&mut self, url: &str, model: &str) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let session = RemoteSession {
            id: id.clone(),
            url: url.to_string(),
            status: RemoteSessionStatus::Connecting,
            created_at: now_ms(),
            model: model.to_string(),
        };
        self.sessions.insert(id.clone(), session);
        Ok(id)
    }

    /// Connect to a session via WebSocket.
    pub async fn connect(&mut self, session_id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get_mut(session_id) {
            // In production, this would establish a WebSocket connection
            session.status = RemoteSessionStatus::Connected;
            Ok(())
        } else {
            anyhow::bail!("Session not found: {session_id}")
        }
    }

    /// Disconnect a session.
    pub fn disconnect(&mut self, session_id: &str) {
        if let Some(session) = self.sessions.get_mut(session_id) {
            session.status = RemoteSessionStatus::Disconnected;
        }
    }

    /// List all sessions.
    pub fn list(&self) -> Vec<&RemoteSession> {
        self.sessions.values().collect()
    }

    /// Get a session by ID.
    pub fn get(&self, id: &str) -> Option<&RemoteSession> {
        self.sessions.get(id)
    }
}

impl Default for RemoteSessionManager {
    fn default() -> Self { Self::new() }
}

