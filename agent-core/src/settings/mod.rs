//! Settings management — validation, MDM overrides, schema output.
//!
//! Mirrors `src/utils/settings/` (10 files) and `src/utils/settings/mdm/`.
#![allow(dead_code)]

pub mod mdm;
pub mod validation;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

/// Resolved settings combining config file, MDM policy, and environment variables.
/// MDM policy always wins over user config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolvedSettings {
    pub model: String,
    pub permission_mode: String,
    pub disable_telemetry: bool,
    pub vim_mode: bool,
    pub theme: String,
    pub max_tool_calls_per_turn: u32,
    pub custom_system_prompt: String,
    pub allowed_dirs: Vec<std::path::PathBuf>,
    /// Fields locked by MDM policy (user cannot change these)
    pub locked_fields: Vec<String>,
}

/// Load and merge settings from all sources.
pub async fn load_resolved() -> Result<ResolvedSettings> {
    let config = crate::config::Config::load().await?;
    let mdm    = mdm::load().await.unwrap_or_default();

    let mut locked_fields = Vec::new();

    // MDM overrides win
    let model = if let Some(v) = &mdm.model {
        locked_fields.push("model".into());
        v.clone()
    } else {
        config.model.clone()
    };

    let permission_mode = if let Some(v) = &mdm.permission_mode {
        locked_fields.push("permissionMode".into());
        v.clone()
    } else {
        config.permission_mode.clone()
    };

    let disable_telemetry = mdm.disable_telemetry.unwrap_or(config.disable_telemetry);

    debug!("Settings resolved: model={model}, locked={:?}", locked_fields);

    Ok(ResolvedSettings {
        model,
        permission_mode,
        disable_telemetry,
        vim_mode: config.vim_mode,
        theme: config.theme,
        max_tool_calls_per_turn: config.max_tool_calls_per_turn,
        custom_system_prompt: config.custom_system_prompt,
        allowed_dirs: config.allowed_dirs,
        locked_fields,
    })
}

/// Check whether a field is locked by MDM policy.
pub fn is_locked(settings: &ResolvedSettings, field: &str) -> bool {
    settings.locked_fields.iter().any(|f| f == field)
}
