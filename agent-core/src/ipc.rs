//! IPC client — Rust side of the Rust ↔ Python bridge.
//!
//! Transport: Unix domain socket at `$AGENT_IPC_SOCKET` (default `/tmp/agent-ipc.sock`).
//! Framing:   4-byte **big-endian** length prefix followed by msgpack payload.
//!
//! All message types match `agent-brain/agent_brain/ipc_types.py` exactly.
//! Field names here MUST stay in sync with the Python Pydantic models.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tracing::{debug, warn};
use uuid::Uuid;

// ── Message types ────────────────────────────────────────────────────────────
// Field names match agent-brain/agent_brain/ipc_types.py EXACTLY.

/// Every message sent over the IPC socket.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcMessage {
    // Rust → Python
    ApiRequest(ApiRequest),
    ToolResult(ToolResult),
    MemoryRequest(MemoryRequest),
    CompactRequest(CompactRequest),
    SkillRequest(SkillRequest),
    VoiceStart(VoiceStart),
    OutputStyleRequest(OutputStyleRequest),
    CostRequest(CostRequest),
    IpcPing(IpcPing),

    // Python → Rust
    TextDelta(TextDelta),
    ToolUse(ToolUse),
    MessageDone(MessageDone),
    MemoryResponse(MemoryResponse),
    CompactResponse(CompactResponse),
    SkillResponse(SkillResponse),
    VoiceTranscript(VoiceTranscript),
    OutputStyleResponse(OutputStyleResponse),
    CostResponse(CostResponse),
    IpcPong(IpcPong),
}

// ── Rust → Python ─────────────────────────────────────────────────────────

/// Matches Python `ipc_types.ApiRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiRequest {
    pub request_id:       String,
    pub model:            String,
    pub messages:         Vec<serde_json::Value>,
    #[serde(default)]
    pub tools:            Vec<serde_json::Value>,
    /// Python field: `system_prompt`
    pub system_prompt:    Option<serde_json::Value>,
    pub max_output_tokens: Option<u32>,
    #[serde(default)]
    pub metadata:         HashMap<String, serde_json::Value>,
    pub tool_choice:      Option<serde_json::Value>,
    /// Python expects dict or None, NOT bool
    pub thinking:         Option<serde_json::Value>,
    #[serde(default)]
    pub betas:            Vec<String>,
    #[serde(default = "default_provider")]
    pub provider:         String,
    pub api_key:          Option<String>,
    pub base_url:         Option<String>,
    #[serde(default)]
    pub fast_mode:        bool,
}

fn default_provider() -> String { "first_party".to_string() }

/// Matches Python `ipc_types.ToolResult`.
/// Note: Python uses `tool_call_id` and `output` (not `tool_use_id`/`content`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub request_id:   String,
    pub tool_call_id: String,
    pub output:       serde_json::Value,
    #[serde(default)]
    pub is_error:     bool,
}

/// Matches Python `ipc_types.MemoryRequest`.
/// Python uses `action` + `payload` (not `operation`/`path`/`content`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRequest {
    pub request_id: String,
    pub action:     String,
    #[serde(default)]
    pub payload:    HashMap<String, serde_json::Value>,
}

/// Matches Python `ipc_types.CompactRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactRequest {
    pub request_id:   String,
    pub messages:     Vec<serde_json::Value>,
    pub token_budget: Option<u32>,
}

/// Matches Python `ipc_types.SkillRequest`.
/// Python uses `arguments` (not `args`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillRequest {
    pub request_id: String,
    pub skill_name: String,
    #[serde(default)]
    pub arguments:  HashMap<String, serde_json::Value>,
}

/// Matches Python `ipc_types.VoiceStart`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceStart {
    pub request_id:      String,
    pub language:        Option<String>,
    pub audio_b64:       Option<String>,
    pub audio_path:      Option<String>,
    #[serde(default)]
    pub keyterms:        Vec<String>,
    #[serde(default)]
    pub recent_files:    Vec<String>,
    pub project_dir:     Option<String>,
    pub branch_name:     Option<String>,
    pub transcript_hint: Option<String>,
}

/// Matches Python `ipc_types.OutputStyleRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleRequest {
    pub request_id: String,
    pub style_name: String,
}

