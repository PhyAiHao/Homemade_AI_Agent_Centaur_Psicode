//! Default keybinding definitions.

use super::schema::*;
use super::parser::parse_chord;

/// Return the default keybinding set.
pub fn default_bindings() -> Vec<Keybinding> {
    vec![
        binding("ctrl+c", "quit", None),
        binding("enter", "submit", Some("inputFocused")),
        binding("escape", "cancel", None),
        binding("ctrl+l", "clear", None),
        binding("ctrl+r", "search_history", Some("inputFocused")),
        binding("ctrl+p", "command_palette", None),
        binding("pageup", "scroll_up", None),
        binding("pagedown", "scroll_down", None),
        binding("ctrl+u", "scroll_half_up", None),
        binding("ctrl+d", "scroll_half_down", None),
        binding("tab", "accept_suggestion", Some("suggestionsVisible")),
        binding("ctrl+n", "next_suggestion", Some("suggestionsVisible")),
        binding("ctrl+p", "prev_suggestion", Some("suggestionsVisible")),
        binding("f1", "help", None),
        binding("ctrl+o", "open_file", None),
        binding("ctrl+s", "save_session", None),
    ]
}

fn binding(keys_str: &str, command: &str, when: Option<&str>) -> Keybinding {
    Keybinding {
        keys: keys_str.split_whitespace()
            .map(|k| parse_chord(k).unwrap_or(KeyChord {
                key: k.to_string(), ctrl: false, alt: false, shift: false, meta: false,
            }))
            .collect(),
        action: Action { command: command.to_string(), args: None },
        when: when.map(|s| s.to_string()),
        priority: 0,
        source: "default".to_string(),
    }
}
