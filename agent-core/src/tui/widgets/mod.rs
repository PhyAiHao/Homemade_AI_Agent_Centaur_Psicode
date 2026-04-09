//! TUI widgets — reusable Ratatui components.
//!
//! Mirrors `src/components/` (100+ React components). In Ratatui, widgets
//! are simple structs that implement `Widget` for immediate-mode rendering.
#![allow(dead_code)]

pub mod message_list;
pub mod tool_status;
pub mod permission_dialog;
pub mod cost_display;
pub mod spinner;

