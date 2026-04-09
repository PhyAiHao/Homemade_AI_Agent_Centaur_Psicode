//! Markdown renderer — converts markdown text to Ratatui styled spans.
//!
//! Provides basic markdown rendering for the TUI: bold, italic, code,
//! headings, links, and code blocks with syntax highlighting indicators.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::text::{Line, Span};

use super::theme::Theme;

/// Render a markdown string into Ratatui `Line`s.
pub fn render_markdown(text: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for raw_line in text.lines() {
        if raw_line.starts_with("```") {
            if in_code_block {
                // End code block
                in_code_block = false;
                lines.push(Line::from(Span::styled(
                    "```".to_string(),
                    theme.dim_style(),
                )));
            } else {
                // Start code block — parse the language tag from the fence line
                in_code_block = true;
                let code_lang = raw_line.trim_start_matches('`').to_string();
                let header = if code_lang.is_empty() {
                    "```".to_string()
                } else {
                    format!("```{code_lang}")
                };
                lines.push(Line::from(Span::styled(header, theme.dim_style())));
            }
            continue;
        }

        if in_code_block {
            // Code block content — render with dim/monospace style
            lines.push(Line::from(Span::styled(
                raw_line.to_string(),
                theme.tool_output_style(),
            )));
            continue;
        }

        // Headings
        if let Some(stripped) = raw_line.strip_prefix("# ") {
            lines.push(Line::from(Span::styled(
                stripped.to_string(),
                theme.accent_style(),
            )));
            continue;
        }
        if let Some(stripped) = raw_line.strip_prefix("## ") {
            lines.push(Line::from(Span::styled(
                stripped.to_string(),
                Style::default().fg(theme.colors.accent),
            )));
            continue;
        }
        if let Some(stripped) = raw_line.strip_prefix("### ") {
            lines.push(Line::from(Span::styled(
                stripped.to_string(),
                Style::default().fg(theme.colors.accent).italic(),
            )));
            continue;
        }

        // Bullet points
        if raw_line.starts_with("- ") || raw_line.starts_with("* ") {
            lines.push(Line::from(vec![
                Span::styled("  • ".to_string(), theme.accent_style()),
                Span::styled(raw_line[2..].to_string(), theme.assistant_style()),
            ]));
            continue;
        }

        // Numbered lists
        if let Some(rest) = try_parse_numbered_list(raw_line) {
            lines.push(Line::from(Span::styled(
                rest.to_string(),
                theme.assistant_style(),
            )));
            continue;
        }

        // Inline formatting
        lines.push(render_inline_markdown(raw_line, theme));
    }

    lines
}

/// Render inline markdown formatting (bold, italic, code, links).
fn render_inline_markdown(line: &str, theme: &Theme) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut i = 0;
    let chars: Vec<char> = line.chars().collect();

    while i < chars.len() {
        // Inline code: `code`
        if chars[i] == '`' {
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                let code_text: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    code_text,
                    Style::default().fg(theme.colors.tool_name_fg).bg(Color::Rgb(40, 40, 40)),
                ));
                i = end + 1;
                continue;
            }
        }

        // Bold: **text**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_double_closing(&chars, i + 2, '*') {
                let bold_text: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(bold_text, Style::default().bold()));
                i = end + 2;
                continue;
            }
        }

        // Italic: *text*
        if chars[i] == '*' {
            if let Some(end) = find_closing(&chars, i + 1, '*') {
                let italic_text: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(italic_text, Style::default().italic()));
                i = end + 1;
                continue;
            }
        }

        // Plain text — collect until next special char
        let start = i;
        while i < chars.len() && chars[i] != '`' && chars[i] != '*' {
            i += 1;
        }
        let plain: String = chars[start..i].iter().collect();
        if !plain.is_empty() {
            spans.push(Span::raw(plain));
        }
    }

    if spans.is_empty() {
        Line::from("")
    } else {
        Line::from(spans)
    }
}

fn find_closing(chars: &[char], start: usize, delimiter: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == delimiter)
}

fn find_double_closing(chars: &[char], start: usize, delimiter: char) -> Option<usize> {
    (start..chars.len() - 1).find(|&i| chars[i] == delimiter && chars[i + 1] == delimiter)
}

fn try_parse_numbered_list(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    if let Some(dot_pos) = trimmed.find(". ") {
        if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
            return Some(trimmed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_heading() {
        let theme = Theme::dark();
        let lines = render_markdown("# Hello", &theme);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn test_render_code_block() {
        let theme = Theme::dark();
        let lines = render_markdown("```rust\nfn main() {}\n```", &theme);
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn test_render_bullet() {
        let theme = Theme::dark();
        let lines = render_markdown("- item one\n- item two", &theme);
        assert_eq!(lines.len(), 2);
    }
}
