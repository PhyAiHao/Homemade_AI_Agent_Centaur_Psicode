#![allow(dead_code)] // Bridge features wired but not all consumed yet
//! Bridge client — WebSocket connection to IDE extension.
//!
//! Connects to the VS Code extension's WebSocket server and forwards
//! QueryEvents from the agent engine to the IDE, and user messages
//! from the IDE back to the engine.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use futures::stream::StreamExt;
use futures::sink::SinkExt;
use tracing::{debug, info, warn};

use crate::ide_integration::IdeInstance;
use crate::query::query_loop::QueryEvent;

/// Message sent from agent → IDE.
#[derive(Debug, Serialize)]
#[serde(tag = "type")]
enum OutboundMessage {
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },
    #[serde(rename = "assistant_message")]
    AssistantMessage { content: String },
    #[serde(rename = "tool_start")]
    ToolStart { id: String, name: String },
    #[serde(rename = "tool_done")]
    ToolDone { id: String, name: String, is_error: bool },
    #[serde(rename = "done")]
    Done { reason: String },
    #[serde(rename = "error")]
    Error { message: String },
    #[serde(rename = "request_permission")]
    RequestPermission {
        request_id: String,
        tool_name: String,
        description: String,
    },
    #[serde(rename = "file_changed")]
    FileChanged { path: String },
}

/// Message received from IDE → agent.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum InboundMessage {
    #[serde(rename = "user_message")]
    UserMessage { content: String },
    #[serde(rename = "permission_response")]
    PermissionResponse {
        request_id: String,
        approved: bool,
        #[serde(default)]
        always: bool,
    },
}

/// A live connection to the IDE extension.
pub struct BridgeConnection {
    /// Send user messages from the IDE to the engine.
    pub user_message_rx: mpsc::Receiver<String>,
    /// Send query events to forward to the IDE.
    event_tx: mpsc::Sender<QueryEvent>,
}

impl BridgeConnection {
    /// Get a clone of the event sender for the fanout task.
    pub fn event_tx_clone(&self) -> mpsc::Sender<QueryEvent> {
        self.event_tx.clone()
    }

    /// Connect to an IDE instance and start the message forwarding loop.
    ///
    /// Returns a `BridgeConnection` with channels for bidirectional communication.
    /// The WebSocket I/O runs on a background tokio task.
    pub async fn connect(ide: &IdeInstance) -> Result<Self> {
        let url = format!("ws://127.0.0.1:{}", ide.lockfile.port);
        info!(url = %url, ide = %ide.lockfile.ide_name, "Connecting to IDE bridge");

        let (ws_stream, _) = connect_async(&url)
            .await
            .context("Failed to connect to IDE WebSocket")?;

        let (mut ws_sink, mut ws_source) = ws_stream.split();

        // Channel: IDE → engine (user messages)
        let (user_tx, user_rx) = mpsc::channel::<String>(64);
        // Channel: engine → IDE (query events to forward)
        let (event_tx, mut event_rx) = mpsc::channel::<QueryEvent>(256);

        // Background task: read from WebSocket, write to WebSocket
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    // Forward query events to IDE via WebSocket
                    Some(event) = event_rx.recv() => {
                        if let Some(msg) = query_event_to_outbound(&event) {
                            let json = match serde_json::to_string(&msg) {
                                Ok(j) => j,
                                Err(e) => {
                                    warn!("Failed to serialize outbound message: {e}");
                                    continue;
                                }
                            };
                            if let Err(e) = ws_sink.send(Message::Text(json.into())).await {
                                warn!("WebSocket send error: {e}");
                                break;
                            }
                        }
                    }
                    // Receive messages from IDE
                    msg = ws_source.next() => {
                        match msg {
                            Some(Ok(Message::Text(text))) => {
                                match serde_json::from_str::<InboundMessage>(&text) {
                                    Ok(InboundMessage::UserMessage { content }) => {
                                        let _ = user_tx.send(content).await;
                                    }
                                    Ok(InboundMessage::PermissionResponse { .. }) => {
                                        // TODO: route permission responses to the gate
                                        debug!("Received permission response from IDE");
                                    }
                                    Err(e) => {
                                        debug!("Unknown inbound message: {e}");
                                    }
                                }
                            }
                            Some(Ok(Message::Close(_))) | None => {
                                info!("IDE WebSocket closed");
                                break;
                            }
                            Some(Err(e)) => {
                                warn!("WebSocket read error: {e}");
                                break;
                            }
                            _ => {} // ping/pong/binary
                        }
                    }
                }
            }
        });

        Ok(BridgeConnection {
            user_message_rx: user_rx,
            event_tx,
        })
    }

    /// Forward a query event to the IDE.
    pub async fn forward_event(&self, event: QueryEvent) {
        let _ = self.event_tx.send(event).await;
    }
}

/// Convert a QueryEvent to an outbound WebSocket message.
fn query_event_to_outbound(event: &QueryEvent) -> Option<OutboundMessage> {
    match event {
        QueryEvent::TextDelta(text) => Some(OutboundMessage::TextDelta {
            delta: text.clone(),
        }),
        QueryEvent::AssistantMessage(msg) => Some(OutboundMessage::AssistantMessage {
            content: msg.text_content(),
        }),
        QueryEvent::ToolStart { id, name } => Some(OutboundMessage::ToolStart {
            id: id.clone(),
            name: name.clone(),
        }),
        QueryEvent::ToolDone { id, result } => Some(OutboundMessage::ToolDone {
            id: id.clone(),
            name: String::new(), // name not available in ToolDone
            is_error: result.is_error,
        }),
        QueryEvent::Done(reason) => Some(OutboundMessage::Done {
            reason: reason.display().to_string(),
        }),
        QueryEvent::Error(msg) => Some(OutboundMessage::Error {
            message: msg.clone(),
        }),
        // Don't forward every delta/update to avoid flooding
        QueryEvent::ThinkingDelta(_)
        | QueryEvent::ToolOutput(_)
        | QueryEvent::UsageUpdate { .. }
        | QueryEvent::RetryWait { .. }
        | QueryEvent::Compacted { .. } => None,
    }
}
