//! MDM (Mobile Device Management) settings reader.
//!
//! Mirrors `src/utils/settings/mdm/` (rawRead.ts, settings.ts, constants.ts).
//! On macOS, reads managed preferences from:
//!   /Library/Managed Preferences/<bundle_id>.plist
//! Falls back to a JSON file at `~/.agent/mdm_policy.json` for testing.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::debug;

const BUNDLE_ID: &str = "com.centaur-psicode.agent";

/// MDM-enforced policy fields. All are optional — absent means "not managed".
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MdmPolicy {
    /// Force a specific model
    pub model: Option<String>,
    /// Force a permission mode
    pub permission_mode: Option<String>,
    /// Force-disable telemetry
    pub disable_telemetry: Option<bool>,
    /// Restrict to these directories only
    pub allowed_dirs: Option<Vec<std::path::PathBuf>>,
    /// Disallow plugins
    pub disable_plugins: Option<bool>,
}

/// Attempt to read MDM policy. Returns default (all None) if unavailable.
pub async fn load() -> Result<MdmPolicy> {
    // Try macOS managed preferences plist
    #[cfg(target_os = "macos")]
    if let Ok(policy) = read_macos_managed_prefs().await {
        debug!("MDM policy loaded from managed preferences");
        return Ok(policy);
    }

    // Fall back to JSON file (useful for testing)
    let fallback_path = crate::config::agent_dir()?.join("mdm_policy.json");
    if fallback_path.exists() {
        let contents = tokio::fs::read_to_string(&fallback_path).await?;
        let policy: MdmPolicy = serde_json::from_str(&contents)?;
        debug!("MDM policy loaded from fallback JSON");
        return Ok(policy);
    }

    debug!("No MDM policy found — using defaults");
    Ok(MdmPolicy::default())
}

#[cfg(target_os = "macos")]
async fn read_macos_managed_prefs() -> Result<MdmPolicy> {
    let plist_path = format!("/Library/Managed Preferences/{BUNDLE_ID}.plist");
    if !std::path::Path::new(&plist_path).exists() {
        anyhow::bail!("No managed preferences plist");
    }
    let contents = tokio::fs::read_to_string(&plist_path).await?;
    // Simple JSON-compatible plist parsing (assumes plist converted to JSON format)
    let policy: MdmPolicy = serde_json::from_str(&contents)?;
    Ok(policy)
}
