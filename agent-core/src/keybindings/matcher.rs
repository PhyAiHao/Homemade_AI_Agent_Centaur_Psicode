//! Key event matcher — matches crossterm key events against keybindings.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use super::schema::KeyChord;

/// Check if a crossterm KeyEvent matches a KeyChord.
pub fn match_event(event: &KeyEvent, chord: &KeyChord) -> bool {
    // Check modifiers
    let ctrl_match = chord.ctrl == event.modifiers.contains(KeyModifiers::CONTROL);
    let alt_match = chord.alt == event.modifiers.contains(KeyModifiers::ALT);
    let shift_match = chord.shift == event.modifiers.contains(KeyModifiers::SHIFT);

    if !ctrl_match || !alt_match || !shift_match {
        return false;
    }

    // Check key
    match &event.code {
        KeyCode::Char(c) => chord.key == c.to_lowercase().to_string(),
        KeyCode::Enter => chord.key == "enter",
        KeyCode::Esc => chord.key == "escape",
        KeyCode::Tab => chord.key == "tab",
        KeyCode::Backspace => chord.key == "backspace",
        KeyCode::Delete => chord.key == "delete",
        KeyCode::Left => chord.key == "left",
        KeyCode::Right => chord.key == "right",
        KeyCode::Up => chord.key == "up",
        KeyCode::Down => chord.key == "down",
        KeyCode::Home => chord.key == "home",
        KeyCode::End => chord.key == "end",
        KeyCode::PageUp => chord.key == "pageup",
        KeyCode::PageDown => chord.key == "pagedown",
        KeyCode::F(n) => chord.key == format!("f{n}"),
        _ => false,
    }
}
