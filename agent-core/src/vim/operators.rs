//! Vim operators — d, c, y, >, <.
#![allow(dead_code)]

use super::types::{Operator, VimAction, VimState, VimMode};

/// Execute an operator over a range in the text buffer.
pub fn execute_operator(
    state: &mut VimState,
    text: &mut String,
    operator: &Operator,
    start: usize,
    end: usize,
) -> VimAction {
    let (lo, hi) = if start <= end { (start, end) } else { (end, start) };
    let hi = hi.min(text.len());

    match operator {
        Operator::Delete => {
            let deleted: String = text[lo..hi].to_string();
            state.clipboard = deleted;
            text.drain(lo..hi);
            VimAction::DeleteRange(lo, hi)
        }
        Operator::Change => {
            let deleted: String = text[lo..hi].to_string();
            state.clipboard = deleted;
            text.drain(lo..hi);
            state.mode = VimMode::Insert;
            VimAction::ChangeMode(VimMode::Insert)
        }
        Operator::Yank => {
            state.clipboard = text[lo..hi].to_string();
            VimAction::YankRange(lo, hi)
        }
        Operator::Indent => {
            // Insert spaces at line start (simplified for single-line input)
            text.insert_str(lo, "  ");
            VimAction::None
        }
        Operator::Dedent => {
            // Remove leading spaces
            if text[lo..].starts_with("  ") {
                text.drain(lo..lo + 2);
            }
            VimAction::None
        }
    }
}
