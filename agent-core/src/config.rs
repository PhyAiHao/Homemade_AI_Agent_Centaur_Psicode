//! Global configuration — load/save `~/.agent/config.json`.
//!
//! Mirrors `src/utils/config.js`.
#![allow(dead_code)]

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::debug;

/// Application-wide configuration persisted to `~/.agent/config.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Schema version — used by the migration system
    #[serde(default = "default_schema_version")]
    pub schema_version: u32,

    /// Default model
    #[serde(default = "default_model")]
    pub model: String,

    /// Permission mode: "default" | "autoApprove" | "planOnly" | "bypass"
    #[serde(default = "default_permission_mode")]
    pub permission_mode: String,

    /// Enable vim keybinding mode
    #[serde(default)]
    pub vim_mode: bool,

    /// Active output style name (empty = default)
    #[serde(default)]
    pub output_style: String,

    /// Active theme name
    #[serde(default = "default_theme")]
    pub theme: String,

    /// Enable auto-update checks
    #[serde(default = "default_true")]
    pub auto_update: bool,

    /// Disable all telemetry
    #[serde(default)]
    pub disable_telemetry: bool,

    /// Maximum tool calls per turn
    #[serde(default = "default_max_tool_calls")]
    pub max_tool_calls_per_turn: u32,

    /// Custom system prompt addition (appended after base system prompt)
    #[serde(default)]
    pub custom_system_prompt: String,

    /// List of MCP server configurations
    #[serde(default)]
    pub mcp_servers: Vec<McpServerConfig>,

    /// Allowed directories for file operations (empty = unrestricted)
    #[serde(default)]
    pub allowed_dirs: Vec<PathBuf>,

    /// LLM provider: "first_party" (Anthropic), "openai", "gemini", "ollama"
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Maximum concurrent sub-agents (default: 4)
    #[serde(default = "default_max_agents")]
    pub max_agents: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfig {
    pub name: String,
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schema_version: default_schema_version(),
            model: default_model(),
            permission_mode: default_permission_mode(),
            vim_mode: false,
            output_style: String::new(),
            theme: default_theme(),
            auto_update: true,
            disable_telemetry: false,
            max_tool_calls_per_turn: default_max_tool_calls(),
            custom_system_prompt: String::new(),
            mcp_servers: Vec::new(),
            allowed_dirs: Vec::new(),
            provider: default_provider(),
            max_agents: default_max_agents(),
        }
    }
}

impl Config {
    /// Load from `~/.agent/config.json`. Returns default if the file doesn't exist.
    pub async fn load() -> Result<Self> {
        let path = config_path()?;
        if !path.exists() {
            debug!("Config file not found, using defaults");
            return Ok(Self::default());
        }

        let contents = tokio::fs::read_to_string(&path)
            .await
            .with_context(|| format!("Reading config at {}", path.display()))?;

        let mut config: Self = serde_json::from_str(&contents)
            .with_context(|| "Parsing config JSON")?;

        // Run migrations if needed
        config = crate::migrations::run(config).await?;

        debug!("Config loaded (schema_version={})", config.schema_version);
        Ok(config)
    }

    /// Persist config to `~/.agent/config.json`.
    pub async fn save(&self) -> Result<()> {
        let path = config_path()?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let contents = serde_json::to_string_pretty(self)?;
        tokio::fs::write(&path, contents).await
            .with_context(|| format!("Writing config to {}", path.display()))?;

        debug!("Config saved to {}", path.display());
        Ok(())
    }

