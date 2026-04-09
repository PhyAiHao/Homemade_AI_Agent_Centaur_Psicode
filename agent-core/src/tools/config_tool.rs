//! ConfigTool — get or set agent configuration settings.
//!
//! Mirrors `src/tools/ConfigTool/`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{Tool, ToolOutput, ToolResult};
use crate::config::Config;

const KNOWN_SETTINGS: &[&str] = &[
    "model", "permission_mode", "vim_mode", "output_style",
    "theme", "auto_update", "disable_telemetry",
    "max_tool_calls_per_turn", "custom_system_prompt",
];

pub struct ConfigTool;

#[async_trait]
impl Tool for ConfigTool {
    fn name(&self) -> &'static str { "Config" }

    fn description(&self) -> &str {
        "Get or set agent configuration settings (theme, model, permissions mode, etc)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "setting": {
                    "type": "string",
                    "description": "Setting key (e.g. \"theme\", \"model\", \"permission_mode\")"
                },
                "value": {
                    "description": "New value; omit to read current value"
                }
            },
            "required": ["setting"]
        })
    }

    fn requires_permission(&self) -> bool { true }

    async fn execute(
        &self,
        input: Value,
        output_tx: mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let setting = input.get("setting")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        if setting.is_empty() {
            return Ok(ToolResult::error("Missing required parameter: setting"));
        }

        if !KNOWN_SETTINGS.contains(&setting) {
            return Ok(ToolResult::error(format!(
                "Unknown setting: {setting}. Known settings: {}",
                KNOWN_SETTINGS.join(", ")
            )));
        }

        let mut config = Config::load().await.unwrap_or_default();

        // Read mode (no value provided)
        if input.get("value").is_none() || input.get("value") == Some(&Value::Null) {
            let current = config.get_field(setting);
            let _ = output_tx.send(ToolOutput {
                text: format!("{setting} = {current}"),
                is_error: false,
            }).await;
            return Ok(ToolResult::ok(format!("{setting} = {current}")));
        }

        // Write mode
        let new_value = match input.get("value") {
            Some(v) => v,
            None => return Ok(ToolResult::error("Missing 'value' parameter".to_string())),
        };
        let old_value = config.get_field(setting);

        config.set_field(setting, new_value.clone());
        if let Err(e) = config.save().await {
            return Ok(ToolResult::error(format!("Failed to save config: {e}")));
        }

        let msg = format!("{setting}: {old_value} → {new_value}");
        let _ = output_tx.send(ToolOutput {
            text: msg.clone(),
            is_error: false,
        }).await;

        Ok(ToolResult::ok(msg))
    }
}
