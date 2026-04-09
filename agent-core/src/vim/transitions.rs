//! Vim transitions — mode switching logic.

use crossterm::event::{KeyCode, KeyEvent};

use super::types::*;

/// Handle a key event in vim mode, producing an action.
pub fn handle_transition(
    state: &mut VimState,
    key: KeyEvent,
    text: &str,
    cursor: usize,
) -> VimAction {
    match state.mode {
        VimMode::Normal => handle_normal(state, key, text, cursor),
        VimMode::Insert => handle_insert(state, key),
        VimMode::Visual => handle_visual(state, key, text, cursor),
        VimMode::Replace => handle_replace(state, key),
    }
}

fn handle_normal(state: &mut VimState, key: KeyEvent, text: &str, cursor: usize) -> VimAction {
    match key.code {
        // Mode transitions
        KeyCode::Char('i') => {
            state.mode = VimMode::Insert;
            VimAction::ChangeMode(VimMode::Insert)
        }
        KeyCode::Char('a') => {
            state.mode = VimMode::Insert;
            VimAction::MoveCursor(Motion::Right)
        }
        KeyCode::Char('I') => {
            state.mode = VimMode::Insert;
            VimAction::MoveCursor(Motion::FirstNonBlank)
        }
        KeyCode::Char('A') => {
            state.mode = VimMode::Insert;
            VimAction::MoveCursor(Motion::LineEnd)
        }
        KeyCode::Char('v') => {
            state.mode = VimMode::Visual;
            state.visual_anchor = Some(cursor);
            VimAction::ChangeMode(VimMode::Visual)
        }
        KeyCode::Char('R') => {
            state.mode = VimMode::Replace;
            VimAction::ChangeMode(VimMode::Replace)
        }

        // Motions
        KeyCode::Char('h') | KeyCode::Left  => VimAction::MoveCursor(Motion::Left),
        KeyCode::Char('l') | KeyCode::Right => VimAction::MoveCursor(Motion::Right),
        KeyCode::Char('w') => VimAction::MoveCursor(Motion::WordForward),
        KeyCode::Char('b') => VimAction::MoveCursor(Motion::WordBackward),
        KeyCode::Char('e') => VimAction::MoveCursor(Motion::WordEnd),
        KeyCode::Char('0') if state.count.is_none() => VimAction::MoveCursor(Motion::LineStart),
        KeyCode::Char('$') => VimAction::MoveCursor(Motion::LineEnd),
        KeyCode::Char('^') => VimAction::MoveCursor(Motion::FirstNonBlank),

        // Numeric prefix
        KeyCode::Char(c @ '1'..='9') => {
            if let Some(digit) = c.to_digit(10) {
                state.count = Some(state.count.unwrap_or(0) * 10 + digit);
            }
            VimAction::None
        }
        KeyCode::Char('0') if state.count.is_some() => {
            state.count = state.count.map(|n| n * 10);
            VimAction::None
        }

        // Operators
        KeyCode::Char('d') => {
            if state.pending_operator == Some(Operator::Delete) {
                // dd — delete whole line
                state.pending_operator = None;
                VimAction::DeleteRange(0, text.len())
            } else {
                state.pending_operator = Some(Operator::Delete);
                VimAction::None
            }
        }
        KeyCode::Char('c') => {
            if state.pending_operator == Some(Operator::Change) {
                state.pending_operator = None;
                state.mode = VimMode::Insert;
                VimAction::DeleteRange(0, text.len())
            } else {
                state.pending_operator = Some(Operator::Change);
                VimAction::None
            }
        }
        KeyCode::Char('y') => {
            if state.pending_operator == Some(Operator::Yank) {
                state.pending_operator = None;
                VimAction::YankRange(0, text.len())
            } else {
                state.pending_operator = Some(Operator::Yank);
                VimAction::None
            }
        }

        // Paste
        KeyCode::Char('p') => VimAction::Paste(PastePosition::After),
        KeyCode::Char('P') => VimAction::Paste(PastePosition::Before),

        // Delete char
        KeyCode::Char('x') => VimAction::DeleteRange(cursor, cursor + 1),
        KeyCode::Char('X') => {
            if cursor > 0 { VimAction::DeleteRange(cursor - 1, cursor) }
            else { VimAction::None }
        }

        // Undo
        KeyCode::Char('u') => VimAction::Undo,

        // Escape cancels pending operator
        KeyCode::Esc => {
            state.pending_operator = None;
            state.count = None;
            VimAction::None
        }

        _ => VimAction::None,
    }
}

fn handle_insert(state: &mut VimState, key: KeyEvent) -> VimAction {
    match key.code {
        KeyCode::Esc => {
            state.mode = VimMode::Normal;
            VimAction::ChangeMode(VimMode::Normal)
        }
        KeyCode::Char(c) => VimAction::Insert(c),
        _ => VimAction::None,
    }
}

fn handle_visual(state: &mut VimState, key: KeyEvent, _text: &str, cursor: usize) -> VimAction {
    match key.code {
        KeyCode::Esc => {
            state.mode = VimMode::Normal;
            state.visual_anchor = None;
            VimAction::ChangeMode(VimMode::Normal)
        }
        // Motions extend the selection
        KeyCode::Char('h') | KeyCode::Left  => VimAction::MoveCursor(Motion::Left),
        KeyCode::Char('l') | KeyCode::Right => VimAction::MoveCursor(Motion::Right),
        KeyCode::Char('w') => VimAction::MoveCursor(Motion::WordForward),
        KeyCode::Char('b') => VimAction::MoveCursor(Motion::WordBackward),
        // Operators on selection
        KeyCode::Char('d') | KeyCode::Char('x') => {
            state.mode = VimMode::Normal;
            let anchor = state.visual_anchor.unwrap_or(cursor);
            state.visual_anchor = None;
            VimAction::DeleteRange(anchor.min(cursor), anchor.max(cursor) + 1)
        }
        KeyCode::Char('y') => {
            state.mode = VimMode::Normal;
            let anchor = state.visual_anchor.unwrap_or(cursor);
            state.visual_anchor = None;
            VimAction::YankRange(anchor.min(cursor), anchor.max(cursor) + 1)
        }
        _ => VimAction::None,
    }
}

fn handle_replace(state: &mut VimState, key: KeyEvent) -> VimAction {
    match key.code {
        KeyCode::Esc => {
            state.mode = VimMode::Normal;
            VimAction::ChangeMode(VimMode::Normal)
        }
        KeyCode::Char(c) => {
            state.mode = VimMode::Normal;
            VimAction::Insert(c) // In full impl, this replaces under cursor
        }
        _ => VimAction::None,
    }
}
