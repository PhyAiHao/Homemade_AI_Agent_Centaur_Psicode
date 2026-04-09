//! Key chord parser — parses strings like "ctrl+shift+p" into KeyChord.

use super::schema::KeyChord;

/// Parse a key chord string (e.g., "ctrl+shift+p", "alt+enter", "escape").
pub fn parse_chord(input: &str) -> Result<KeyChord, String> {
    let lowered = input.to_lowercase();
    let parts: Vec<&str> = lowered.split('+').collect();
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut meta = false;
    let mut key = String::new();

    for part in &parts {
        match *part {
            "ctrl" | "control" => ctrl = true,
            "alt" | "option" => alt = true,
            "shift" => shift = true,
            "meta" | "cmd" | "command" | "super" | "win" => meta = true,
            k => {
                if !key.is_empty() {
                    return Err(format!("Multiple keys in chord: {key} and {k}"));
                }
                key = normalize_key_name(k);
            }
        }
    }

    if key.is_empty() {
        return Err(format!("No key in chord: {input}"));
    }

    Ok(KeyChord { key, ctrl, alt, shift, meta })
}

/// Parse a binding string that may contain chord sequences (space-separated).
pub fn parse_binding(input: &str) -> Result<Vec<KeyChord>, String> {
    input.split_whitespace()
        .map(parse_chord)
        .collect()
}

fn normalize_key_name(name: &str) -> String {
    match name {
        "esc" | "escape" => "escape".to_string(),
        "enter" | "return" | "cr" => "enter".to_string(),
        "tab" => "tab".to_string(),
        "space" | " " => "space".to_string(),
        "backspace" | "bs" => "backspace".to_string(),
        "delete" | "del" => "delete".to_string(),
        "up" | "arrowup" => "up".to_string(),
        "down" | "arrowdown" => "down".to_string(),
        "left" | "arrowleft" => "left".to_string(),
        "right" | "arrowright" => "right".to_string(),
        "pageup" | "pgup" => "pageup".to_string(),
        "pagedown" | "pgdn" | "pgdown" => "pagedown".to_string(),
        "home" => "home".to_string(),
        "end" => "end".to_string(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_key() {
        let chord = parse_chord("a").unwrap();
        assert_eq!(chord.key, "a");
        assert!(!chord.ctrl);
    }

    #[test]
    fn test_ctrl_key() {
        let chord = parse_chord("ctrl+c").unwrap();
        assert_eq!(chord.key, "c");
        assert!(chord.ctrl);
    }

    #[test]
    fn test_complex_chord() {
        let chord = parse_chord("ctrl+shift+p").unwrap();
        assert_eq!(chord.key, "p");
        assert!(chord.ctrl);
        assert!(chord.shift);
    }

    #[test]
    fn test_normalize() {
        let chord = parse_chord("Esc").unwrap();
        assert_eq!(chord.key, "escape");
    }

    #[test]
    fn test_chord_sequence() {
        let chords = parse_binding("ctrl+k ctrl+c").unwrap();
        assert_eq!(chords.len(), 2);
    }
}
