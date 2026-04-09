//! Tool result budget — mirrors `src/constants/toolLimits.ts` and
//! `src/utils/toolResultStorage.ts`.
//!
//! Enforces per-tool and per-message size limits on tool results to
//! keep the conversation within the context window.
#![allow(dead_code)]

use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ─── Constants (matching original) ──────────────────────────────────────────

/// Default per-tool result size cap (characters).
pub const DEFAULT_MAX_RESULT_SIZE_CHARS: usize = 50_000;
/// Aggregate per-message cap (characters).
pub const MAX_TOOL_RESULTS_PER_MESSAGE_CHARS: usize = 200_000;
/// Preview size for persisted large results.
const PREVIEW_SIZE_BYTES: usize = 2_048;

// ─── Per-tool persistence ───────────────────────────────────────────────────

/// If a tool result exceeds the size threshold, persist to disk and
/// replace with a preview + file path reference.
pub fn maybe_persist_large_result(
    _tool_name: &str,
    tool_use_id: &str,
    content: &str,
    session_dir: &Path,
    threshold: usize,
) -> String {
    if content.len() <= threshold {
        return content.to_string();
    }

    // Write to session_dir/tool-results/<tool_use_id>.txt
    let results_dir = session_dir.join("tool-results");
    let _ = std::fs::create_dir_all(&results_dir);
    let path = results_dir.join(format!("{tool_use_id}.txt"));

    // Only write if file doesn't exist (flag 'wx' equivalent)
    if !path.exists() {
        let _ = std::fs::write(&path, content);
    }

    // Generate preview
    let preview_end = content
        .char_indices()
        .take_while(|(i, _)| *i < PREVIEW_SIZE_BYTES)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(content.len().min(PREVIEW_SIZE_BYTES));

    // Find a clean break point (newline boundary)
    let preview = &content[..preview_end];
    let clean_end = preview.rfind('\n').unwrap_or(preview_end);
    let preview_text = &content[..clean_end];

    format!(
        "<persisted-output>\n\
         Output too large ({size}). Full output saved to: {path}\n\n\
         Preview (first {preview_kb}):\n\
         {preview}\n...\n\
         </persisted-output>",
        size = format_size(content.len()),
        path = path.display(),
        preview_kb = format_size(clean_end),
        preview = preview_text,
    )
}

fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes}B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1}KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1}MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

// ─── Aggregate per-message budget ───────────────────────────────────────────

/// State for the tool result budget enforcement across the conversation.
/// Decisions are frozen after first observation for prompt cache stability.
#[derive(Debug, Default)]
pub struct ToolResultBudgetState {
    /// Tool result IDs that have been seen and their decisions are frozen.
    seen_ids: HashSet<String>,
    /// Cached replacements (tool_use_id → replacement content).
    replacements: HashMap<String, String>,
}

/// Enforce the aggregate tool result budget on a message's tool_result blocks.
///
/// Returns true if any results were replaced.
pub fn enforce_tool_result_budget(
    content_blocks: &mut [Value],
    state: &mut ToolResultBudgetState,
    skip_tools: &HashSet<String>,
    session_dir: &Path,
) -> bool {
    let mut any_replaced = false;

    // First pass: collect info without holding borrows
    let mut total_size: usize = 0;
    let mut candidates: Vec<(usize, usize, String)> = Vec::new(); // (index, size, tool_use_id)
    let mut frozen_replacements: Vec<(usize, String)> = Vec::new(); // (index, replacement)

    for (i, block) in content_blocks.iter().enumerate() {
        if block.get("type").and_then(|t| t.as_str()) != Some("tool_result") {
            continue;
        }
        let tool_use_id = block.get("tool_use_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let content_str = block.get("content")
            .or_else(|| block.get("output"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let size = content_str.len();
        total_size += size;

        let tool_name = block.get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if skip_tools.contains(&tool_name) {
            continue;
        }

        if state.seen_ids.contains(&tool_use_id) {
            if let Some(replacement) = state.replacements.get(&tool_use_id) {
                frozen_replacements.push((i, replacement.clone()));
            }
            continue;
        }

        candidates.push((i, size, tool_use_id));
    }

    // Apply frozen replacements
    for (idx, replacement) in frozen_replacements {
        if let Some(content_field) = content_blocks[idx].get_mut("content") {
            *content_field = Value::String(replacement);
            any_replaced = true;
        }
    }

    // If within budget, just freeze all seen IDs
    if total_size <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
        for (_, _, id) in &candidates {
            state.seen_ids.insert(id.clone());
        }
        return any_replaced;
    }

    // Over budget — replace largest fresh results first
    candidates.sort_by(|a, b| b.1.cmp(&a.1)); // sort by size descending

    for (idx, _size, tool_use_id) in candidates {
        if total_size <= MAX_TOOL_RESULTS_PER_MESSAGE_CHARS {
            state.seen_ids.insert(tool_use_id);
            continue;
        }

        let content_str = content_blocks[idx].get("content")
            .or_else(|| content_blocks[idx].get("output"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let replacement = maybe_persist_large_result(
            "", &tool_use_id, &content_str, session_dir, DEFAULT_MAX_RESULT_SIZE_CHARS,
        );
        let saved = content_str.len() - replacement.len();

        if let Some(content_field) = content_blocks[idx].get_mut("content") {
            *content_field = Value::String(replacement.clone());
        }

        state.replacements.insert(tool_use_id.clone(), replacement);
        state.seen_ids.insert(tool_use_id);
        total_size = total_size.saturating_sub(saved);
        any_replaced = true;
    }

    any_replaced
}
