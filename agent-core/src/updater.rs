//! Auto-updater — checks for and applies updates.
//!
//! Mirrors `src/utils/autoUpdater.ts`.
#![allow(dead_code)]

use anyhow::Result;
use serde::Deserialize;

const RELEASE_CHECK_URL: &str = "https://api.github.com/repos/centaur-psicode/agent/releases/latest";
const CHECK_INTERVAL_HOURS: u64 = 24;

/// Version information from the release endpoint.
#[derive(Debug, Deserialize)]
struct ReleaseInfo {
    tag_name: String,
    html_url: String,
    assets: Vec<ReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct ReleaseAsset {
    name: String,
    browser_download_url: String,
    size: u64,
}

/// Update check result.
#[derive(Debug, Clone)]
pub enum UpdateStatus {
    /// Already on the latest version.
    UpToDate,
    /// A newer version is available.
    Available { version: String, url: String },
    /// Check failed.
    CheckFailed(String),
    /// Updates are disabled.
    Disabled,
}

/// Check if a newer version is available.
pub async fn check_for_updates(current_version: &str) -> UpdateStatus {
    let client = match reqwest::Client::builder()
        .user_agent("centaur-psicode-agent")
        .build()
    {
        Ok(c) => c,
        Err(e) => return UpdateStatus::CheckFailed(e.to_string()),
    };

    match client.get(RELEASE_CHECK_URL).send().await {
        Ok(resp) if resp.status().is_success() => {
            match resp.json::<ReleaseInfo>().await {
                Ok(release) => {
                    let latest = release.tag_name.trim_start_matches('v');
                    if is_newer(latest, current_version) {
                        UpdateStatus::Available {
                            version: latest.to_string(),
                            url: release.html_url,
                        }
                    } else {
                        UpdateStatus::UpToDate
                    }
                }
                Err(e) => UpdateStatus::CheckFailed(e.to_string()),
            }
        }
        Ok(resp) => UpdateStatus::CheckFailed(format!("HTTP {}", resp.status())),
        Err(e) => UpdateStatus::CheckFailed(e.to_string()),
    }
}

/// Simple semver comparison (major.minor.patch).
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> (u32, u32, u32) {
        let parts: Vec<u32> = v.split('.').filter_map(|p| p.parse().ok()).collect();
        (
            parts.first().copied().unwrap_or(0),
            parts.get(1).copied().unwrap_or(0),
            parts.get(2).copied().unwrap_or(0),
        )
    };
    parse(latest) > parse(current)
}

/// Record when the last update check was performed.
pub async fn record_check_time() -> Result<()> {
    let path = crate::config::agent_dir()?.join("last_update_check");
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs()
        .to_string();
    tokio::fs::write(path, now).await?;
    Ok(())
}

/// Check if enough time has passed since the last update check.
pub async fn should_check() -> bool {
    let path = match crate::config::agent_dir() {
        Ok(d) => d.join("last_update_check"),
        Err(_) => return true,
    };

    match tokio::fs::read_to_string(&path).await {
        Ok(contents) => {
            let last: u64 = contents.trim().parse().unwrap_or(0);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now - last > CHECK_INTERVAL_HOURS * 3600
        }
        Err(_) => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_comparison() {
        assert!(is_newer("1.1.0", "1.0.0"));
        assert!(is_newer("2.0.0", "1.9.9"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("0.9.0", "1.0.0"));
    }
}
