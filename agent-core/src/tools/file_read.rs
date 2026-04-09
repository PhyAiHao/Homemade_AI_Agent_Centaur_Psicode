//! FileRead tool — read text files, images (base64), PDFs, Jupyter notebooks.
//!
//! Mirrors `src/tools/FileReadTool/` from the TypeScript layer.
//!
//! Features:
//! - `pages` param for PDF page range selection
//! - PDF page validation (max 20 pages per request)
//! - Binary file detection (first 8 KB null-byte scan)
//! - Blocked device paths (/dev/zero, /dev/random, /dev/stdin, /dev/null, /proc/self/fd/0-2)
//! - 1-indexed offset (offset=0 means line 1, offset=5 means start at line 6)
//! - Max file size check (configurable, default 10 MB for text)
//! - Path expansion (~ to home dir)

use anyhow::Result;
use async_trait::async_trait;
use base64::Engine;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// Default maximum text file size: 10 MB.
const DEFAULT_MAX_TEXT_SIZE: u64 = 10 * 1024 * 1024;

/// Maximum pages per PDF request.
const MAX_PDF_PAGES: usize = 20;

/// Paths that must never be read.
const BLOCKED_PATHS: &[&str] = &[
    "/dev/zero",
    "/dev/random",
    "/dev/urandom",
    "/dev/stdin",
    "/dev/null",
    "/proc/self/fd/0",
    "/proc/self/fd/1",
    "/proc/self/fd/2",
];

pub struct FileReadTool;

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

/// Check whether the first 8 KB of a file contain null bytes (binary indicator).
async fn is_binary_file(path: &Path) -> Result<bool> {
    use tokio::io::AsyncReadExt;
    let mut file = tokio::fs::File::open(path).await?;
    let mut buf = vec![0u8; 8192];
    let n = file.read(&mut buf).await?;
    Ok(buf[..n].contains(&0))
}

/// Parse a page range string like "1-5", "3", "10-20" into (start, end) 1-indexed inclusive.
fn parse_page_range(pages: &str) -> Result<Vec<(usize, usize)>> {
    let mut ranges = Vec::new();
    for part in pages.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let start: usize = a.trim().parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {a}"))?;
            let end: usize = b.trim().parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {b}"))?;
            if start == 0 || end == 0 {
                return Err(anyhow::anyhow!("Page numbers are 1-indexed; 0 is not valid."));
            }
            if start > end {
                return Err(anyhow::anyhow!("Invalid range: {start}-{end} (start > end)"));
            }
            ranges.push((start, end));
        } else {
            let page: usize = part.parse()
                .map_err(|_| anyhow::anyhow!("Invalid page number: {part}"))?;
            if page == 0 {
                return Err(anyhow::anyhow!("Page numbers are 1-indexed; 0 is not valid."));
            }
            ranges.push((page, page));
        }
    }
    Ok(ranges)
}

/// Count total pages requested from ranges.
fn count_pages(ranges: &[(usize, usize)]) -> usize {
    ranges.iter().map(|(s, e)| e - s + 1).sum()
}

#[async_trait]
impl Tool for FileReadTool {
    fn name(&self) -> &'static str { "Read" }

    fn description(&self) -> &str {
        "Read a file from the filesystem. Supports text files, images (returned as base64), \
         PDFs (with page ranges), and Jupyter notebooks. Binary files are detected and rejected."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute path to the file to read"
                },
                "offset": {
                    "type": "integer",
                    "description": "Line offset to start reading from (0 means line 1, 5 means start at line 6)"
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read (optional)"
                },
                "pages": {
                    "type": "string",
                    "description": "Page range for PDF files (e.g. \"1-5\", \"3\", \"10-20\"). Max 20 pages per request."
                }
            },
            "required": ["file_path"]
        })
    }

    fn requires_permission(&self) -> bool { false }

    async fn execute(&self, input: Value, output_tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let raw_path = input["file_path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("file_path is required"))?;

        let offset = input["offset"].as_u64().unwrap_or(0) as usize;
        let limit = input["limit"].as_u64().map(|v| v as usize);
        let pages = input["pages"].as_str();

        let path = expand_tilde(raw_path);

        // ── Resolve symlinks to prevent traversal attacks ───────────────
        // Canonicalize resolves all symlinks so `./safe` → `/etc/passwd`
        // is checked against the blocklist using the real target path.
        let path = if path.exists() {
            path.canonicalize().unwrap_or(path)
        } else {
            path
        };
        let path_str = path.display().to_string();

        debug!("FileRead: path={path_str} offset={offset} limit={limit:?} pages={pages:?}");

        // ── Blocked device paths ────────────────────────────────────────
        for blocked in BLOCKED_PATHS {
            if path_str == *blocked || path_str.starts_with(&format!("{blocked}/")) {
                return Ok(ToolResult::error(format!(
                    "Reading from {path_str} is not allowed (blocked device path)."
                )));
            }
        }

        // ── File existence check ────────────────────────────────────────
        if !path.exists() {
            return Ok(ToolResult::error(format!("File not found: {path_str}")));
        }

        // ── Extension-based dispatch ────────────────────────────────────
        let extension = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        let result = match extension.as_str() {
            "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" => {
                read_image(&path).await?
            }
            #[cfg(feature = "pdf")]
            "pdf" => {
                read_pdf(&path, pages).await?
            }
            #[cfg(not(feature = "pdf"))]
            "pdf" => {
                "[PDF reading disabled — build with --features pdf]".to_string()
            }
            "ipynb" => {
                read_notebook(&path).await?
            }
            _ => {
                // ── Max file size check for text ────────────────────────
                let meta = tokio::fs::metadata(&path).await?;
                if meta.len() > DEFAULT_MAX_TEXT_SIZE {
                    return Ok(ToolResult::error(format!(
                        "File is too large ({} bytes, max {} bytes). \
                         Use offset/limit to read portions, or consider a different approach.",
                        meta.len(),
                        DEFAULT_MAX_TEXT_SIZE,
                    )));
                }

                // ── Binary detection ────────────────────────────────────
                if is_binary_file(&path).await.unwrap_or(false) {
                    return Ok(ToolResult::error(format!(
                        "File appears to be binary: {path_str}. \
                         Use a dedicated tool for binary formats."
                    )));
                }

                read_text(&path, offset, limit).await?
            }
        };

        let _ = output_tx
            .send(ToolOutput { text: result.clone(), is_error: false })
            .await;
        Ok(ToolResult::ok(result))
    }
}

