//! FileWrite tool — atomic file write with encoding preservation.
//!
//! Mirrors `src/tools/FileWriteTool/` from the TypeScript layer.
//!
//! Features:
//! - Path expansion (~ to home dir)
//! - Parent directory auto-creation
//! - Encoding detection from existing file (UTF-16LE BOM)
//! - Write with same encoding if detected

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// UTF-16 LE BOM bytes.
const UTF16LE_BOM: &[u8] = &[0xFF, 0xFE];

pub struct FileWriteTool;

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

/// Detect the encoding of an existing file.
/// Returns `true` if the file starts with a UTF-16LE BOM.
async fn detect_utf16le(path: &Path) -> bool {
    if let Ok(bytes) = tokio::fs::read(path).await {
        bytes.len() >= 2 && bytes[0] == UTF16LE_BOM[0] && bytes[1] == UTF16LE_BOM[1]
    } else {
        false
    }
}

/// Encode a UTF-8 string as UTF-16LE bytes with BOM.
fn encode_utf16le_with_bom(s: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(2 + s.len() * 2);
    // BOM
    out.push(0xFF);
    out.push(0xFE);
    // Encode each char as UTF-16LE
    for ch in s.encode_utf16() {
        out.push((ch & 0xFF) as u8);
        out.push((ch >> 8) as u8);
    }
    out
}

#[async_trait]
impl Tool for FileWriteTool {
    fn name(&self) -> &'static str { "Write" }

    fn description(&self) -> &str {
        "Write content to a file. Creates file and directories if needed. \
         Preserves UTF-16LE encoding if the existing file uses it."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "The absolute path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let raw_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path required"))?;
        let content = input["content"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("content required"))?;

        let path = expand_tilde(raw_path);
        let path_str = path.display().to_string();

        debug!("FileWrite: path={path_str}");

        // ── Detect existing encoding ────────────────────────────────────
        let use_utf16le = path.exists() && detect_utf16le(&path).await;

        // ── Auto-create parent directories ──────────────────────────────
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // ── Write via temp file for atomicity ───────────────────────────
        let tmp = format!("{}.tmp.{}", path_str, std::process::id());

        if use_utf16le {
            let bytes = encode_utf16le_with_bom(content);
            tokio::fs::write(&tmp, &bytes).await?;
        } else {
            tokio::fs::write(&tmp, content).await?;
        }

        tokio::fs::rename(&tmp, &path).await?;

        let encoding_note = if use_utf16le { " (UTF-16LE)" } else { "" };
        let msg = format!("File written{encoding_note}: {path_str}");
        let _ = tx.send(ToolOutput { text: msg.clone(), is_error: false }).await;
        Ok(ToolResult::ok(msg))
    }
}
