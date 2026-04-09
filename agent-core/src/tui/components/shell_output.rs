//! Shell output component — renders streaming command output.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use crate::tui::theme::Theme;

pub struct ShellOutputWidget<'a> {
    lines: &'a [String],
    max_lines: usize,
    theme: &'a Theme,
}

impl<'a> ShellOutputWidget<'a> {
    pub fn new(lines: &'a [String], theme: &'a Theme) -> Self {
        ShellOutputWidget { lines, max_lines: 50, theme }
    }
    pub fn max_lines(mut self, n: usize) -> Self { self.max_lines = n; self }
}

impl<'a> Widget for ShellOutputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let visible = area.height.saturating_sub(2) as usize;
        let skip = self.lines.len().saturating_sub(visible);
        let display_lines: Vec<Line> = self.lines.iter()
            .skip(skip)
            .take(self.max_lines)
            .map(|l| Line::from(Span::styled(l.clone(), self.theme.tool_output_style())))
            .collect();

        Paragraph::new(display_lines)
            .block(Block::default().borders(Borders::ALL).border_style(self.theme.border_style()))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
