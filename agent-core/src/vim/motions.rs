//! Vim motions — cursor movement commands.
#![allow(dead_code)]

use super::types::Motion;

/// Execute a motion on a text buffer, returning the new cursor position.
pub fn execute_motion(
    text: &str,
    cursor: usize,
    motion: &Motion,
    count: u32,
) -> usize {
    let mut pos = cursor;
    for _ in 0..count {
        pos = single_motion(text, pos, motion);
    }
    pos
}

fn single_motion(text: &str, cursor: usize, motion: &Motion) -> usize {
    let bytes = text.as_bytes();
    let len = bytes.len();

    match motion {
        Motion::Left => cursor.saturating_sub(1),
        Motion::Right => (cursor + 1).min(len.saturating_sub(1)),

        Motion::WordForward => {
            let mut i = cursor;
            // Skip current word
            while i < len && !bytes[i].is_ascii_whitespace() { i += 1; }
            // Skip whitespace
            while i < len && bytes[i].is_ascii_whitespace() { i += 1; }
            i.min(len)
        }
        Motion::WordBackward => {
            let mut i = cursor;
            // Skip preceding whitespace
            while i > 0 && bytes[i.saturating_sub(1)].is_ascii_whitespace() { i -= 1; }
            // Skip word
            while i > 0 && !bytes[i.saturating_sub(1)].is_ascii_whitespace() { i -= 1; }
            i
        }
        Motion::WordEnd => {
            let mut i = cursor + 1;
            if i >= len { return cursor; }
            // Skip whitespace
            while i < len && bytes[i].is_ascii_whitespace() { i += 1; }
            // Skip to end of word
            while i < len && !bytes[i].is_ascii_whitespace() { i += 1; }
            (i.saturating_sub(1)).min(len.saturating_sub(1))
        }

        Motion::LineStart => 0,
        Motion::LineEnd => len.saturating_sub(1),
        Motion::FirstNonBlank => {
            bytes.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(0)
        }

        Motion::FindChar(ch) => {
            let target = *ch as u8;
            bytes[cursor + 1..].iter()
                .position(|b| *b == target)
                .map(|p| cursor + 1 + p)
                .unwrap_or(cursor)
        }
        Motion::TillChar(ch) => {
            let target = *ch as u8;
            bytes[cursor + 1..].iter()
                .position(|b| *b == target)
                .map(|p| cursor + p)
                .unwrap_or(cursor)
        }

        Motion::Top => 0,
        Motion::Bottom => len.saturating_sub(1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_word_forward() {
        let text = "hello world foo";
        assert_eq!(execute_motion(text, 0, &Motion::WordForward, 1), 6);
        assert_eq!(execute_motion(text, 6, &Motion::WordForward, 1), 12);
    }

    #[test]
    fn test_word_backward() {
        let text = "hello world";
        assert_eq!(execute_motion(text, 8, &Motion::WordBackward, 1), 6);
        assert_eq!(execute_motion(text, 6, &Motion::WordBackward, 1), 0);
    }

    #[test]
    fn test_line_boundaries() {
        let text = "  hello";
        assert_eq!(execute_motion(text, 4, &Motion::LineStart, 1), 0);
        assert_eq!(execute_motion(text, 0, &Motion::FirstNonBlank, 1), 2);
    }

    #[test]
    fn test_find_char() {
        let text = "hello world";
        assert_eq!(execute_motion(text, 0, &Motion::FindChar('o'), 1), 4);
        assert_eq!(execute_motion(text, 0, &Motion::TillChar('o'), 1), 3);
    }
}
