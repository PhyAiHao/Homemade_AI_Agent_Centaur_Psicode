//! Vim text objects — iw, aw, i", a(, etc.
#![allow(dead_code)]

/// A text object selection (start, end) in the buffer.
#[derive(Debug, Clone)]
pub struct TextObject {
    pub start: usize,
    pub end: usize,
}

/// Select an "inner word" at the cursor position.
pub fn inner_word(text: &str, cursor: usize) -> TextObject {
    let bytes = text.as_bytes();
    let mut start = cursor;
    let mut end = cursor;

    // Expand backward over word chars
    while start > 0 && !bytes[start - 1].is_ascii_whitespace() {
        start -= 1;
    }
    // Expand forward over word chars
    while end < bytes.len() && !bytes[end].is_ascii_whitespace() {
        end += 1;
    }

    TextObject { start, end }
}

/// Select "a word" (inner word + trailing whitespace).
pub fn a_word(text: &str, cursor: usize) -> TextObject {
    let mut obj = inner_word(text, cursor);
    let bytes = text.as_bytes();

    // Include trailing whitespace
    while obj.end < bytes.len() && bytes[obj.end].is_ascii_whitespace() {
        obj.end += 1;
    }

    obj
}

/// Select inside matching delimiters (e.g., i", i', i(, i[, i{).
pub fn inner_delimited(text: &str, cursor: usize, open: char, close: char) -> Option<TextObject> {
    let chars: Vec<char> = text.chars().collect();

    // Search backward for opening delimiter
    let mut depth = 0i32;
    let mut start = None;
    for i in (0..=cursor.min(chars.len().saturating_sub(1))).rev() {
        if chars[i] == close && i != cursor { depth += 1; }
        if chars[i] == open {
            if depth == 0 { start = Some(i + 1); break; }
            depth -= 1;
        }
    }

    let start = start?;

    // Search forward for closing delimiter
    depth = 0;
    let mut end = None;
    for (i, &ch) in chars.iter().enumerate().skip(cursor) {
        if ch == open && i != start.saturating_sub(1) { depth += 1; }
        if ch == close {
            if depth == 0 { end = Some(i); break; }
            depth -= 1;
        }
    }

    let end = end?;

    // Convert char indices to byte indices
    let byte_start: usize = chars[..start].iter().map(|c| c.len_utf8()).sum();
    let byte_end: usize = chars[..end].iter().map(|c| c.len_utf8()).sum();

    Some(TextObject { start: byte_start, end: byte_end })
}

/// Select inside quotes.
pub fn inner_quoted(text: &str, cursor: usize, quote: char) -> Option<TextObject> {
    inner_delimited(text, cursor, quote, quote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inner_word() {
        let text = "hello world";
        let obj = inner_word(text, 2);
        assert_eq!(obj.start, 0);
        assert_eq!(obj.end, 5);
        assert_eq!(&text[obj.start..obj.end], "hello");
    }

    #[test]
    fn test_a_word() {
        let text = "hello world";
        let obj = a_word(text, 2);
        assert_eq!(&text[obj.start..obj.end], "hello ");
    }

    #[test]
    fn test_inner_parens() {
        let text = "foo(bar baz)qux";
        let obj = inner_delimited(text, 5, '(', ')').unwrap();
        assert_eq!(&text[obj.start..obj.end], "bar baz");
    }
}
