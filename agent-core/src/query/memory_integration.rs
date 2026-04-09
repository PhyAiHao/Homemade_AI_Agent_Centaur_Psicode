//! Memory integration — bridges the query loop with the Python memory system.
//!
//! Mirrors the memory integration points from the original:
//! 1. Memory extraction at stop hooks (end of turn)
//! 2. Session memory update after each model response
//! 3. Session memory compaction before full compact
//! 4. Relevant memory prefetch at start of user turn
//! 5. MEMORY.md injection into system prompt

use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, info, warn};

use crate::ipc::{IpcClient, IpcMessage};

// ─── Constants ──────────────────────────────────────────────────────────────

/// Minimum tokens before first session memory extraction.
const SESSION_MEMORY_INIT_THRESHOLD: u64 = 10_000;
/// Minimum token growth between session memory updates.
const SESSION_MEMORY_UPDATE_TOKEN_THRESHOLD: u64 = 5_000;
/// Minimum tool calls between session memory updates.
const SESSION_MEMORY_UPDATE_TOOL_THRESHOLD: u32 = 3;
/// Max lines from core/MEMORY.md to include in system prompt.
/// Reduced from 200 to 80 — only core memories go in the prompt now.
const MEMORY_PROMPT_MAX_LINES: usize = 80;
/// Max bytes from core/MEMORY.md. Reduced from 25KB to 8KB.
const MEMORY_PROMPT_MAX_BYTES: usize = 8_000;

// ─── State tracking ─────────────────────────────────────────────────────────

/// Tracks session memory extraction state across turns.
#[derive(Debug, Default)]
#[allow(dead_code)]
pub struct SessionMemoryState {
    /// Tokens at last extraction.
    pub tokens_at_last_extraction: u64,
    /// Tool calls since last extraction.
    pub tool_calls_since_extraction: u32,
    /// Whether session memory has been initialized.
    pub initialized: bool,
    /// Whether an extraction is currently in progress.
    pub extraction_in_progress: bool,
    /// Keywords from the first user message (for topic-change detection in M4).
    pub initial_topic_keywords: Vec<String>,
    /// The last user message that triggered a prefetch.
    pub last_prefetch_query: String,
    /// How many times the conversation has been compacted (summary-of-summary depth).
    pub compaction_depth: u32,
}

impl SessionMemoryState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if session memory extraction should run.
    pub fn should_extract(&self, current_tokens: u64, has_tool_calls: bool) -> bool {
        if self.extraction_in_progress {
            return false;
        }

        // First extraction: wait until conversation reaches init threshold
        if !self.initialized {
            return current_tokens >= SESSION_MEMORY_INIT_THRESHOLD;
        }

        // Subsequent: check both token growth AND tool call thresholds
        let token_growth = current_tokens.saturating_sub(self.tokens_at_last_extraction);
        let enough_tokens = token_growth >= SESSION_MEMORY_UPDATE_TOKEN_THRESHOLD;
        let enough_tools = self.tool_calls_since_extraction >= SESSION_MEMORY_UPDATE_TOOL_THRESHOLD;

        // Natural break (no tool calls) with enough token growth
        if !has_tool_calls && enough_tokens {
            return true;
        }

        // Both thresholds met
        enough_tokens && enough_tools
    }

    /// Record that extraction happened.
    pub fn record_extraction(&mut self, current_tokens: u64) {
        self.tokens_at_last_extraction = current_tokens;
        self.tool_calls_since_extraction = 0;
        self.initialized = true;
        self.extraction_in_progress = false;
    }

    /// Record tool calls.
    pub fn record_tool_calls(&mut self, count: u32) {
        self.tool_calls_since_extraction += count;
    }

    /// Check if the topic has shifted enough to warrant a re-prefetch of relevant memories.
    /// Uses keyword overlap: if < 30% of current keywords match the initial topic, topic has shifted.
    #[allow(dead_code)]
    pub fn should_reprefetch(&self, current_user_message: &str) -> bool {
        if self.initial_topic_keywords.is_empty() || self.last_prefetch_query.is_empty() {
            return false;
        }
        let current_keywords = extract_keywords(current_user_message);
        if current_keywords.is_empty() {
            return false;
        }
        let overlap = current_keywords.iter()
            .filter(|kw| self.initial_topic_keywords.contains(kw))
            .count();
        let overlap_ratio = overlap as f64 / current_keywords.len() as f64;
        overlap_ratio < 0.30
    }

    /// Record that a prefetch happened for a given query.
    #[allow(dead_code)]
    pub fn record_prefetch(&mut self, query: &str) {
        let keywords = extract_keywords(query);
        if self.initial_topic_keywords.is_empty() {
            self.initial_topic_keywords = keywords.clone();
        }
        self.last_prefetch_query = query.to_string();
    }

    /// Record that compaction happened. Increments the depth counter.
    #[allow(dead_code)]
    pub fn record_compaction(&mut self) {
        self.compaction_depth += 1;
    }
}

