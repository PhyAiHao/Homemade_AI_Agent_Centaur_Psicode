//! Permission rule loading/saving — mirrors `src/utils/permissions/permissionsLoader.ts`.
//!
//! Loads rules from settings files, saves "Always allow" decisions to disk.
//! Multi-source merging: user > project > local > policy.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use serde_json::Value;
use tracing::{debug, warn};

use super::rule_parser::{parse_rule_string, serialize_rule, RuleValue};
use super::rules::{PermissionRule, RuleEffect};

/// Sources for permission rules, in priority order.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleSource {
    /// User-level: ~/.claude/settings.json
    User,
    /// Project-level: <cwd>/.claude/settings.json
    Project,
    /// Local (gitignored): <cwd>/.claude/settings.local.json
    Local,
    /// Session-only (in-memory).
    Session,
}

impl RuleSource {
    fn settings_path(&self) -> Option<PathBuf> {
        match self {
            RuleSource::User => {
                dirs::home_dir().map(|h| h.join(".claude").join("settings.json"))
            }
            RuleSource::Project => {
                std::env::current_dir().ok().map(|d| d.join(".claude").join("settings.json"))
            }
            RuleSource::Local => {
                std::env::current_dir().ok().map(|d| d.join(".claude").join("settings.local.json"))
            }
            RuleSource::Session => None,
        }
    }
}

/// Load all permission rules from all settings sources on disk.
pub fn load_all_rules() -> Vec<PermissionRule> {
    let mut all_rules = Vec::new();

    for source in &[RuleSource::User, RuleSource::Project, RuleSource::Local] {
        if let Some(path) = source.settings_path() {
            let rules = load_rules_from_file(&path);
            all_rules.extend(rules);
        }
    }

    debug!(count = all_rules.len(), "Loaded permission rules from disk");
    all_rules
}

/// Load permission rules from a single settings file.
fn load_rules_from_file(path: &Path) -> Vec<PermissionRule> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return Vec::new(), // file doesn't exist = no rules
    };

    let json: Value = match serde_json::from_str(&content) {
        Ok(v) => v,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "Failed to parse settings JSON");
            return Vec::new();
        }
    };

    let permissions = match json.get("permissions") {
        Some(p) => p,
        None => return Vec::new(),
    };

    let mut rules = Vec::new();

    // Parse "allow" array
    if let Some(allow_arr) = permissions.get("allow").and_then(|v| v.as_array()) {
        for item in allow_arr {
            if let Some(s) = item.as_str() {
                let parsed = parse_rule_string(s);
                rules.push(PermissionRule {
                    tool: parsed.tool_name,
                    effect: RuleEffect::Allow,
                    reason: None,
                    content: parsed.content,
                    pattern: None,
                });
            }
        }
    }

    // Parse "deny" array
    if let Some(deny_arr) = permissions.get("deny").and_then(|v| v.as_array()) {
        for item in deny_arr {
            if let Some(s) = item.as_str() {
                let parsed = parse_rule_string(s);
                rules.push(PermissionRule {
                    tool: parsed.tool_name,
                    effect: RuleEffect::Deny,
                    reason: Some(format!("Denied by settings: {s}")),
                    content: parsed.content,
                    pattern: None,
                });
            }
        }
    }

    // Parse "ask" array -> RuleEffect::Ask
    if let Some(ask_arr) = permissions.get("ask").and_then(|v| v.as_array()) {
        for item in ask_arr {
            if let Some(s) = item.as_str() {
                let parsed = parse_rule_string(s);
                rules.push(PermissionRule {
                    tool: parsed.tool_name,
                    effect: RuleEffect::Ask,
                    reason: None,
                    content: parsed.content,
                    pattern: None,
                });
            }
        }
    }

    rules
}

/// Save a permission rule to a settings file (for "Always allow").
/// Appends to the `permissions.allow` array, deduplicating.
pub fn save_rule_to_settings(
    tool_name: &str,
    content: Option<&str>,
    destination: &RuleSource,
) -> Result<(), String> {
    let path = destination.settings_path()
        .ok_or_else(|| "Cannot save to session-only source".to_string())?;

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create directory: {e}"))?;
    }

    // Read existing settings (or start with empty object)
    let mut json: Value = if path.exists() {
        let content = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read settings: {e}"))?;
        serde_json::from_str(&content)
            .map_err(|e| format!("Failed to parse settings: {e}"))?
    } else {
        serde_json::json!({})
    };

    // Ensure permissions.allow array exists
    if json.get("permissions").is_none() {
        json["permissions"] = serde_json::json!({});
    }
    if json["permissions"].get("allow").is_none() {
        json["permissions"]["allow"] = serde_json::json!([]);
    }

    // Build the rule string
    let rule_value = RuleValue {
        tool_name: tool_name.to_string(),
        content: content.map(|c| c.to_string()),
    };
    let rule_str = serialize_rule(&rule_value);

    // Deduplicate
    let allow_arr = json["permissions"]["allow"].as_array_mut()
        .ok_or("permissions.allow is not an array")?;
    let already_exists = allow_arr.iter()
        .any(|v| v.as_str() == Some(&rule_str));

    if !already_exists {
        allow_arr.push(Value::String(rule_str.clone()));
    }

    // Write back
    let formatted = serde_json::to_string_pretty(&json)
        .map_err(|e| format!("Failed to serialize settings: {e}"))?;
    std::fs::write(&path, formatted)
        .map_err(|e| format!("Failed to write settings: {e}"))?;

    debug!(rule = %rule_str, path = %path.display(), "Permission rule saved");
    Ok(())
}
