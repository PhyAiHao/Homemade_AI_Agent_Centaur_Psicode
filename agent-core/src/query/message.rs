//! Conversation message types — the wire format for API communication.
//!
//! These types match the Anthropic Messages API format.
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Message role.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    User,
    Assistant,
    System,
}

/// A content block within a message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Plain text content.
    Text { text: String },

    /// Tool use request from the assistant.
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    /// Tool result sent back to the API.
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },

    /// Thinking block (extended thinking / chain-of-thought).
    Thinking {
        thinking: String,
    },

    /// Image content.
    Image {
        source: ImageSource,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageSource {
    #[serde(rename = "type")]
    pub source_type: String,  // "base64"
    pub media_type: String,   // "image/png"
    pub data: String,         // base64-encoded
}

/// A single message in the conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub role: Role,
    pub content: Value,
}

impl ConversationMessage {
    /// Create a user message with text.
    pub fn user_text(text: impl Into<String>) -> Self {
        ConversationMessage {
            role: Role::User,
            content: serde_json::json!([{
                "type": "text",
                "text": text.into()
            }]),
        }
    }

    /// Create an assistant message with text.
    pub fn assistant_text(text: impl Into<String>) -> Self {
        ConversationMessage {
            role: Role::Assistant,
            content: serde_json::json!([{
                "type": "text",
                "text": text.into()
            }]),
        }
    }

