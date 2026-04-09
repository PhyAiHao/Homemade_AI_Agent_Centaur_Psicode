//! Format shortcuts for display (e.g., "Ctrl+Shift+P").

use super::schema::KeyChord;

/// Format a key chord for human display.
pub fn format_chord(chord: &KeyChord) -> String {
    let mut parts = Vec::new();
    if chord.ctrl { parts.push("Ctrl"); }
    if chord.alt { parts.push("Alt"); }
    if chord.shift { parts.push("Shift"); }
    if chord.meta {
        #[cfg(target_os = "macos")]
        parts.push("⌘");
        #[cfg(not(target_os = "macos"))]
        parts.push("Super");
    }
    let key_display = display_key(&chord.key);
    parts.push(&key_display);
    parts.join("+")
}

/// Format a binding (possibly multi-chord) for display.
pub fn format_binding(chords: &[KeyChord]) -> String {
    chords.iter().map(format_chord).collect::<Vec<_>>().join(" ")
}

fn display_key(key: &str) -> String {
    match key {
        "escape" => "Esc".to_string(),
        "enter" => "Enter".to_string(),
        "tab" => "Tab".to_string(),
        "space" => "Space".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Del".to_string(),
        "pageup" => "PgUp".to_string(),
        "pagedown" => "PgDn".to_string(),
        k if k.len() == 1 => k.to_uppercase(),
        k => k.to_string(),
    }
}
