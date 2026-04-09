//! Diff view component — renders file diffs with syntax coloring.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};

use crate::tui::theme::Theme;

pub struct DiffViewWidget<'a> {
    file_path: &'a str,
    diff_text: &'a str,
    theme: &'a Theme,
}

impl<'a> DiffViewWidget<'a> {
    pub fn new(file_path: &'a str, diff_text: &'a str, theme: &'a Theme) -> Self {
        DiffViewWidget { file_path, diff_text, theme }
    }
}

impl<'a> Widget for DiffViewWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();

        for line in self.diff_text.lines() {
            let (text, style) = if line.starts_with('+') && !line.starts_with("+++") {
                (line, Style::default().fg(Color::Green))
            } else if line.starts_with('-') && !line.starts_with("---") {
                (line, Style::default().fg(Color::Red))
            } else if line.starts_with("@@") {
                (line, Style::default().fg(Color::Cyan))
            } else if line.starts_with("diff ") || line.starts_with("index ") {
                (line, self.theme.dim_style())
            } else {
                (line, self.theme.assistant_style())
            };
            lines.push(Line::from(Span::styled(text.to_string(), style)));
        }

        let block = Block::default()
            .title(format!(" {} ", self.file_path))
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
