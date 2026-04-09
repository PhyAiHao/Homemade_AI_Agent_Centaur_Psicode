//! Grep tool — ripgrep-based content search.
//!
//! Mirrors `src/tools/GrepTool/GrepTool.ts` with full parameter parity.
//! Uses the `grep-*` crate family (ripgrep as a library) for search and
//! `ignore::WalkBuilder` for .gitignore-aware file walking.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::cmp::Reverse;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;

use super::{Tool, ToolOutput, ToolResult};

pub struct GrepTool;

/// VCS directories to always exclude.
const VCS_DIRS: &[&str] = &[".git", ".svn", ".hg", ".bzr", ".jj", ".sl"];
/// Max column width before truncation (prevents base64/minified clutter).
const MAX_COLUMN_WIDTH: usize = 500;
/// Default head limit.
const DEFAULT_HEAD_LIMIT: usize = 250;

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &'static str { "Grep" }

    fn description(&self) -> &str {
        "Search file contents using a regex pattern. Supports multiple output modes, \
         context lines, file type filters, and pagination."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "The regex pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory or file to search in (default: current directory)"
                },
                "glob": {
                    "type": "string",
                    "description": "Glob pattern to filter files (e.g. \"*.js\", \"*.{ts,tsx}\")"
                },
                "output_mode": {
                    "type": "string",
                    "enum": ["content", "files_with_matches", "count"],
                    "description": "Output mode: content (matching lines), files_with_matches (file paths only), count (match counts). Default: files_with_matches"
                },
                "-A": {
                    "type": "number",
                    "description": "Lines to show after each match (requires output_mode: content)"
                },
                "-B": {
                    "type": "number",
                    "description": "Lines to show before each match (requires output_mode: content)"
                },
                "-C": {
                    "type": "number",
                    "description": "Alias for context (lines before and after)"
                },
                "context": {
                    "type": "number",
                    "description": "Lines to show before and after each match (requires output_mode: content)"
                },
                "-n": {
                    "type": "boolean",
                    "description": "Show line numbers in output (default: true, requires output_mode: content)"
                },
                "-i": {
                    "type": "boolean",
                    "description": "Case insensitive search"
                },
                "type": {
                    "type": "string",
                    "description": "File type to search (e.g. js, py, rust, go)"
                },
                "head_limit": {
                    "type": "number",
                    "description": "Max entries to return (default: 250, 0 for unlimited)"
                },
                "offset": {
                    "type": "number",
                    "description": "Skip first N entries before applying head_limit (default: 0)"
                },
                "multiline": {
                    "type": "boolean",
                    "description": "Enable multiline mode where . matches newlines (default: false)"
                }
            },
            "required": ["pattern"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        // ── Parse parameters ────────────────────────────────────────────
        let pattern = input["pattern"].as_str()
            .ok_or_else(|| anyhow::anyhow!("pattern required"))?;
        let base_path = input["path"].as_str().unwrap_or(".");
        let glob_pattern = input["glob"].as_str();
        let output_mode = input["output_mode"].as_str().unwrap_or("files_with_matches");
        let case_insensitive = input["-i"].as_bool().unwrap_or(false);
        let show_line_numbers = input["-n"].as_bool().unwrap_or(true);
        let multiline = input["multiline"].as_bool().unwrap_or(false);
        let file_type = input["type"].as_str();

        // Context lines
        let context = input["-C"].as_u64()
            .or_else(|| input["context"].as_u64())
            .map(|v| v as usize);
        let after = input["-A"].as_u64().map(|v| v as usize).or(context);
        let before = input["-B"].as_u64().map(|v| v as usize).or(context);

        // Pagination
        let head_limit = input["head_limit"].as_u64()
            .map(|v| v as usize)
            .unwrap_or(DEFAULT_HEAD_LIMIT);
        let offset = input["offset"].as_u64()
            .map(|v| v as usize)
            .unwrap_or(0);

        // ── Build regex ─────────────────────────────────────────────────
        let mut regex_str = pattern.to_string();
        if case_insensitive {
            regex_str = format!("(?i){regex_str}");
        }
        if multiline {
            regex_str = format!("(?s){regex_str}");
        }
        let regex = regex::Regex::new(&regex_str)
            .map_err(|e| anyhow::anyhow!("Invalid regex: {e}"))?;

        // ── Resolve base path ───────────────────────────────────────────
        let base = Path::new(base_path);
        if !base.exists() {
            return Ok(ToolResult::error(format!(
                "Path does not exist: {base_path}. Check the path and try again."
            )));
        }

        // ── File type extension mapping ─────────────────────────────────
        let type_extensions: Option<HashSet<&str>> = file_type.map(|t| {
            match t {
                "js" => ["js", "mjs", "cjs", "jsx"].iter().copied().collect(),
                "ts" => ["ts", "mts", "cts", "tsx"].iter().copied().collect(),
                "py" => ["py", "pyi", "pyw"].iter().copied().collect(),
                "rust" | "rs" => ["rs"].iter().copied().collect(),
                "go" => ["go"].iter().copied().collect(),
                "java" => ["java"].iter().copied().collect(),
                "c" => ["c", "h"].iter().copied().collect(),
                "cpp" => ["cpp", "cxx", "cc", "hpp", "hxx", "hh"].iter().copied().collect(),
                "rb" => ["rb", "erb"].iter().copied().collect(),
                "php" => ["php"].iter().copied().collect(),
                "swift" => ["swift"].iter().copied().collect(),
                "kt" => ["kt", "kts"].iter().copied().collect(),
                "sh" => ["sh", "bash", "zsh"].iter().copied().collect(),
                "css" => ["css", "scss", "sass", "less"].iter().copied().collect(),
                "html" => ["html", "htm"].iter().copied().collect(),
                "json" => ["json", "jsonc", "json5"].iter().copied().collect(),
                "yaml" | "yml" => ["yaml", "yml"].iter().copied().collect(),
                "toml" => ["toml"].iter().copied().collect(),
                "md" => ["md", "mdx", "markdown"].iter().copied().collect(),
                "sql" => ["sql"].iter().copied().collect(),
                _ => [t].iter().copied().collect(),
            }
        });

        // ── Build glob matchers ─────────────────────────────────────────
        let glob_matchers: Vec<globset::GlobMatcher> = if let Some(g) = glob_pattern {
            // Support comma/space-separated globs
            g.split(|c: char| c == ',' || (c == ' ' && !g.contains('{')))
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .filter_map(|s| globset::Glob::new(s).ok().map(|g| g.compile_matcher()))
                .collect()
        } else {
            Vec::new()
        };

        // ── Walk and search ─────────────────────────────────────────────
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Collect matching files with metadata for mtime sorting
        struct FileMatch {
            path: PathBuf,
            relative: String,
            mtime: std::time::SystemTime,
            matches: Vec<LineMatch>,
            match_count: usize,
        }
        struct LineMatch {
            line_no: usize,
            line: String,
        }

        let mut file_matches: Vec<FileMatch> = Vec::new();

        let walker = ignore::WalkBuilder::new(base)
            .hidden(false)
            .git_ignore(true)
            .filter_entry(|entry| {
                // Exclude VCS directories
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_str().unwrap_or("");
                    if VCS_DIRS.contains(&name) {
                        return false;
                    }
                }
                true
            })
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() { continue; }

            // File type filter
            if let Some(ref exts) = type_extensions {
                let ext = path.extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("");
                if !exts.contains(ext) {
                    continue;
                }
            }

            // Glob filter
            if !glob_matchers.is_empty() {
                let matches_any = glob_matchers.iter().any(|gm| {
                    gm.is_match(path) || gm.is_match(path.file_name().unwrap_or_default())
                });
                if !matches_any {
                    continue;
                }
            }

            // Read file content
            let contents = match std::fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue, // skip binary/unreadable files
            };

            let lines: Vec<&str> = contents.lines().collect();
            let mut matches: Vec<LineMatch> = Vec::new();

            for (i, line) in lines.iter().enumerate() {
                // Column width limit
                let search_line = if line.len() > MAX_COLUMN_WIDTH {
                    &line[..MAX_COLUMN_WIDTH]
                } else {
                    line
                };

                if regex.is_match(search_line) {
                    matches.push(LineMatch {
                        line_no: i + 1,
                        line: line.to_string(),
                    });
                }
            }

            if !matches.is_empty() {
                let relative = pathdiff::diff_paths(path, &cwd)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string_lossy().to_string());

                let mtime = path.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::UNIX_EPOCH);

                let match_count = matches.len();
                file_matches.push(FileMatch {
                    path: path.to_path_buf(),
                    relative,
                    mtime,
                    matches,
                    match_count,
                });
            }
        }

        // ── Sort by modification time (most recent first) ───────────────
        file_matches.sort_by_key(|f| Reverse(f.mtime));

        // ── Format output based on mode ─────────────────────────────────
        let result = match output_mode {
            "files_with_matches" => {
                let entries: Vec<String> = file_matches.iter()
                    .map(|f| f.relative.clone())
                    .collect();
                let total = entries.len();
                let paginated = apply_pagination(&entries, offset, head_limit);
                if paginated.is_empty() {
                    "No matches found".to_string()
                } else {
                    let mut out = paginated.join("\n");
                    if paginated.len() < total {
                        out.push_str(&format!(
                            "\n\n[Showing {}/{total} files. Use offset={} to see more]",
                            paginated.len(),
                            offset + paginated.len(),
                        ));
                    }
                    out
                }
            }

            "count" => {
                let entries: Vec<String> = file_matches.iter()
                    .map(|f| format!("{}:{}", f.relative, f.match_count))
                    .collect();
                let total_matches: usize = file_matches.iter().map(|f| f.match_count).sum();
                let paginated = apply_pagination(&entries, offset, head_limit);
                if paginated.is_empty() {
                    "No matches found".to_string()
                } else {
                    let mut out = paginated.join("\n");
                    out.push_str(&format!("\n\nTotal: {total_matches} matches in {} files", file_matches.len()));
                    out
                }
            }

            _ => {
                let mut all_lines: Vec<String> = Vec::new();

                for fm in &file_matches {
                    // Read file again for context lines
                    let contents = std::fs::read_to_string(&fm.path).unwrap_or_default();
                    let lines: Vec<&str> = contents.lines().collect();

                    for m in &fm.matches {
                        let line_idx = m.line_no - 1;
                        let start = line_idx.saturating_sub(before.unwrap_or(0));
                        let end = (line_idx + 1 + after.unwrap_or(0)).min(lines.len());

                        for i in start..end {
                            let prefix = if show_line_numbers {
                                format!("{}:{}:", fm.relative, i + 1)
                            } else {
                                format!("{}:", fm.relative)
                            };
                            let line_text = lines.get(i).unwrap_or(&"");
                            // Truncate long lines
                            let truncated = if line_text.len() > MAX_COLUMN_WIDTH {
                                format!("{}...", &line_text[..MAX_COLUMN_WIDTH])
                            } else {
                                line_text.to_string()
                            };
                            all_lines.push(format!("{prefix}{truncated}"));
                        }
                        // Separator between match groups when using context
                        if before.is_some() || after.is_some() {
                            all_lines.push("--".to_string());
                        }
                    }
                }

                // Remove trailing separator
                if all_lines.last().map(|l| l == "--").unwrap_or(false) {
                    all_lines.pop();
                }

                let paginated = apply_pagination(&all_lines, offset, head_limit);
                if paginated.is_empty() {
                    "No matches found".to_string()
                } else {
                    let mut out = paginated.join("\n");
                    if head_limit > 0 && paginated.len() < all_lines.len() {
                        out.push_str(&format!(
                            "\n\n[Showing {}/{} lines. Use offset={} to see more]",
                            paginated.len(),
                            all_lines.len(),
                            offset + paginated.len(),
                        ));
                    }
                    out
                }
            }
        };

        let _ = tx.send(ToolOutput { text: result.clone(), is_error: false }).await;
        Ok(ToolResult::ok(result))
    }
}

/// Apply offset + head_limit pagination to a list of items.
fn apply_pagination(items: &[String], offset: usize, head_limit: usize) -> Vec<String> {
    let skipped: Vec<String> = items.iter().skip(offset).cloned().collect();
    if head_limit == 0 {
        skipped
    } else {
        skipped.into_iter().take(head_limit).collect()
    }
}
