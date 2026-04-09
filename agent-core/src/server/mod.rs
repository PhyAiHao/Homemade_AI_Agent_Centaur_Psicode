//! Standalone server — HTTP server for direct-connect sessions.
//!
//! Mirrors `src/server/` (3 files). Provides session creation,
//! management, and persistence across server restarts.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::util::now_ms;

/// Server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub auth_required: bool,
    pub max_sessions: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            host: "127.0.0.1".to_string(),
            port: 3000,
            auth_required: true,
            max_sessions: 10,
        }
    }
}

/// A server-managed session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerSession {
    pub id: String,
    pub created_at: u64,
    pub last_active: u64,
    pub model: String,
    pub message_count: usize,
}

/// Manages server sessions.
pub struct SessionManager {
    sessions: Arc<RwLock<HashMap<String, ServerSession>>>,
    config: ServerConfig,
}

impl SessionManager {
    pub fn new(config: ServerConfig) -> Self {
        SessionManager {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Create a new session.
    pub async fn create_session(&self, model: &str) -> Result<String> {
        let mut sessions = self.sessions.write().await;

        if sessions.len() >= self.config.max_sessions {
            anyhow::bail!("Max sessions ({}) reached", self.config.max_sessions);
        }

        let id = uuid::Uuid::new_v4().to_string();
        let now = now_ms();
        sessions.insert(id.clone(), ServerSession {
            id: id.clone(),
            created_at: now,
            last_active: now,
            model: model.to_string(),
            message_count: 0,
        });

        Ok(id)
    }

    /// Get a session.
    pub async fn get_session(&self, id: &str) -> Option<ServerSession> {
        self.sessions.read().await.get(id).cloned()
    }

    /// List all sessions.
    pub async fn list_sessions(&self) -> Vec<ServerSession> {
        self.sessions.read().await.values().cloned().collect()
    }

    /// Remove a session.
    pub async fn remove_session(&self, id: &str) -> bool {
        self.sessions.write().await.remove(id).is_some()
    }
}

