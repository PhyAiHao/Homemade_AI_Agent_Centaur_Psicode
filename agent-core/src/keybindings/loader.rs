//! Keybinding loader — loads user bindings from ~/.agent/keybindings.json.

use anyhow::{Context, Result};
use super::schema::Keybinding;

/// Load user keybindings from the config directory.
pub async fn load_user_bindings() -> Result<Vec<Keybinding>> {
    let path = crate::config::agent_dir()?.join("keybindings.json");

    if !path.exists() {
        return Ok(Vec::new());
    }

    let contents = tokio::fs::read_to_string(&path)
        .await
        .with_context(|| format!("Reading keybindings from {}", path.display()))?;

    let bindings: Vec<Keybinding> = serde_json::from_str(&contents)
        .with_context(|| "Parsing keybindings.json")?;

    // Mark as user bindings
    let bindings = bindings.into_iter().map(|mut b| {
        b.source = "user".to_string();
        b
    }).collect();

    Ok(bindings)
}

/// Save keybindings to the config directory.
pub async fn save_user_bindings(bindings: &[Keybinding]) -> Result<()> {
    let path = crate::config::agent_dir()?.join("keybindings.json");
    let contents = serde_json::to_string_pretty(bindings)?;
    tokio::fs::write(&path, contents).await?;
    Ok(())
}
