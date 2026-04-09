//! Bootstrap — parallel startup prefetch.
//!
//! Mirrors `src/bootstrap/state.ts` and `src/entrypoints/init.ts`.
//! Fires several I/O operations in parallel before the TUI is shown,
//! shaving startup latency by ~100ms.

use anyhow::Result;
use std::path::PathBuf;
use tracing::{debug, warn};

use crate::config::Config;

/// Everything resolved at startup, passed through the application.
pub struct Bootstrap {
    pub config: Config,
    pub api_key: Option<String>,
    pub working_dir: PathBuf,
    pub model: String,
    pub bypass_permissions: bool,
    pub bare_mode: bool,
    pub auth_status: crate::auth::AuthStatus,
}

impl Bootstrap {
    /// Perform parallel startup tasks and return a ready Bootstrap context.
    pub async fn new(cli: &crate::Cli) -> Result<Self> {
        debug!("Bootstrap: starting parallel prefetch");

        // Run config load, keychain read, and auth detection concurrently
        let (config_result, api_key_result, auth_status) = tokio::join!(
            Config::load(),
            prefetch_api_key(),
            crate::auth::detect_auth_status(),
        );

        let config = config_result.unwrap_or_else(|e| {
            warn!("Failed to load config, using defaults: {e}");
            Config::default()
        });

        let api_key = api_key_result.unwrap_or_else(|_| {
            // Fall back to env var
            std::env::var("ANTHROPIC_API_KEY").ok()
        });

        // Resolve working directory
        let working_dir = match &cli.dir {
            Some(d) => d.clone(),
            None => std::env::current_dir()?,
        };

        // Model priority: CLI flag > env var > config > smart default
        let model = cli.model.clone()
            .or_else(|| std::env::var("CLAUDE_MODEL").ok())
            .unwrap_or_else(|| {
                // If no Anthropic key is set, don't default to a Claude model.
                // Try to detect a local model from Ollama instead.
                if std::env::var("ANTHROPIC_API_KEY").is_err() && config.model.starts_with("claude") {
                    detect_local_model().unwrap_or_else(|| config.model.clone())
                } else {
                    config.model.clone()
                }
            });

        debug!(
            "Bootstrap complete: model={model}, cwd={}, auth={:?}",
            working_dir.display(),
            auth_status.auth_type
        );

        Ok(Bootstrap {
            config,
            api_key,
            working_dir,
            model,
            bypass_permissions: cli.dangerously_bypass_permissions,
            bare_mode: cli.bare,
            auth_status,
        })
    }
}

/// Detect the first available local model from Ollama.
/// Runs `ollama list` and picks the first model name.
fn detect_local_model() -> Option<String> {
    let output = std::process::Command::new("ollama")
        .arg("list")
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Skip header line, take first model name (e.g. "gemma4:31b")
    let first_model = stdout.lines()
        .skip(1) // skip "NAME  ID  SIZE  MODIFIED" header
        .next()?
        .split_whitespace()
        .next()?
        .to_string();
    if first_model.is_empty() { None } else { Some(first_model) }
}

/// Attempt to read the API key from the system keychain (macOS Keychain / SecretService).
/// Falls back silently — env var is checked in Bootstrap::new().
async fn prefetch_api_key() -> Result<Option<String>> {
    tokio::task::spawn_blocking(|| {
        let entry = keyring::Entry::new("centaur-psicode", "anthropic-api-key")?;
        match entry.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(anyhow::anyhow!("Keychain error: {e}")),
        }
    })
    .await?
}