/// Extract lowercase keywords (>2 chars) from text for topic comparison.
#[allow(dead_code)]
fn extract_keywords(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|w| w.len() > 2)
        .map(|w| w.to_lowercase())
        .collect()
}

// ─── M1: Memory extraction at stop hooks ────────────────────────────────────

/// Extract durable memories from the conversation (fire-and-forget).
/// Called at the end of each complete turn (when model responds with no tool calls).
///
/// Sends a MemoryRequest(extract, apply=true) to the Python brain, which runs
/// the MemoryExtractor on recent messages and auto-saves candidates.
pub async fn extract_memories_fire_and_forget(
    ipc: &mut IpcClient,
    messages: &[super::message::ConversationMessage],
) {
    let messages_json: Vec<Value> = messages.iter().map(|m| {
        json!({ "role": m.role, "content": m.content })
    }).collect();

    let request = IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        action: "extract".to_string(),
        payload: {
            let mut p = HashMap::new();
            p.insert("messages".to_string(), Value::Array(messages_json));
            p.insert("apply".to_string(), Value::Bool(true));
            // Source lineage: memories extracted from conversation transcripts
            p.insert("source_type".to_string(), Value::String("transcript".to_string()));
            p
        },
    });

    match ipc.request(request).await {
        Ok(IpcMessage::MemoryResponse(resp)) => {
            if resp.ok {
                debug!("Memory extraction completed");
            } else if let Some(err) = resp.error {
                warn!(error = %err, "Memory extraction failed");
            }
        }
        Ok(_) => {}
        Err(e) => {
            warn!(error = %e, "Memory extraction IPC failed");
        }
    }
}

// ─── M2: Session memory update (post-sampling hook) ─────────────────────────

/// Update session memory after a model response.
/// Called after each API response when thresholds are met.
pub async fn update_session_memory(
    ipc: &mut IpcClient,
    messages: &[super::message::ConversationMessage],
) {
    let messages_json: Vec<Value> = messages.iter()
        .rev()
        .take(20) // Only send recent messages for session update
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(|m| json!({ "role": m.role, "content": m.content }))
        .collect();

    let request = IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        action: "session_update".to_string(),
        payload: {
            let mut p = HashMap::new();
            p.insert("messages".to_string(), Value::Array(messages_json));
            p
        },
    });

    match ipc.request(request).await {
        Ok(_) => debug!("Session memory updated"),
        Err(e) => warn!(error = %e, "Session memory update failed"),
    }
}

// ─── M3: Session memory compaction ──────────────────────────────────────────

