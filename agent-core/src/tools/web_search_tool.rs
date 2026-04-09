//! WebSearchTool — performs web searches using DuckDuckGo HTML.
//!
//! Native Rust implementation using reqwest. Parses DuckDuckGo's HTML
//! search results page to extract titles, URLs, and snippets.
//! No API key required.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc::Sender;
use tracing::debug;

use super::{Tool, ToolOutput, ToolResult};

/// Request timeout for search.
const SEARCH_TIMEOUT_SECS: u64 = 15;

pub struct WebSearchTool;

#[async_trait]
impl Tool for WebSearchTool {
    fn name(&self) -> &'static str { "WebSearch" }

    fn description(&self) -> &str {
        "Search the web using DuckDuckGo and return results with titles, URLs, \
         and snippets. Supports domain allow/block lists for filtering."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query (min 2 characters)"
                },
                "allowed_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Only return results from these domains"
                },
                "blocked_domains": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Exclude results from these domains"
                }
            },
            "required": ["query"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(&self, input: Value, tx: Sender<ToolOutput>) -> Result<ToolResult> {
        let query = input["query"].as_str()
            .ok_or_else(|| anyhow::anyhow!("query is required"))?;

        if query.len() < 2 {
            return Ok(ToolResult::error("Query must be at least 2 characters"));
        }

        let allowed_domains: Vec<String> = input["allowed_domains"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        let blocked_domains: Vec<String> = input["blocked_domains"]
            .as_array()
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();

        if !allowed_domains.is_empty() && !blocked_domains.is_empty() {
            return Ok(ToolResult::error(
                "Cannot specify both allowed_domains and blocked_domains"
            ));
        }

        let _ = tx.send(ToolOutput {
            text: format!("Searching: {query}"),
            is_error: false,
        }).await;

        debug!("WebSearch: {query}");

        // Build the DuckDuckGo search URL with domain restriction
        let mut search_query = query.to_string();
        for domain in &allowed_domains {
            search_query = format!("{search_query} site:{domain}");
        }
        for domain in &blocked_domains {
            search_query = format!("{search_query} -site:{domain}");
        }

        let encoded_query = urlencoding::encode(&search_query);
        let url = format!("https://html.duckduckgo.com/html/?q={encoded_query}");

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(SEARCH_TIMEOUT_SECS))
            .user_agent("Mozilla/5.0 (compatible; CentaurAgent/1.0)")
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to build HTTP client: {e}"))?;

        let response = match client.get(&url).send().await {
            Ok(r) => r,
            Err(e) => {
                return Ok(ToolResult::error(format!("Search request failed: {e}")));
            }
        };

        if !response.status().is_success() {
            return Ok(ToolResult::error(format!(
                "Search returned HTTP {}", response.status()
            )));
        }

        let body = match response.text().await {
            Ok(t) => t,
            Err(e) => {
                return Ok(ToolResult::error(format!("Failed to read search results: {e}")));
            }
        };

        // Parse DuckDuckGo HTML results
        let results = parse_ddg_results(&body);

        if results.is_empty() {
            let result = format!("No results found for: {query}");
            let _ = tx.send(ToolOutput { text: result.clone(), is_error: false }).await;
            return Ok(ToolResult::ok(result));
        }

        // Format results
        let mut output = format!("Search results for: {query}\n\n");
        for (i, r) in results.iter().enumerate().take(10) {
            output.push_str(&format!(
                "{}. {}\n   {}\n   {}\n\n",
                i + 1, r.title, r.url, r.snippet
            ));
        }
        output.push_str(&format!("({} results shown)", results.len().min(10)));

        let _ = tx.send(ToolOutput {
            text: format!("Found {} results for: {query}", results.len()),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(output))
    }
}

#[derive(Debug)]
struct SearchResult {
    title: String,
    url: String,
    snippet: String,
}

/// Parse DuckDuckGo HTML search results page.
fn parse_ddg_results(html: &str) -> Vec<SearchResult> {
    let mut results = Vec::new();

    // DuckDuckGo HTML results are in <div class="result"> blocks
    // Each has: <a class="result__a"> for title+URL, <a class="result__snippet"> for snippet
    for chunk in html.split("class=\"result__body\"") {
        if results.len() >= 15 { break; }

        // Extract URL and title from result__a
        let title_url = extract_between(chunk, "class=\"result__a\"", "</a>");
        let title_html = match title_url {
            Some(t) => t,
            None => continue,
        };

        let url = extract_href(title_html).unwrap_or_default();
        let title = strip_html_tags(title_html);

        if url.is_empty() || title.is_empty() { continue; }

        // Extract snippet
        let snippet = extract_between(chunk, "class=\"result__snippet\"", "</a>")
            .or_else(|| extract_between(chunk, "class=\"result__snippet\"", "</td>"))
            .map(strip_html_tags)
            .unwrap_or_default();

        // Decode DuckDuckGo redirect URLs
        let clean_url = decode_ddg_url(&url);

        results.push(SearchResult {
            title: title.trim().to_string(),
            url: clean_url,
            snippet: snippet.trim().to_string(),
        });
    }

    results
}

fn extract_between<'a>(text: &'a str, start_marker: &str, end_marker: &str) -> Option<&'a str> {
    let start = text.find(start_marker)?;
    let after_start = start + start_marker.len();
    // Find the closing > of the opening tag
    let tag_close = text[after_start..].find('>')?;
    let content_start = after_start + tag_close + 1;
    let end = text[content_start..].find(end_marker)?;
    Some(&text[content_start..content_start + end])
}

fn extract_href(html: &str) -> Option<String> {
    let href_start = html.find("href=\"")?;
    let url_start = href_start + 6;
    let url_end = html[url_start..].find('"')?;
    Some(html[url_start..url_start + url_end].to_string())
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        if c == '<' { in_tag = true; continue; }
        if c == '>' { in_tag = false; continue; }
        if !in_tag { result.push(c); }
    }
    // Decode common entities
    result.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&nbsp;", " ")
}

/// Decode DuckDuckGo redirect URLs like //duckduckgo.com/l/?uddg=https%3A%2F%2F...
fn decode_ddg_url(url: &str) -> String {
    if url.contains("duckduckgo.com/l/?uddg=") {
        if let Some(uddg_start) = url.find("uddg=") {
            let encoded = &url[uddg_start + 5..];
            let end = encoded.find('&').unwrap_or(encoded.len());
            let encoded_url = &encoded[..end];
            return urlencoding::decode(encoded_url)
                .unwrap_or_else(|_| encoded_url.into())
                .to_string();
        }
    }
    // Clean up protocol-relative URLs
    if url.starts_with("//") {
        return format!("https:{url}");
    }
    url.to_string()
}