    /// Read a single field by key. Returns a JSON value.
    pub fn get_field(&self, key: &str) -> serde_json::Value {
        match key {
            "model"                   => serde_json::Value::String(self.model.clone()),
            "permission_mode"         => serde_json::Value::String(self.permission_mode.clone()),
            "vim_mode"                => serde_json::Value::Bool(self.vim_mode),
            "output_style"            => serde_json::Value::String(self.output_style.clone()),
            "theme"                   => serde_json::Value::String(self.theme.clone()),
            "auto_update"             => serde_json::Value::Bool(self.auto_update),
            "disable_telemetry"       => serde_json::Value::Bool(self.disable_telemetry),
            "max_tool_calls_per_turn" => serde_json::json!(self.max_tool_calls_per_turn),
            "custom_system_prompt"    => serde_json::Value::String(self.custom_system_prompt.clone()),
            "provider"                => serde_json::Value::String(self.provider.clone()),
            "max_agents"              => serde_json::json!(self.max_agents),
            _ => serde_json::Value::Null,
        }
    }

    /// Update a single field by key with a JSON value. Used by ConfigTool.
    pub fn set_field(&mut self, key: &str, value: serde_json::Value) {
        match key {
            "model"                   => if let Some(s) = value.as_str() { self.model = s.to_string(); },
            "permission_mode"         => if let Some(s) = value.as_str() { self.permission_mode = s.to_string(); },
            "vim_mode"                => if let Some(b) = value.as_bool() { self.vim_mode = b; },
            "output_style"            => if let Some(s) = value.as_str() { self.output_style = s.to_string(); },
            "theme"                   => if let Some(s) = value.as_str() { self.theme = s.to_string(); },
            "auto_update"             => if let Some(b) = value.as_bool() { self.auto_update = b; },
            "disable_telemetry"       => if let Some(b) = value.as_bool() { self.disable_telemetry = b; },
            "max_tool_calls_per_turn" => if let Some(n) = value.as_u64() { self.max_tool_calls_per_turn = n as u32; },
            "custom_system_prompt"    => if let Some(s) = value.as_str() { self.custom_system_prompt = s.to_string(); },
            "provider"                => if let Some(s) = value.as_str() { self.provider = s.to_string(); },
            "max_agents"              => if let Some(n) = value.as_u64() { self.max_agents = n as u32; },
            _ => {}
        }
    }

    /// Update a single field by key string. Legacy convenience wrapper.
    pub async fn set_field_str(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "model"          => self.model = value.to_string(),
            "permissionMode" => self.permission_mode = value.to_string(),
            "vimMode"        => self.vim_mode = value.parse()?,
            "theme"          => self.theme = value.to_string(),
            "autoUpdate"     => self.auto_update = value.parse()?,
            "disableTelemetry" => self.disable_telemetry = value.parse()?,
            _ => anyhow::bail!("Unknown config key: {key}"),
        }
        self.save().await
    }
}

/// Returns the path to the config directory: `~/.agent/`
pub fn agent_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Cannot determine home directory")?;
    Ok(home.join(".agent"))
}

/// Returns `~/.agent/config.json`
pub fn config_path() -> Result<PathBuf> {
    Ok(agent_dir()?.join("config.json"))
}

// ---- Default value helpers ----

fn default_schema_version() -> u32 { 1 }
fn default_model() -> String { "claude-sonnet-4-6".into() }
fn default_permission_mode() -> String { "default".into() }
fn default_theme() -> String { "dark".into() }
fn default_max_tool_calls() -> u32 { 50 }
fn default_true() -> bool { true }
fn default_provider() -> String { "first_party".into() }
fn default_max_agents() -> u32 { 4 }

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn test_config_default() {
        let cfg = Config::default();
        assert_eq!(cfg.model, "claude-sonnet-4-6");
        assert_eq!(cfg.schema_version, 1);
    }

    #[tokio::test]
    async fn test_config_round_trip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("config.json");

        let original = Config {
            model: "claude-opus-4-6".into(),
            vim_mode: true,
            ..Config::default()
        };

        let json = serde_json::to_string_pretty(&original).unwrap();
        tokio::fs::write(&path, json).await.unwrap();

        let loaded: Config = serde_json::from_str(
            &tokio::fs::read_to_string(&path).await.unwrap()
        ).unwrap();

        assert_eq!(loaded.model, "claude-opus-4-6");
        assert!(loaded.vim_mode);
    }
}
