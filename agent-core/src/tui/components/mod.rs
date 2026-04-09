//! Application components — full set of Ratatui widgets for the agent TUI.
//!
//! Mirrors `src/components/` (100+ React components), translated to
//! immediate-mode Ratatui widgets.
#![allow(dead_code)]

pub mod message;
pub mod text_input;
pub mod status_line;
pub mod notices;
pub mod suggestions;
pub mod diff_view;
pub mod bash_progress;
pub mod agent_progress;
pub mod tasks_panel;
pub mod search_dialog;
pub mod history_dialog;
pub mod memory_viewer;
pub mod skills_panel;
pub mod agents_panel;
pub mod mcp_status;
pub mod shell_output;
pub mod agent_wizard;
