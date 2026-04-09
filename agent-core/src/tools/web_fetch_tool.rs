//! WebFetchTool — fetches web pages and converts HTML to readable text.
//!
//! Native Rust implementation using reqwest. Converts HTML to a simplified
//! markdown-like text format for the model to consume.
#![allow(dead_code)]

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// Max response body size (5 MB).
const MAX_BODY_BYTES: usize = 5 * 1024 * 1024;
/// Max content to return to the model (100K chars).
const MAX_CONTENT_CHARS: usize = 100_000;
/// Request timeout.
const FETCH_TIMEOUT_SECS: u64 = 30;

pub struct WebFetchTool;

#[async_trait]
impl Tool for WebFetchTool {
    fn name(&self) -> &'static str { "WebFetch" }

    fn description(&self) -> &str {
        "Fetch a web page and return its content as readable text. HTML pages are \
         converted to a simplified format. Returns the page title, URL, and content."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "The URL to fetch"
                },
                "prompt": {
                    "type": "string",
                    "description": "Optional: focus extraction on specific content (e.g. 'extract the API docs')"
                }
            },
            "required": ["url"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let url = input["url"].as_str()
            .ok_or_else(|| anyhow::anyhow!("url is required"))?;
        let _prompt = input["prompt"].as_str().unwrap_or("");

        // Validate URL
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return Ok(ToolResult::error(format!(
                "Invalid URL: {url}. Must start with http:// or https://"
            )));
        }

        let _ = tx.send(ToolOutput {
            text: format!("Fetching {url}..."),
            is_error: false,
        }).await;

        debug!("WebFetch: {url}");

        // Fetch the page
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(FETCH_TIMEOUT_SECS))
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent("Mozilla/5.0 (compatible; CentaurAgent/1.0)")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {e}"))?;

        let response = match client.get(url).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!("Fetch failed: {e}")));
            }
        };

        let status = response.status();
        let final_url = response.url().to_string();
        let content_type = response.headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        if !status.is_success() {
            return Ok(ToolResult::error(format!(
                "HTTP {status} for {url}"
            )));
        }

        // Read body with size limit
        let bytes = match response.bytes().await {
            Ok(b) => b,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to read response body: {e}")));
            }
        };

        if bytes.len() > MAX_BODY_BYTES {
            return Ok(ToolResult::error(format!(
                "Response too large ({} bytes, max {})", bytes.len(), MAX_BODY_BYTES
            )));
        }

        let body = String::from_utf8_lossy(&bytes).to_string();

        // Convert based on content type
        let content = if content_type.contains("text/html") {
            html_to_text(&body)
        } else if content_type.contains("application/json") {
            // Pretty-print JSON
            match serde_json::from_str::<Value>(&body) {
                Ok(v) => serde_json::to_string_pretty(&v).unwrap_or(body),
                Err(_) => body,
            }
        } else {
            body
        };

        // Truncate if too long
        let truncated = if content.len() > MAX_CONTENT_CHARS {
            format!(
                "{}\n\n... [Content truncated at {} chars, total {} chars]",
                &content[..MAX_CONTENT_CHARS],
                MAX_CONTENT_CHARS,
                content.len()
            )
        } else {
            content
        };

        // Build result
        let redirect_note = if final_url != url {
            format!("\nRedirected to: {final_url}")
        } else {
            String::new()
        };

        let result = format!(
            "URL: {url}{redirect_note}\nContent-Type: {content_type}\n\
             Content ({} chars):\n\n{truncated}",
            truncated.len()
        );

        let _ = tx.send(ToolOutput {
            text: format!("Fetched {} ({} chars)", url, truncated.len()),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(result))
    }
}

/// Convert HTML to readable plain text.
/// Strips tags, extracts text content, preserves basic structure.
fn html_to_text(html: &str) -> String {
    let mut result = String::new();
    let mut in_script = false;
    let mut in_style = false;
    let mut tag_name = String::new();
    let mut last_was_newline = false;

    let chars: Vec<char> = html.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        if c == '<' {
            tag_name.clear();
            i += 1;
            // Collect tag name
            let closing = if i < chars.len() && chars[i] == '/' {
                i += 1;
                true
            } else {
                false
            };
            while i < chars.len() && chars[i] != '>' && chars[i] != ' ' {
                tag_name.push(chars[i].to_ascii_lowercase());
                i += 1;
            }
            // Skip to end of tag
            while i < chars.len() && chars[i] != '>' {
                i += 1;
            }
            if i < chars.len() { i += 1; } // skip >

            // Handle block-level tags
            match tag_name.as_str() {
                "script" if !closing => { in_script = true; }
                "script" if closing => { in_script = false; }
                "style" if !closing => { in_style = true; }
                "style" if closing => { in_style = false; }
                "p" | "div" | "br" | "li" | "tr" | "h1" | "h2" | "h3"
                | "h4" | "h5" | "h6" | "blockquote" | "pre" if !closing => {
                    if !last_was_newline {
                        result.push('\n');
                        last_was_newline = true;
                    }
                    // Add heading markers
                    if tag_name.starts_with('h') && tag_name.len() == 2 {
                        let level = tag_name.chars().nth(1).unwrap_or('1');
                        let hashes = "#".repeat(level.to_digit(10).unwrap_or(1) as usize);
                        result.push_str(&format!("{hashes} "));
                    }
                    if tag_name == "li" {
                        result.push_str("- ");
                    }
                }
                _ => {}
            }
            continue;
        }

        if in_script || in_style {
            i += 1;
            continue;
        }

        // Decode HTML entities
        if c == '&' {
            let mut entity = String::from("&");
            i += 1;
            while i < chars.len() && chars[i] != ';' && entity.len() < 10 {
                entity.push(chars[i]);
                i += 1;
            }
            if i < chars.len() { i += 1; } // skip ;
            let decoded = match entity.as_str() {
                "&amp" => "&",
                "&lt" => "<",
                "&gt" => ">",
                "&quot" => "\"",
                "&apos" => "'",
                "&nbsp" => " ",
                _ => " ",
            };
            result.push_str(decoded);
            last_was_newline = false;
            continue;
        }

        // Regular text
        if c == '\n' || c == '\r' {
            if !last_was_newline {
                result.push('\n');
                last_was_newline = true;
            }
        } else if c == ' ' || c == '\t' {
            if !result.ends_with(' ') && !last_was_newline {
                result.push(' ');
            }
        } else {
            result.push(c);
            last_was_newline = false;
        }
        i += 1;
    }

    // Clean up excessive newlines
    let mut cleaned = String::new();
    let mut consecutive_newlines = 0;
    for c in result.chars() {
        if c == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                cleaned.push(c);
            }
        } else {
            consecutive_newlines = 0;
            cleaned.push(c);
        }
    }

    cleaned.trim().to_string()
}
