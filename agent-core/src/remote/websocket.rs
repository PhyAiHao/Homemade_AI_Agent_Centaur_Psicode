//! WebSocket session protocol — bidirectional communication for remote agents.
//!
//! Mirrors `src/remote/SessionsWebSocket.ts`. Uses tokio-tungstenite for
//! WebSocket connections with automatic reconnection and ping/pong.
#![allow(dead_code)]

use anyhow::Result;
use futures::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, warn};

/// WebSocket connection for a remote session.
pub struct SessionWebSocket {
    url: String,
    reconnect_attempts: u32,
    max_reconnect_attempts: u32,
}

impl SessionWebSocket {
    pub fn new(url: &str) -> Self {
        SessionWebSocket {
            url: url.to_string(),
            reconnect_attempts: 0,
            max_reconnect_attempts: 5,
        }
    }

    /// Connect and start the message loop.
    pub async fn connect(
        &mut self,
        incoming_tx: mpsc::Sender<Value>,
        mut outgoing_rx: mpsc::Receiver<Value>,
    ) -> Result<()> {
        loop {
            match connect_async(&self.url).await {
                Ok((ws_stream, _)) => {
                    self.reconnect_attempts = 0;
                    let (mut write, mut read) = ws_stream.split();

                    loop {
                        tokio::select! {
                            // Incoming messages
                            Some(msg) = read.next() => {
                                match msg {
                                    Ok(Message::Text(text)) => {
                                        if let Ok(value) = serde_json::from_str(&text) {
                                            let _ = incoming_tx.send(value).await;
                                        }
                                    }
                                    Ok(Message::Ping(data)) => {
                                        let _ = write.send(Message::Pong(data)).await;
                                    }
                                    Ok(Message::Close(_)) => break,
                                    Err(e) => {
                                        warn!(error = %e, "WebSocket read error");
                                        break;
                                    }
                                    _ => {}
                                }
                            }
                            // Outgoing messages
                            Some(value) = outgoing_rx.recv() => {
                                let text = serde_json::to_string(&value)?;
                                if write.send(Message::Text(text)).await.is_err() {
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, attempt = self.reconnect_attempts, "WebSocket connection failed");
                }
            }

            // Reconnection logic
            self.reconnect_attempts += 1;
            if self.reconnect_attempts >= self.max_reconnect_attempts {
                anyhow::bail!("Max reconnection attempts ({}) reached", self.max_reconnect_attempts);
            }

            let backoff = std::time::Duration::from_millis(
                100 * 2u64.pow(self.reconnect_attempts.min(5))
            );
            debug!(backoff_ms = ?backoff, "Reconnecting...");
            tokio::time::sleep(backoff).await;
        }
    }
}