/// Matches Python `ipc_types.CostRequest`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostRequest {
    pub request_id: String,
    #[serde(default)]
    pub reset:      bool,
}

// ── Python → Rust ─────────────────────────────────────────────────────────

/// Matches Python `ipc_types.TextDelta`.
/// Note: Python does NOT send an `index` field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextDelta {
    pub request_id: String,
    pub delta:      String,
}

/// Matches Python `ipc_types.ToolUse`.
/// Python uses `tool_call_id` (not `tool_use_id`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolUse {
    pub request_id:      String,
    pub tool_call_id:    String,
    pub name:            String,
    #[serde(default)]
    pub input:           serde_json::Value,
    #[serde(default)]
    pub server_tool_use: bool,
}

/// Matches Python `ipc_types.MessageDone`.
/// `stop_reason` is a free-form string (or null), not a strict enum.
/// `usage` is a dict (not a struct with required fields).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDone {
    pub request_id:  String,
    #[serde(default)]
    pub usage:       HashMap<String, serde_json::Value>,
    pub stop_reason: Option<String>,
}

/// Matches Python `ipc_types.MemoryResponse`.
/// Python uses `ok` + `payload` + `error`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryResponse {
    pub request_id: String,
    pub ok:         bool,
    #[serde(default)]
    pub payload:    HashMap<String, serde_json::Value>,
    pub error:      Option<String>,
}

/// Matches Python `ipc_types.CompactResponse`.
/// Python sends `summary` + `messages` (not `tokens_before`/`tokens_after`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactResponse {
    pub request_id: String,
    #[serde(default)]
    pub summary:    String,
    #[serde(default)]
    pub messages:   Vec<serde_json::Value>,
}

/// Matches Python `ipc_types.SkillResponse`.
/// Python sends `content` + `metadata` (not `expanded_prompt`/`found`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillResponse {
    pub request_id: String,
    #[serde(default)]
    pub content:    String,
    #[serde(default)]
    pub metadata:   HashMap<String, serde_json::Value>,
}

/// Matches Python `ipc_types.VoiceTranscript`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceTranscript {
    pub request_id: String,
    pub text:       String,
    #[serde(default = "default_true")]
    pub is_final:   bool,
    #[serde(default)]
    pub metadata:   HashMap<String, serde_json::Value>,
}

fn default_true() -> bool { true }

/// Matches Python `ipc_types.OutputStyleResponse`.
/// Python uses `style` (not `definition`/`found`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputStyleResponse {
    pub request_id: String,
    #[serde(default)]
    pub style:      HashMap<String, serde_json::Value>,
}

/// Matches Python `ipc_types.CostResponse`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CostResponse {
    pub request_id:  String,
    #[serde(default)]
    pub usage:       HashMap<String, serde_json::Value>,
    #[serde(default)]
    pub diagnostics: HashMap<String, serde_json::Value>,
}

/// Heartbeat ping — Rust sends to verify Python brain is alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPing {
    pub request_id: String,
}

/// Heartbeat pong — Python responds confirming it is alive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPong {
    pub request_id: String,
    #[serde(default)]
    pub status:     String,
    #[serde(default)]
    pub uptime_ms:  u64,
}

// ── IPC Client ───────────────────────────────────────────────────────────────

/// Async IPC client — manages the Unix socket connection to agent-brain.
pub struct IpcClient {
    socket_path: PathBuf,
    stream: Option<UnixStream>,
}

impl IpcClient {
    /// Create a new client and immediately connect to the given socket path.
    /// Used for fire-and-forget background tasks that need their own connection.
    pub async fn connect_to(socket_path: &str) -> Result<Self> {
        let mut client = IpcClient {
            socket_path: PathBuf::from(socket_path),
            stream: None,
        };
        client.connect().await?;
        Ok(client)
    }

    /// Create a new client with the default socket path and connect immediately.
    /// Convenience for background tasks (e.g., dream) that need their own connection.
    pub async fn new_connected() -> Result<Self> {
        let path = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
        Self::connect_to(&path).await
    }

