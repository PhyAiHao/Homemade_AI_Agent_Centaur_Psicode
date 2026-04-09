//! Vim mode — full vi keybinding emulation for the input handler.
//!
//! Mirrors `src/vim/` (5 files). Provides Normal, Insert, and Visual modes
//! with motions, operators, and text objects.

pub mod types;
pub mod motions;
pub mod operators;
pub mod transitions;
pub mod text_objects;

pub use types::{VimMode, VimState, VimAction};
pub use motions::execute_motion;
pub use transitions::handle_transition;
