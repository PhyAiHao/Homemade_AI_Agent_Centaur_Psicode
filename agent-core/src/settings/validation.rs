//! Settings validation — type-checks and allowed-value checks for config fields.
#![allow(dead_code)]

use anyhow::{bail, Result};

const VALID_PERMISSION_MODES: &[&str] = &["default", "autoApprove", "planOnly", "bypass"];
const VALID_THEMES: &[&str] = &["dark", "light", "system"];

pub fn validate_model(model: &str) -> Result<()> {
    if model.is_empty() {
        bail!("Model cannot be empty");
    }
    Ok(())
}

pub fn validate_permission_mode(mode: &str) -> Result<()> {
    if !VALID_PERMISSION_MODES.contains(&mode) {
        bail!(
            "Invalid permission mode '{mode}'. Valid: {}",
            VALID_PERMISSION_MODES.join(", ")
        );
    }
    Ok(())
}

pub fn validate_theme(theme: &str) -> Result<()> {
    if !VALID_THEMES.contains(&theme) {
        bail!(
            "Invalid theme '{theme}'. Valid: {}",
            VALID_THEMES.join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_permission_modes() {
        assert!(validate_permission_mode("default").is_ok());
        assert!(validate_permission_mode("autoApprove").is_ok());
        assert!(validate_permission_mode("invalid").is_err());
    }

    #[test]
    fn test_valid_themes() {
        assert!(validate_theme("dark").is_ok());
        assert!(validate_theme("neon").is_err());
    }
}