    pub fn new() -> Self {
        let socket_path = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into())
            .into();
        IpcClient { socket_path, stream: None }
    }

    /// Connect to the Python IPC server, retrying with exponential backoff.
    pub async fn connect(&mut self) -> Result<()> {
        let max_attempts = 10;
        let mut delay = Duration::from_millis(100);

        for attempt in 1..=max_attempts {
            match UnixStream::connect(&self.socket_path).await {
                Ok(stream) => {
                    debug!("IPC connected to {:?}", self.socket_path);
                    self.stream = Some(stream);
                    return Ok(());
                }
                Err(e) if attempt < max_attempts => {
                    warn!("IPC connect attempt {attempt}/{max_attempts} failed: {e}. Retrying in {delay:?}");
                    tokio::time::sleep(delay).await;
                    delay = (delay * 2).min(Duration::from_secs(5));
                }
                Err(e) => {
                    bail!("IPC connect failed after {max_attempts} attempts: {e}. Is agent-brain running? (make dev-python)");
                }
            }
        }
        unreachable!()
    }

    /// Send a message and stream back responses until `MessageDone`.
    /// Uses a dedicated connection per streaming request.
    pub async fn send_streaming(
        &mut self,
        msg: IpcMessage,
    ) -> Result<mpsc::Receiver<Result<IpcMessage>>> {
        let socket_path = self.socket_path.clone();
        let mut stream = UnixStream::connect(&socket_path).await
            .context("Opening dedicated IPC connection for streaming request")?;

        send_message(&mut stream, &msg).await?;

        let (tx, rx) = mpsc::channel(64);

        tokio::spawn(async move {
            loop {
                match recv_message(&mut stream).await {
                    Ok(incoming) => {
                        let is_terminal = matches!(
                            &incoming,
                            IpcMessage::MessageDone(_)
                        );
                        let _ = tx.send(Ok(incoming)).await;
                        if is_terminal { break; }
                    }
                    Err(e) => {
                        let _ = tx.send(Err(e)).await;
                        break;
                    }
                }
            }
        });

        Ok(rx)
    }

    /// Send a request and wait for a single (non-streaming) response.
    /// Uses per-request-type timeouts instead of a flat 60s.
    pub async fn request(&mut self, msg: IpcMessage) -> Result<IpcMessage> {
        let timeout_secs = Self::timeout_for_message(&msg);
        self.request_with_timeout(msg, Duration::from_secs(timeout_secs)).await
    }

    /// Send a request with a custom timeout.
    pub async fn request_with_timeout(&mut self, msg: IpcMessage, timeout: Duration) -> Result<IpcMessage> {
        let stream = self.stream.as_mut().context("IPC not connected")?;
        send_message(stream, &msg).await?;
        tokio::time::timeout(timeout, recv_message(stream))
            .await
            .context(format!("IPC request timed out ({timeout:?}) — is agent-brain running?"))?
    }

    /// Send a heartbeat ping. Returns Ok(uptime_ms) or error if brain is unresponsive.
    #[allow(dead_code)]
    pub async fn ping(&mut self) -> Result<u64> {
        let msg = IpcMessage::IpcPing(IpcPing {
            request_id: Self::new_request_id(),
        });
        let resp = self.request_with_timeout(msg, Duration::from_secs(5)).await?;
        match resp {
            IpcMessage::IpcPong(pong) => Ok(pong.uptime_ms),
            other => bail!("Expected IpcPong, got: {:?}", other),
        }
    }

    /// Per-request-type timeout in seconds.
    fn timeout_for_message(msg: &IpcMessage) -> u64 {
        match msg {
            IpcMessage::IpcPing(_)             => 5,
            IpcMessage::CostRequest(_)         => 10,
            IpcMessage::OutputStyleRequest(_)  => 10,
            IpcMessage::SkillRequest(_)        => 30,
            IpcMessage::MemoryRequest(req) => {
                match req.action.as_str() {
                    "dream_consolidate" => 300,
                    "crewai_run" => 600,
                    "wiki_ingest" => 300,
                    "wiki_query" => 120,
                    "wiki_lint" => 30,
                    _ => 30,
                }
            }
            IpcMessage::CompactRequest(_)      => 60,
            IpcMessage::VoiceStart(_)          => 60,
            // API requests use streaming, not this method, but just in case:
            IpcMessage::ApiRequest(_)          => 120,
            _                                  => 60,
        }
    }

    /// Generate a new request ID (UUID v4).
    pub fn new_request_id() -> String {
        Uuid::new_v4().to_string()
    }
}

