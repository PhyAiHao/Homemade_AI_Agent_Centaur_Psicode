//! Screens — full-screen views for REPL, Doctor, and Resume.
//!
//! Mirrors `src/screens/` (3 files). Each screen owns its own rendering
//! and input handling, composing from the component library.
#![allow(dead_code)]

pub mod repl;
pub mod doctor;
pub mod resume;
pub mod settings;
pub mod setup_wizard;

/// The active screen in the application.
#[derive(Debug, Clone, PartialEq)]
pub enum Screen {
    /// Main REPL / chat screen.
    Repl,
    /// Environment diagnostic screen.
    Doctor,
    /// Resume a previous conversation.
    Resume { session_id: Option<String> },
}