/// Try session-memory-based compaction BEFORE full LLM compaction.
///
/// This reads the session memory file and uses it as a zero-LLM-call summary,
/// keeping only the most recent messages verbatim.
///
/// Returns Some((summary, kept_messages)) on success, None if unavailable.
pub async fn try_session_memory_compact(
    ipc: &mut IpcClient,
    messages: &[super::message::ConversationMessage],
    _token_budget: u32,
) -> Option<(String, Vec<Value>)> {
    // 1. Get current session memory
    let get_request = IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        action: "session_get".to_string(),
        payload: HashMap::new(),
    });

    let session_memory = match ipc.request(get_request).await {
        Ok(IpcMessage::MemoryResponse(resp)) if resp.ok => {
            resp.payload.get("content")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
        }
        _ => None,
    }?;

    if session_memory.trim().is_empty() {
        debug!("Session memory empty — falling back to full compact");
        return None;
    }

    // Staleness check: if session memory was extracted from messages still in
    // context verbatim, skip injecting it to avoid duplication.
    // Heuristic: check if the first 100 chars of session memory appear in any message.
    let probe = session_memory.chars().take(100).collect::<String>();
    let still_in_context = messages.iter().any(|m| {
        let text = m.text_content();
        text.contains(&probe)
    });
    if still_in_context {
        debug!("Session memory still present verbatim in context — skipping to avoid duplication");
        return None;
    }

    // 2. Calculate how many recent messages to keep (at least 10K tokens worth)
    let min_keep_tokens: u64 = 10_000;
    let mut keep_from = messages.len();
    let mut accumulated_tokens: u64 = 0;

    for i in (0..messages.len()).rev() {
        let msg = &messages[i];
        let msg_tokens = super::compact::estimate_tokens(std::slice::from_ref(msg));
        accumulated_tokens += msg_tokens;
        keep_from = i;
        if accumulated_tokens >= min_keep_tokens {
            break;
        }
    }

    // Keep at least 5 messages
    keep_from = keep_from.min(messages.len().saturating_sub(5));

    let kept_messages: Vec<Value> = messages[keep_from..].iter().map(|m| {
        json!({ "role": m.role, "content": m.content })
    }).collect();

    let summary = format!(
        "Session Memory (extracted from conversation):\n\n{session_memory}"
    );

    // Note: Callers should check SessionMemoryState.compaction_depth.
    // If depth > 2, consider injecting a warning into the system prompt:
    // "Note: older context has been summarized multiple times and may be imprecise."

    info!(
        kept = kept_messages.len(),
        snipped = keep_from,
        "Session memory compact: using pre-extracted session memory as summary"
    );

    Some((summary, kept_messages))
}

// ─── M4: Relevant memory prefetch ──────────────────────────────────────────

/// Maximum bytes of memory text to inject via M4 prefetch.
/// Caps total injection to ~2,500 tokens regardless of how many memories match.
const M4_MAX_INJECTION_BYTES: usize = 8_000;

/// Maximum characters per individual memory preview in M4 injection.
const M4_MAX_PER_MEMORY_CHARS: usize = 500;

/// Prefetch relevant memories based on the user's latest message.
/// Returns formatted memory text to inject as context, or None.
///
/// Python recall returns: `{"query": "...", "memories": [MemoryRecord, ...]}`
/// Each MemoryRecord has: `{"metadata": {"name", "memory_type", "description", ...}, "body": "...", "path": "..."}`
pub async fn prefetch_relevant_memories(
    ipc: &mut IpcClient,
    user_message: &str,
) -> Option<String> {
    let request = IpcMessage::MemoryRequest(crate::ipc::MemoryRequest {
        request_id: crate::ipc::IpcClient::new_request_id(),
        action: "recall".to_string(),
        payload: {
            let mut p = HashMap::new();
            p.insert("query".to_string(), Value::String(user_message.to_string()));
            p.insert("limit".to_string(), json!(5));
            p
        },
    });

    match ipc.request(request).await {
        Ok(IpcMessage::MemoryResponse(resp)) if resp.ok => {
            // Python returns {"memories": [MemoryRecord, ...]}
            let memories = resp.payload.get("memories")
                .and_then(|v| v.as_array())?;

            if memories.is_empty() {
                debug!("M4 recall returned no memories");
                return None;
            }

            // Format each memory into a concise text block, capped per-memory and total
            let mut result = String::from("# Relevant Memories from Past Conversations\n\n");
            let mut total_bytes = result.len();

            for mem in memories {
                let metadata = mem.get("metadata").unwrap_or(mem);
                let name = metadata.get("name")
                    .and_then(|v| v.as_str()).unwrap_or("Untitled");
                let mem_type = metadata.get("memory_type")
                    .and_then(|v| v.as_str()).unwrap_or("unknown");
                let description = metadata.get("description")
                    .and_then(|v| v.as_str()).unwrap_or("");
                let body = mem.get("body")
                    .and_then(|v| v.as_str()).unwrap_or("");

                // Truncate body preview
                let body_preview: String = if body.len() > M4_MAX_PER_MEMORY_CHARS {
                    format!("{}...", &body[..M4_MAX_PER_MEMORY_CHARS])
                } else {
                    body.to_string()
                };

                let entry = format!(
                    "**[{mem_type}] {name}**{desc}\n{body_preview}\n\n",
                    desc = if description.is_empty() { String::new() } else { format!(" — {description}") },
                );

                if total_bytes + entry.len() > M4_MAX_INJECTION_BYTES {
                    result.push_str("_(more memories available via MemoryRecall tool)_\n");
                    break;
                }

                total_bytes += entry.len();
                result.push_str(&entry);
            }

            if result.trim().len() > 50 {
                info!(memories = memories.len(), bytes = total_bytes, "M4: relevant memories recalled");
                Some(result)
            } else {
                None
            }
        }
        Ok(IpcMessage::MemoryResponse(resp)) => {
            if let Some(err) = resp.error {
                warn!(error = %err, "M4 recall failed");
            }
            None
        }
        _ => None,
    }
}