// ── Wire protocol ─────────────────────────────────────────────────────────
// Framing: [4-byte BIG-ENDIAN length][msgpack payload]
// MUST match agent-brain/agent_brain/ipc_wire.py (BYTE_ORDER = "big").

/// Write a message to the stream: [4-byte BE length][msgpack payload]
async fn send_message(stream: &mut UnixStream, msg: &IpcMessage) -> Result<()> {
    let payload = rmp_serde::to_vec_named(msg)
        .context("Serializing IPC message to msgpack")?;

    let len = payload.len() as u32;
    stream.write_all(&len.to_be_bytes()).await?;  // BIG-endian
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}

/// Read a message from the stream: read 4-byte BE length, then payload.
async fn recv_message(stream: &mut UnixStream) -> Result<IpcMessage> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await
        .context("Reading IPC message length")?;

    let len = u32::from_be_bytes(len_buf) as usize;  // BIG-endian
    if len == 0 || len > 128 * 1024 * 1024 {
        bail!("IPC message length out of bounds: {len}");
    }

    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload).await
        .context("Reading IPC message payload")?;

    let msg: IpcMessage = rmp_serde::from_slice(&payload)
        .context("Deserializing IPC message from msgpack")?;

    Ok(msg)
}

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_request_serialization() {
        let msg = IpcMessage::ApiRequest(ApiRequest {
            request_id: "test-123".into(),
            model: "claude-sonnet-4-6".into(),
            messages: vec![],
            tools: vec![],
            system_prompt: Some(serde_json::Value::String("You are a helpful assistant.".into())),
            max_output_tokens: Some(8192),
            metadata: HashMap::new(),
            tool_choice: None,
            thinking: None,
            betas: vec![],
            provider: "first_party".into(),
            api_key: None,
            base_url: None,
            fast_mode: false,
        });

        let bytes = rmp_serde::to_vec_named(&msg).unwrap();
        let decoded: IpcMessage = rmp_serde::from_slice(&bytes).unwrap();

        if let IpcMessage::ApiRequest(req) = decoded {
            assert_eq!(req.request_id, "test-123");
            assert_eq!(req.model, "claude-sonnet-4-6");
        } else {
            panic!("Wrong variant after decode");
        }
    }

    #[test]
    fn test_text_delta_serialization() {
        let msg = IpcMessage::TextDelta(TextDelta {
            request_id: "req-1".into(),
            delta: "Hello".into(),
        });
        let bytes = rmp_serde::to_vec_named(&msg).unwrap();
        let decoded: IpcMessage = rmp_serde::from_slice(&bytes).unwrap();
        assert!(matches!(decoded, IpcMessage::TextDelta(_)));
    }

    #[test]
    fn test_all_variants_serialize() {
        let messages: Vec<IpcMessage> = vec![
            IpcMessage::ToolResult(ToolResult {
                request_id: "r".into(), tool_call_id: "t".into(),
                output: serde_json::json!("ok"), is_error: false,
            }),
            IpcMessage::MemoryRequest(MemoryRequest {
                request_id: "r".into(), action: "load".into(),
                payload: HashMap::new(),
            }),
            IpcMessage::CompactRequest(CompactRequest {
                request_id: "r".into(),
                messages: vec![], token_budget: Some(4096),
            }),
            IpcMessage::SkillRequest(SkillRequest {
                request_id: "r".into(), skill_name: "commit".into(),
                arguments: HashMap::new(),
            }),
            IpcMessage::MessageDone(MessageDone {
                request_id: "r".into(),
                stop_reason: Some("end_turn".into()),
                usage: HashMap::new(),
            }),
        ];

        for msg in messages {
            let bytes = rmp_serde::to_vec_named(&msg).unwrap();
            let _decoded: IpcMessage = rmp_serde::from_slice(&bytes).unwrap();
        }
    }

    #[test]
    fn test_big_endian_framing() {
        // Verify the length encoding matches Python's big-endian convention
        let len: u32 = 256;
        let bytes = len.to_be_bytes();
        assert_eq!(bytes, [0x00, 0x00, 0x01, 0x00]);
        assert_eq!(u32::from_be_bytes(bytes), 256);
    }
}
