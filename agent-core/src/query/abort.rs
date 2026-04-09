//! Abort/cancellation system — mirrors `src/utils/abortController.ts`.
//!
//! Provides cooperative cancellation across the query loop, tool execution,
//! and API streaming via a shared `AbortHandle`.
//!
//! Design:
//!   - `AbortHandle` is a cheap-to-clone, thread-safe cancellation token.
//!   - `create_child_handle()` creates a child that aborts when the parent
//!     aborts, but NOT vice versa. Used for sibling tool cancellation.
//!   - The query loop checks `is_aborted()` at key points.
//!   - Tools receive the handle and can check/listen for cancellation.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio_util::sync::CancellationToken;

/// The reason an abort was triggered.
#[derive(Debug, Clone, PartialEq)]
pub enum AbortReason {
    /// User pressed Ctrl-C or equivalent.
    UserInterrupt,
    /// User submitted a new message while the agent was still working
    /// (submit-interrupt). Skips the interruption message.
    SubmitInterrupt,
    /// A sibling tool (Bash) errored and cascaded cancellation.
    SiblingError,
    /// Max turns or budget exceeded.
    LimitReached,
    /// Custom reason.
    Custom(String),
}

/// A cooperative cancellation handle.
///
/// Cheap to clone (just Arc bumps). Thread-safe.
#[derive(Clone)]
pub struct AbortHandle {
    inner: Arc<AbortInner>,
}

struct AbortInner {
    token: CancellationToken,
    aborted: AtomicBool,
    reason: Mutex<Option<AbortReason>>,
    /// Child tokens that should be cancelled when this handle is aborted.
    children: Mutex<Vec<CancellationToken>>,
}