    /// Create a user message containing tool results.
    pub fn tool_results(results: Vec<ToolResultBlock>) -> Self {
        let content: Vec<Value> = results.into_iter().map(|r| {
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": r.tool_use_id,
                "content": r.content,
                "is_error": r.is_error,
            })
        }).collect();

        ConversationMessage {
            role: Role::User,
            content: Value::Array(content),
        }
    }

    /// Extract tool_use blocks from an assistant message.
    pub fn tool_use_blocks(&self) -> Vec<ToolUseBlock> {
        let blocks = match &self.content {
            Value::Array(arr) => arr,
            _ => return Vec::new(),
        };

        blocks.iter().filter_map(|b| {
            if b.get("type")?.as_str()? == "tool_use" {
                Some(ToolUseBlock {
                    id: b.get("id")?.as_str()?.to_string(),
                    name: b.get("name")?.as_str()?.to_string(),
                    input: b.get("input")?.clone(),
                })
            } else {
                None
            }
        }).collect()
    }

    /// Extract text content from a message.
    pub fn text_content(&self) -> String {
        match &self.content {
            Value::String(s) => s.clone(),
            Value::Array(arr) => {
                arr.iter()
                    .filter_map(|b| {
                        if b.get("type")?.as_str()? == "text" {
                            b.get("text")?.as_str().map(|s| s.to_string())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            }
            _ => String::new(),
        }
    }
}

/// Extracted tool use block.
#[derive(Debug, Clone)]
pub struct ToolUseBlock {
    pub id: String,
    pub name: String,
    pub input: Value,
}

/// Tool result to send back.
#[derive(Debug, Clone)]
pub struct ToolResultBlock {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

// ─── Thinking block helpers ────────────────────────────────────────────────

/// Check if a content block is a thinking or redacted_thinking block.
fn is_thinking_block(block: &Value) -> bool {
    matches!(
        block.get("type").and_then(|t| t.as_str()),
        Some("thinking") | Some("redacted_thinking")
    )
}

/// Check if a content block is a connector_text block (inserted between thinking blocks).
fn is_connector_block(block: &Value) -> bool {
    block.get("type").and_then(|t| t.as_str()) == Some("connector_text")
}

impl ConversationMessage {
    /// Check if this is an assistant message containing ONLY thinking/redacted_thinking blocks.
    pub fn is_thinking_only(&self) -> bool {
        if self.role != Role::Assistant {
            return false;
        }
        match &self.content {
            Value::Array(blocks) if !blocks.is_empty() => {
                blocks.iter().all(|b| is_thinking_block(b) || is_connector_block(b))
            }
            _ => false,
        }
    }

    /// Strip all thinking, redacted_thinking, and connector_text blocks from this message.
    /// Used when switching to a fallback model (thinking signatures are model-bound).
    pub fn strip_thinking_blocks(&mut self) {
        if let Value::Array(ref mut blocks) = self.content {
            blocks.retain(|b| !is_thinking_block(b) && !is_connector_block(b));
            // If all blocks were stripped, insert a placeholder
            if blocks.is_empty() {
                blocks.push(serde_json::json!({
                    "type": "text",
                    "text": "[No message content]"
                }));
            }
        }
    }

    /// Strip trailing thinking blocks from an assistant message.
    /// The API requires that thinking blocks not be the last block.
    pub fn strip_trailing_thinking(&mut self) {
        if self.role != Role::Assistant {
            return;
        }
        if let Value::Array(ref mut blocks) = self.content {
            while blocks.last().map(|b| is_thinking_block(b) || is_connector_block(b)).unwrap_or(false) {
                blocks.pop();
            }
            if blocks.is_empty() {
                blocks.push(serde_json::json!({
                    "type": "text",
                    "text": "[No message content]"
                }));
            }
        }
    }
}

/// Normalize messages before sending to the API.
///
/// Mirrors `normalizeMessagesForAPI` from `src/utils/messages.ts`:
/// 1. Filter orphaned thinking-only assistant messages
/// 2. Strip trailing thinking from the last assistant message
/// 3. Filter whitespace-only assistant messages
/// 4. Ensure non-empty content arrays
pub fn normalize_messages_for_api(messages: &mut Vec<ConversationMessage>) {
    // 1. Remove assistant messages that contain ONLY thinking blocks
    messages.retain(|m| !m.is_thinking_only());

    // 2. Strip trailing thinking from the last assistant message
    if let Some(last_assistant) = messages.iter_mut().rev()
        .find(|m| m.role == Role::Assistant)
    {
        last_assistant.strip_trailing_thinking();
    }

    // 3. Remove whitespace-only assistant messages
    messages.retain(|m| {
        if m.role != Role::Assistant {
            return true;
        }
        let text = m.text_content();
        !text.trim().is_empty() || {
            // Keep if it has non-text content blocks (tool_use, images, etc.)
            match &m.content {
                Value::Array(blocks) => blocks.iter().any(|b| {
                    let t = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
                    t != "text" && !is_thinking_block(b) && !is_connector_block(b)
                }),
                _ => true,
            }
        }
    });

    // 4. Ensure non-empty content arrays
    for msg in messages.iter_mut() {
        if let Value::Array(ref blocks) = msg.content {
            if blocks.is_empty() {
                msg.content = serde_json::json!([{
                    "type": "text",
                    "text": "[No message content]"
                }]);
            }
        }
    }
}

/// Strip all thinking/redacted_thinking/connector_text blocks from ALL
/// assistant messages. Used on model fallback since thinking signatures
/// are model-bound and replaying them to a different model causes 400 errors.
pub fn strip_all_thinking(messages: &mut [ConversationMessage]) {
    for msg in messages.iter_mut() {
        if msg.role == Role::Assistant {
            msg.strip_thinking_blocks();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_thinking_only() {
        let msg = ConversationMessage {
            role: Role::Assistant,
            content: json!([
                { "type": "thinking", "thinking": "Let me think..." },
            ]),
        };
        assert!(msg.is_thinking_only());

        let msg2 = ConversationMessage {
            role: Role::Assistant,
            content: json!([
                { "type": "thinking", "thinking": "hmm" },
                { "type": "text", "text": "Hello" },
            ]),
        };
        assert!(!msg2.is_thinking_only());
    }

    #[test]
    fn test_strip_thinking_blocks() {
        let mut msg = ConversationMessage {
            role: Role::Assistant,
            content: json!([
                { "type": "thinking", "thinking": "step 1" },
                { "type": "text", "text": "Hello" },
                { "type": "redacted_thinking", "data": "xxx" },
            ]),
        };
        msg.strip_thinking_blocks();
        if let Value::Array(blocks) = &msg.content {
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0]["type"], "text");
            assert_eq!(blocks[0]["text"], "Hello");
        } else {
            panic!("Expected array");
        }
    }

    #[test]
    fn test_strip_trailing_thinking() {
        let mut msg = ConversationMessage {
            role: Role::Assistant,
            content: json!([
                { "type": "text", "text": "Hello" },
                { "type": "thinking", "thinking": "trailing" },
            ]),
        };
        msg.strip_trailing_thinking();
        if let Value::Array(blocks) = &msg.content {
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0]["text"], "Hello");
        }
    }

    #[test]
    fn test_normalize_removes_thinking_only() {
        let mut messages = vec![
            ConversationMessage::user_text("hello"),
            ConversationMessage {
                role: Role::Assistant,
                content: json!([{ "type": "thinking", "thinking": "only thinking" }]),
            },
            ConversationMessage {
                role: Role::Assistant,
                content: json!([
                    { "type": "thinking", "thinking": "step" },
                    { "type": "text", "text": "Real answer" },
                ]),
            },
        ];
        normalize_messages_for_api(&mut messages);
        // The thinking-only message should be removed
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].text_content(), "Real answer");
    }
}

