//! Computer use — automated desktop interaction for the agent.
//!
//! Mirrors `src/utils/computerUse/` (10+ files). Provides screen capture,
//! mouse/keyboard input, and application interaction capabilities.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Computer use action types.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ComputerAction {
    /// Take a screenshot.
    Screenshot { display: Option<u32> },
    /// Move the mouse.
    MouseMove { x: u32, y: u32 },
    /// Click at position.
    Click { x: u32, y: u32, button: MouseButton },
    /// Double click.
    DoubleClick { x: u32, y: u32 },
    /// Type text.
    TypeText { text: String },
    /// Press a key combination.
    KeyPress { keys: Vec<String> },
    /// Scroll.
    Scroll { x: u32, y: u32, direction: ScrollDirection, amount: u32 },
    /// Drag from one position to another.
    Drag { from_x: u32, from_y: u32, to_x: u32, to_y: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MouseButton { Left, Right, Middle }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection { Up, Down, Left, Right }

/// Result of a computer use action.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionResult {
    pub success: bool,
    pub screenshot: Option<String>,  // base64 encoded
    pub error: Option<String>,
}

/// Execute a computer use action.
pub async fn execute_action(action: &ComputerAction) -> Result<ActionResult> {
    match action {
        ComputerAction::Screenshot { display } => take_screenshot(*display).await,
        _ => {
            // Other actions require platform-specific implementations
            Ok(ActionResult {
                success: false,
                screenshot: None,
                error: Some("Computer use actions not yet supported on this platform".to_string()),
            })
        }
    }
}

/// Take a screenshot using platform-specific tools.
async fn take_screenshot(_display: Option<u32>) -> Result<ActionResult> {
    #[cfg(target_os = "macos")]
    {
        let tmp = std::env::temp_dir().join("agent_screenshot.png");
        let status = tokio::process::Command::new("screencapture")
            .args(["-x", "-C", tmp.to_str().unwrap()])
            .status()
            .await?;

        if status.success() {
            let data = tokio::fs::read(&tmp).await?;
            use base64::Engine;
            let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
            let _ = tokio::fs::remove_file(&tmp).await;
            Ok(ActionResult { success: true, screenshot: Some(b64), error: None })
        } else {
            Ok(ActionResult { success: false, screenshot: None, error: Some("screencapture failed".into()) })
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(ActionResult {
            success: false,
            screenshot: None,
            error: Some("Screenshot not supported on this platform".to_string()),
        })
    }
}

/// Check if computer use is available on this system.
pub fn is_available() -> bool {
    cfg!(target_os = "macos") || cfg!(target_os = "linux")
}

/// Computer use gate — checks if the feature is enabled.
pub fn is_enabled() -> bool {
    std::env::var("AGENT_COMPUTER_USE")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false)
}
