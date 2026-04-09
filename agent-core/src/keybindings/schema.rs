//! Keybinding schema — data types for bindings.

use serde::{Deserialize, Serialize};

/// A single key chord (e.g., Ctrl+Shift+P).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct KeyChord {
    pub key: String,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
}

/// An action triggered by a keybinding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Action {
    pub command: String,
    #[serde(default)]
    pub args: Option<serde_json::Value>,
}

/// A complete keybinding definition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Keybinding {
    /// Key chord(s) — single or sequence.
    pub keys: Vec<KeyChord>,
    /// Action to trigger.
    pub action: Action,
    /// Context condition (e.g., "inputFocused").
    #[serde(default)]
    pub when: Option<String>,
    /// Priority (higher wins).
    #[serde(default)]
    pub priority: i32,
    /// Source: "default", "user", "reserved".
    #[serde(default = "default_source")]
    pub source: String,
}

fn default_source() -> String { "default".to_string() }
