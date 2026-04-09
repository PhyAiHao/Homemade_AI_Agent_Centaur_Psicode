//! Reserved shortcuts — cannot be overridden by user bindings.

use super::schema::*;
use super::parser::parse_chord;

/// Return the set of reserved keybindings.
pub fn reserved_bindings() -> Vec<Keybinding> {
    vec![
        reserved("ctrl+c", "quit"),
        reserved("ctrl+z", "suspend"),
    ]
}

/// Check if a key chord is reserved.
pub fn is_reserved(chord: &KeyChord) -> bool {
    let reserved = reserved_bindings();
    reserved.iter().any(|b| b.keys.contains(chord))
}

fn reserved(keys_str: &str, command: &str) -> Keybinding {
    Keybinding {
        keys: vec![parse_chord(keys_str).unwrap()],
        action: Action { command: command.to_string(), args: None },
        when: None,
        priority: 1000,
        source: "reserved".to_string(),
    }
}
