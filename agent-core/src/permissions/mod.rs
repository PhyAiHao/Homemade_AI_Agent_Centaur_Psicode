//! Permission system — gate checked on every tool invocation.
//!
//! Mirrors `src/hooks/toolPermission/` (15+ files) and `src/utils/permissions/` (15+ files).

pub mod gate;
pub mod rules;
pub mod mode;
pub mod rule_parser;
pub mod dangerous_patterns;
pub mod denial_tracking;
pub mod loader;
pub mod shadowed;
pub mod explainer;

/// Extract `file_path` from tool input JSON.
pub(crate) fn extract_file_path(input_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(input_json)
        .ok()
        .and_then(|v| v.get("file_path").and_then(|p| p.as_str()).map(|s| s.to_string()))
}

/// Extract `command` from tool input JSON, falling back to the raw input.
pub(crate) fn extract_command(input_json: &str) -> String {
    serde_json::from_str::<serde_json::Value>(input_json)
        .ok()
        .and_then(|v| v.get("command").and_then(|c| c.as_str()).map(|s| s.to_string()))
        .unwrap_or_else(|| input_json.to_string())
}