// ─── M5: MEMORY.md injection into system prompt ─────────────────────────────

/// Load memory prompt for injection into the system prompt.
/// Prefers core/MEMORY.md (small, curated) over the full MEMORY.md.
/// Falls back to legacy paths for backwards compatibility.
pub fn load_memory_prompt(cwd: &Path) -> Option<String> {
    let candidates = [
        // Prefer core tier index (small, always-relevant memories)
        dirs::home_dir()
            .map(|h| h.join(".agent").join("memory").join("core").join("MEMORY.md"))
            .unwrap_or_default(),
        // Fall back to legacy paths
        cwd.join(".claude").join("MEMORY.md"),
        dirs::home_dir()
            .map(|h| h.join(".claude").join("MEMORY.md"))
            .unwrap_or_default(),
    ];

    for path in &candidates {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(content) if !content.trim().is_empty() => {
                    let truncated = truncate_memory_content(&content);
                    return Some(format!(
                        "# Memory\n\
                         The following is your persistent memory from previous conversations.\n\
                         Use it to recall context, preferences, and project state.\n\n\
                         {truncated}"
                    ));
                }
                _ => continue,
            }
        }
    }
    None
}

/// Truncate MEMORY.md content to fit within limits.
fn truncate_memory_content(content: &str) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = String::new();

    for (i, line) in lines.iter().enumerate() {
        if i >= MEMORY_PROMPT_MAX_LINES || result.len() + line.len() > MEMORY_PROMPT_MAX_BYTES {
            result.push_str(&format!(
                "\n... [MEMORY.md truncated at line {}/{}, {}/{} bytes]",
                i, lines.len(), result.len(), content.len()
            ));
            break;
        }
        result.push_str(line);
        result.push('\n');
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_memory_state_init_threshold() {
        let state = SessionMemoryState::new();
        assert!(!state.should_extract(5_000, false));
        assert!(state.should_extract(10_000, false));
    }

    #[test]
    fn test_session_memory_state_update_threshold() {
        let mut state = SessionMemoryState::new();
        state.initialized = true;
        state.tokens_at_last_extraction = 10_000;
        state.tool_calls_since_extraction = 3;

        // Not enough token growth
        assert!(!state.should_extract(12_000, true));

        // Enough growth + enough tools
        assert!(state.should_extract(15_000, true));
    }

    #[test]
    fn test_session_memory_natural_break() {
        let mut state = SessionMemoryState::new();
        state.initialized = true;
        state.tokens_at_last_extraction = 10_000;
        state.tool_calls_since_extraction = 0;

        // Natural break (no tool calls) with enough tokens
        assert!(state.should_extract(15_000, false));

        // Natural break but not enough tokens
        assert!(!state.should_extract(12_000, false));
    }

    #[test]
    fn test_truncate_memory() {
        let content = "Line 1\nLine 2\nLine 3\n";
        let result = truncate_memory_content(content);
        assert!(result.contains("Line 1"));
    }
}