/// Read a text file with `cat -n` style output.
///
/// `offset` is 0-indexed into the concept of "starting line":
///   offset=0 -> start at line 1
///   offset=5 -> start at line 6
async fn read_text(path: &Path, offset: usize, limit: Option<usize>) -> Result<String> {
    let contents = tokio::fs::read_to_string(path).await?;
    let lines: Vec<&str> = contents.lines().collect();

    // offset=0 means start at line 1 (index 0), offset=5 means start at line 6 (index 5)
    let start = offset;
    let end = match limit {
        Some(l) => (start + l).min(lines.len()),
        None => lines.len(),
    };

    if start >= lines.len() {
        return Ok(format!(
            "[File has {} lines, but offset {} is beyond the end]",
            lines.len(),
            offset
        ));
    }

    let selected: Vec<String> = lines[start..end]
        .iter()
        .enumerate()
        .map(|(i, line)| format!("{}\t{}", start + i + 1, line))
        .collect();

    Ok(selected.join("\n"))
}

/// Read an image file and return as a data URI (base64).
async fn read_image(path: &Path) -> Result<String> {
    let bytes = tokio::fs::read(path).await?;
    let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
    let mime = match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
    {
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => "image/png",
    };
    Ok(format!("data:{mime};base64,{encoded}"))
}

/// Read a PDF, optionally filtering to specific page ranges.
#[cfg(feature = "pdf")]
async fn read_pdf(path: &Path, pages: Option<&str>) -> Result<String> {
    let bytes = tokio::fs::read(path).await?;

    // If pages are specified, validate
    if let Some(page_spec) = pages {
        let ranges = parse_page_range(page_spec)?;
        let total = count_pages(&ranges);
        if total > MAX_PDF_PAGES {
            return Ok(format!(
                "[Error: Requested {total} pages but maximum is {MAX_PDF_PAGES} per request. \
                 Please request a smaller range.]"
            ));
        }

        // Extract all text, then filter by page
        // pdf_extract doesn't support per-page extraction well, so we do
        // a best-effort extraction and split by form-feed characters.
        match pdf_extract::extract_text_from_mem(&bytes) {
            Ok(text) => {
                let raw_pages: Vec<&str> = text.split('\u{000C}').collect();
                let mut output = String::new();
                for (start, end) in &ranges {
                    for page_num in *start..=*end {
                        if page_num <= raw_pages.len() {
                            output.push_str(&format!(
                                "--- Page {page_num} ---\n{}\n\n",
                                raw_pages[page_num - 1]
                            ));
                        } else {
                            output.push_str(&format!(
                                "--- Page {page_num} ---\n[Page does not exist (PDF has {} pages)]\n\n",
                                raw_pages.len()
                            ));
                        }
                    }
                }
                Ok(output)
            }
            Err(e) => Ok(format!("[PDF extraction error: {e}]")),
        }
    } else {
        match pdf_extract::extract_text_from_mem(&bytes) {
            Ok(text) => Ok(text),
            Err(e) => Ok(format!("[PDF extraction error: {e}]")),
        }
    }
}

/// Read a Jupyter notebook, rendering all cells with outputs.
async fn read_notebook(path: &Path) -> Result<String> {
    let contents = tokio::fs::read_to_string(path).await?;
    let notebook: Value = serde_json::from_str(&contents)?;

    let mut output = String::new();
    if let Some(cells) = notebook["cells"].as_array() {
        for (i, cell) in cells.iter().enumerate() {
            let cell_type = cell["cell_type"].as_str().unwrap_or("unknown");
            let source = cell["source"]
                .as_array()
                .map(|lines| {
                    lines
                        .iter()
                        .filter_map(|l| l.as_str())
                        .collect::<Vec<_>>()
                        .join("")
                })
                .unwrap_or_default();

            output.push_str(&format!("# Cell {i} [{cell_type}]\n{source}\n\n"));

            if cell_type == "code" {
                if let Some(outputs) = cell["outputs"].as_array() {
                    for out in outputs {
                        let text = out["text"]
                            .as_array()
                            .map(|lines| {
                                lines
                                    .iter()
                                    .filter_map(|l| l.as_str())
                                    .collect::<Vec<_>>()
                                    .join("")
                            })
                            .unwrap_or_default();
                        if !text.is_empty() {
                            output.push_str(&format!("# Output:\n{text}\n\n"));
                        }
                    }
                }
            }
        }
    }
    Ok(output)
}
