//! FileEdit tool — partial string replacement with full feature parity.
//!
//! Mirrors `src/tools/FileEditTool/` from the TypeScript layer.
//!
//! Features:
//! - `replace_all` parameter: replace all occurrences when true
//! - Uniqueness check with helpful suggestions
//! - Path expansion (~ to home dir)
//! - Parent directory auto-creation
//! - File creation flow (empty old_string + nonexistent file = create)
//! - Max file size check (1 GiB)
//! - Line ending detection and preservation (CRLF vs LF)

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// Maximum file size: 1 GiB.
const MAX_FILE_SIZE: u64 = 1_073_741_824;

pub struct FileEditTool;

/// Expand `~` prefix to the user's home directory.
fn expand_tilde(p: &str) -> PathBuf {
    if let Some(rest) = p.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if p == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }
    PathBuf::from(p)
}

/// Detect whether the file content uses CRLF line endings.
/// Returns `true` if the majority of line endings are CRLF.
fn uses_crlf(content: &str) -> bool {
    let crlf_count = content.matches("\r\n").count();
    let lf_only_count = content.matches('\n').count().saturating_sub(crlf_count);
    crlf_count > 0 && crlf_count >= lf_only_count
}

/// Normalize line endings in `text` to match the target style.
fn normalize_line_endings(text: &str, crlf: bool) -> String {
    if crlf {
        // First normalize everything to LF, then convert to CRLF
        let lf = text.replace("\r\n", "\n");
        lf.replace('\n', "\r\n")
    } else {
        text.replace("\r\n", "\n")
    }
}

/// Find lines in the file that are similar to `needle` (simple substring containment).
/// Returns up to 3 candidate lines for the user to inspect.
fn find_similar_strings(content: &str, needle: &str) -> Vec<String> {
    if needle.is_empty() || needle.len() < 4 {
        return Vec::new();
    }

    // Take the first significant "word" of the needle for fuzzy matching.
    let needle_lower = needle.to_lowercase();
    let words: Vec<&str> = needle_lower.split_whitespace().collect();
    let key = if let Some(w) = words.first() { *w } else { &needle_lower };

    let mut candidates: Vec<String> = Vec::new();
    for line in content.lines() {
        let ll = line.to_lowercase();
        if ll.contains(key) && line.trim() != needle.trim() {
            candidates.push(line.to_string());
            if candidates.len() >= 3 {
                break;
            }
        }
    }
    candidates
}

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &'static str { "Edit" }

    fn description(&self) -> &str {
        "Performs exact string replacements in files. Supports replace_all, \
         file creation when old_string is empty, path expansion, and line ending preservation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to modify"
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to replace"
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace it with (must be different from old_string)"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_string (default false)"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let raw_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path required"))?;
        let old = input["old_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("old_string required"))?;
        let new = input["new_string"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("new_string required"))?;
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let path = expand_tilde(raw_path);
        let path_str = path.display().to_string();

        debug!("FileEdit: path={path_str} replace_all={replace_all}");

        // ── File creation flow ──────────────────────────────────────────
        // When old_string is empty and the file doesn't exist, create it.
        if old.is_empty() && !path.exists() {
            // Auto-create parent directories
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&path, new).await?;
            let msg = format!("Created new file: {path_str}");
            let _ = tx.send(ToolOutput { text: msg.clone(), is_error: false }).await;
            return Ok(ToolResult::ok(msg));
        }

        // ── Read existing file ──────────────────────────────────────────
        if !path.exists() {
            return Ok(ToolResult::error(format!(
                "File not found: {path_str}. To create a new file, pass an empty old_string."
            )));
        }

        // Check file size
        let metadata = tokio::fs::metadata(&path).await?;
        if metadata.len() > MAX_FILE_SIZE {
            return Ok(ToolResult::error(format!(
                "File too large ({} bytes). Maximum supported size is {} bytes (1 GiB).",
                metadata.len(),
                MAX_FILE_SIZE
            )));
        }

        // Read as raw bytes to preserve line endings
        let raw_bytes = tokio::fs::read(&path).await?;
        let content = String::from_utf8_lossy(&raw_bytes).to_string();
        let crlf = uses_crlf(&content);

        // ── Handle empty old_string on existing file ────────────────────
        // (prepend new_string to existing content)
        if old.is_empty() {
            let updated = if new.is_empty() {
                content.clone()
            } else {
                let normalized_new = normalize_line_endings(new, crlf);
                format!("{normalized_new}{content}")
            };
            write_atomic(&path, &updated).await?;
            let msg = format!("Prepended content to {path_str}");
            let _ = tx.send(ToolOutput { text: msg.clone(), is_error: false }).await;
            return Ok(ToolResult::ok(msg));
        }

        // ── Count occurrences ───────────────────────────────────────────
        let count = content.matches(old).count();

        if count == 0 {
            // Try to find similar strings to help the user
            let similar = find_similar_strings(&content, old);
            let mut msg = format!("old_string not found in {path_str}.");
            if !similar.is_empty() {
                msg.push_str("\n\nDid you mean one of these?\n");
                for s in &similar {
                    let trimmed = s.trim();
                    let display = if trimmed.len() > 120 {
                        format!("{}...", &trimmed[..120])
                    } else {
                        trimmed.to_string()
                    };
                    msg.push_str(&format!("  - {display}\n"));
                }
            }
            msg.push_str("\nMake sure you have read the file first with the Read tool.");
            return Ok(ToolResult::error(msg));
        }

        if count > 1 && !replace_all {
            return Ok(ToolResult::error(format!(
                "old_string matches {count} times in {path_str} — must be unique. \
                 Either provide more surrounding context to make it unique, \
                 or set replace_all: true to replace all {count} occurrences."
            )));
        }

        // ── Perform replacement ─────────────────────────────────────────
        let normalized_new = normalize_line_endings(new, crlf);

        let updated = if replace_all {
            content.replace(old, &normalized_new)
        } else {
            content.replacen(old, &normalized_new, 1)
        };

        write_atomic(&path, &updated).await?;

        let msg = if replace_all && count > 1 {
            format!("Replaced {count} occurrences in {path_str}")
        } else {
            format!("Edit applied to {path_str}")
        };
        let _ = tx.send(ToolOutput { text: msg.clone(), is_error: false }).await;
        Ok(ToolResult::ok(msg))
    }
}

/// Write content to a file atomically via a temp file + rename.
async fn write_atomic(path: &Path, content: &str) -> Result<()> {
    // Auto-create parent directories
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp = format!("{}.tmp.{}", path.display(), std::process::id());
    tokio::fs::write(&tmp, content).await?;
    tokio::fs::rename(&tmp, path).await?;
    Ok(())
}
