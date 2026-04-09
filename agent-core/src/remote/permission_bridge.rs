//! Remote permission bridge — forwards permission prompts to remote clients.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

/// A permission request sent to the remote client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePermissionRequest {
    pub id: String,
    pub tool_name: String,
    pub tool_input: String,
    pub session_id: String,
}

/// A permission response from the remote client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemotePermissionResponse {
    pub id: String,
    pub approved: bool,
    pub always: bool,
}

/// Bridge for forwarding permission decisions over the remote session.
pub struct PermissionBridge {
    pending: std::collections::HashMap<String, tokio::sync::oneshot::Sender<RemotePermissionResponse>>,
}

impl PermissionBridge {
    pub fn new() -> Self {
        PermissionBridge { pending: std::collections::HashMap::new() }
    }

    /// Request permission from the remote client.
    pub async fn request_permission(
        &mut self,
        tool_name: &str,
        tool_input: &str,
        session_id: &str,
        send_fn: impl FnOnce(RemotePermissionRequest),
    ) -> RemotePermissionResponse {
        let id = uuid::Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.insert(id.clone(), tx);

        send_fn(RemotePermissionRequest {
            id: id.clone(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.to_string(),
            session_id: session_id.to_string(),
        });

        rx.await.unwrap_or(RemotePermissionResponse {
            id, approved: false, always: false,
        })
    }

    /// Handle a permission response from the remote client.
    pub fn handle_response(&mut self, response: RemotePermissionResponse) {
        if let Some(tx) = self.pending.remove(&response.id) {
            let _ = tx.send(response);
        }
    }
}

impl Default for PermissionBridge {
    fn default() -> Self { Self::new() }
}