impl AbortHandle {
    /// Create a new abort handle (not yet aborted).
    pub fn new() -> Self {
        AbortHandle {
            inner: Arc::new(AbortInner {
                token: CancellationToken::new(),
                aborted: AtomicBool::new(false),
                reason: Mutex::new(None),
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Trigger the abort with a reason.
    pub fn abort(&self, reason: AbortReason) {
        if self.inner.aborted.swap(true, Ordering::SeqCst) {
            return; // already aborted
        }
        *self.inner.reason.lock().unwrap() = Some(reason);
        self.inner.token.cancel();
        // Cancel all children
        let children = self.inner.children.lock().unwrap();
        for child in children.iter() {
            child.cancel();
        }
    }

    /// Check if this handle has been aborted.
    pub fn is_aborted(&self) -> bool {
        self.inner.aborted.load(Ordering::SeqCst)
    }

    /// Get the abort reason (if aborted).
    pub fn reason(&self) -> Option<AbortReason> {
        self.inner.reason.lock().unwrap().clone()
    }

    /// Returns true if the abort was a submit-interrupt (user queued new input).
    pub fn is_submit_interrupt(&self) -> bool {
        matches!(self.reason(), Some(AbortReason::SubmitInterrupt))
    }

    /// Get the cancellation token for use with `tokio::select!`.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.inner.token
    }

    /// Create a child handle that:
    /// - Is cancelled when this (parent) handle is cancelled.
    /// - Does NOT cancel the parent when the child is cancelled.
    ///
    /// Used for sibling tool cancellation: a Bash error cancels the
    /// child handle, which cancels other siblings sharing it, but
    /// does not abort the parent query loop.
    pub fn create_child(&self) -> AbortHandle {
        let child_token = self.inner.token.child_token();
        // Register the child token so parent abort cascades
        self.inner.children.lock().unwrap().push(child_token.clone());

        AbortHandle {
            inner: Arc::new(AbortInner {
                token: child_token,
                aborted: AtomicBool::new(self.is_aborted()),
                reason: Mutex::new(None),
                children: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Wait until this handle is aborted. Useful in `tokio::select!`.
    pub async fn cancelled(&self) {
        self.inner.token.cancelled().await
    }
}

impl std::fmt::Debug for AbortHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AbortHandle")
            .field("aborted", &self.is_aborted())
            .finish()
    }
}

impl Default for AbortHandle {
    fn default() -> Self {
        Self::new()
    }
}

/// Generate synthetic error tool_result messages for all tool_use blocks
/// in the given assistant messages that don't have matching tool_results.
///
/// This ensures the API never sees an orphaned tool_use without a
/// corresponding tool_result (which would cause a 400 error).
///
/// Mirrors `yieldMissingToolResultBlocks()` from `src/query.ts`.
pub fn backfill_missing_tool_results(
    messages: &[super::message::ConversationMessage],
    error_message: &str,
) -> Vec<super::message::ConversationMessage> {
    use serde_json::Value;
    use std::collections::HashSet;
    use super::message::{ConversationMessage, Role, ToolResultBlock};

    // Collect all tool_use IDs from assistant messages
    let mut tool_use_ids: Vec<(String, String)> = Vec::new(); // (id, name)
    for msg in messages {
        if msg.role != Role::Assistant {
            continue;
        }
        for block in msg.tool_use_blocks() {
            tool_use_ids.push((block.id.clone(), block.name.clone()));
        }
    }

    // Collect all tool_result IDs already present
    let mut existing_results: HashSet<String> = HashSet::new();
    for msg in messages {
        if msg.role != Role::User {
            continue;
        }
        if let Value::Array(blocks) = &msg.content {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                    if let Some(id) = block.get("tool_use_id").and_then(|v| v.as_str()) {
                        existing_results.insert(id.to_string());
                    }
                }
            }
        }
    }

    // Generate error results for any missing tool_use IDs
    let missing: Vec<ToolResultBlock> = tool_use_ids.into_iter()
        .filter(|(id, _)| !existing_results.contains(id))
        .map(|(id, _name)| ToolResultBlock {
            tool_use_id: id,
            content: error_message.to_string(),
            is_error: true,
        })
        .collect();

    if missing.is_empty() {
        Vec::new()
    } else {
        vec![ConversationMessage::tool_results(missing)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_abort_handle_basic() {
        let handle = AbortHandle::new();
        assert!(!handle.is_aborted());
        assert!(handle.reason().is_none());

        handle.abort(AbortReason::UserInterrupt);
        assert!(handle.is_aborted());
        assert_eq!(handle.reason(), Some(AbortReason::UserInterrupt));
    }

    #[test]
    fn test_abort_handle_idempotent() {
        let handle = AbortHandle::new();
        handle.abort(AbortReason::UserInterrupt);
        handle.abort(AbortReason::SiblingError); // second abort is ignored
        assert_eq!(handle.reason(), Some(AbortReason::UserInterrupt));
    }

    #[test]
    fn test_child_aborts_when_parent_aborts() {
        let parent = AbortHandle::new();
        let child = parent.create_child();

        assert!(!child.is_aborted());
        parent.abort(AbortReason::UserInterrupt);
        // Child token is cancelled by parent
        assert!(child.cancellation_token().is_cancelled());
    }

    #[test]
    fn test_child_abort_does_not_affect_parent() {
        let parent = AbortHandle::new();
        let child = parent.create_child();

        child.abort(AbortReason::SiblingError);
        assert!(child.is_aborted());
        assert!(!parent.is_aborted()); // parent is NOT aborted
    }

    #[test]
    fn test_backfill_missing_tool_results() {
        use super::super::message::ConversationMessage;
        use serde_json::json;

        let messages = vec![
            ConversationMessage {
                role: super::super::message::Role::Assistant,
                content: json!([
                    { "type": "tool_use", "id": "tu_1", "name": "Bash", "input": {} },
                    { "type": "tool_use", "id": "tu_2", "name": "Grep", "input": {} },
                ]),
            },
            // Only tu_1 has a result
            ConversationMessage {
                role: super::super::message::Role::User,
                content: json!([
                    { "type": "tool_result", "tool_use_id": "tu_1", "content": "ok", "is_error": false },
                ]),
            },
        ];

        let backfills = backfill_missing_tool_results(&messages, "Interrupted by user");
        assert_eq!(backfills.len(), 1);
        // Should contain a tool_result for tu_2
        let content = &backfills[0].content;
        if let serde_json::Value::Array(blocks) = content {
            assert_eq!(blocks.len(), 1);
            assert_eq!(blocks[0]["tool_use_id"], "tu_2");
            assert_eq!(blocks[0]["is_error"], true);
        } else {
            panic!("Expected array content");
        }
    }
}
