//! Keybinding system — configurable keyboard shortcuts.
//!
//! Mirrors `src/keybindings/` (14 files). Provides a layered binding system:
//!   1. Reserved bindings (Ctrl-C, etc.) — cannot be overridden
//!   2. User bindings (~/.agent/keybindings.json)
//!   3. Default bindings
#![allow(dead_code)]

pub mod schema;
pub mod parser;
pub mod matcher;
pub mod resolver;
pub mod defaults;
pub mod loader;
pub mod format;
pub mod reserved;
pub mod context;

