//! Glob tool — fast file pattern matching with result limits and path validation.
//!
//! Mirrors `src/tools/GlobTool/` from the TypeScript layer.
//!
//! Features:
//! - Result limit (default 100) with truncation indicator
//! - Relative path conversion (relative to cwd)
//! - Path validation (check directory exists)
//! - Path expansion (~ to home dir)

use anyhow::Result;
use async_trait::async_trait;
use globset::Glob;
use ignore::WalkBuilder;
use serde_json::{json, Value};
use std::path::PathBuf;
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// Default result limit.
const DEFAULT_LIMIT: usize = 100;

pub struct GlobTool;

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

#[async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &'static str { "Glob" }

    fn description(&self) -> &str {
        "Find files matching a glob pattern. Returns file paths sorted by modification time \
         (most recent first). Results are limited to 100 by default."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The glob pattern to match files against"
                },
                "path": {
                    "type": "string",
                    "description": "The directory to search in (default: current working directory)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 100, 0 for unlimited)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let pattern = input["pattern"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("pattern required"))?;
        let raw_path = input["path"].as_str();
        let limit = input["limit"]
            .as_u64()
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_LIMIT);

        // ── Resolve base path ───────────────────────────────────────────
        let base = match raw_path {
            Some(p) => expand_tilde(p),
            None => std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };

        let base_str = base.display().to_string();
        debug!("Glob: pattern={pattern} base={base_str} limit={limit}");

        // ── Path validation ─────────────────────────────────────────────
        if !base.exists() {
            return Ok(ToolResult::error(format!(
                "Directory does not exist: {base_str}"
            )));
        }
        if !base.is_dir() {
            return Ok(ToolResult::error(format!(
                "Path is not a directory: {base_str}"
            )));
        }

        // ── Build glob matcher ──────────────────────────────────────────
        let glob = Glob::new(pattern)
            .map_err(|e| anyhow::anyhow!("Invalid glob pattern: {e}"))?
            .compile_matcher();

        // ── Walk and match ──────────────────────────────────────────────
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        let mut matches: Vec<(std::time::SystemTime, String)> = Vec::new();
        for entry in WalkBuilder::new(&base).hidden(false).build().flatten() {
            let path = entry.path();
            if glob.is_match(path) || glob.is_match(path.file_name().unwrap_or_default()) {
                let mtime = path
                    .metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
                // Convert to relative path
                let display = pathdiff::diff_paths(path, &cwd)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.display().to_string());
                matches.push((mtime, display));
            }
        }

        // ── Sort by modification time (most recent first) ───────────────
        matches.sort_by(|a, b| b.0.cmp(&a.0));

        // ── Apply limit ─────────────────────────────────────────────────
        let total = matches.len();
        let truncated = limit > 0 && total > limit;
        let display_matches: Vec<&str> = if limit > 0 {
            matches.iter().take(limit).map(|(_, p)| p.as_str()).collect()
        } else {
            matches.iter().map(|(_, p)| p.as_str()).collect()
        };

        if display_matches.is_empty() {
            let msg = format!("No files matched pattern: {pattern}");
            let _ = tx.send(ToolOutput { text: msg.clone(), is_error: false }).await;
            return Ok(ToolResult::ok(msg));
        }

        let mut result = display_matches.join("\n");
        if truncated {
            result.push_str(&format!(
                "\n\n[Showing {limit}/{total} results. Set limit to 0 for all results, or refine your pattern.]"
            ));
        }

        let _ = tx.send(ToolOutput { text: result.clone(), is_error: false }).await;
        Ok(ToolResult::ok(result))
    }
}
