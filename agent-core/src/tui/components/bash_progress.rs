//! Bash progress component — shows streaming shell output with exit code.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Widget, Wrap};
use crate::tui::theme::Theme;

pub struct BashProgressWidget<'a> {
    command: &'a str,
    output_lines: &'a [String],
    exit_code: Option<i32>,
    is_running: bool,
    elapsed_secs: u64,
    theme: &'a Theme,
}

impl<'a> BashProgressWidget<'a> {
    pub fn new(command: &'a str, output_lines: &'a [String], theme: &'a Theme) -> Self {
        BashProgressWidget { command, output_lines, exit_code: None, is_running: true, elapsed_secs: 0, theme }
    }
    pub fn exit_code(mut self, code: i32) -> Self { self.exit_code = Some(code); self.is_running = false; self }
    pub fn elapsed(mut self, secs: u64) -> Self { self.elapsed_secs = secs; self }
}

impl<'a> Widget for BashProgressWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let status = if self.is_running {
            let spinner = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'][(self.elapsed_secs as usize) % 10];
            format!("{spinner} running ({:.0}s)", self.elapsed_secs)
        } else {
            match self.exit_code {
                Some(0) => "done".to_string(),
                Some(c) => format!("exit {c}"),
                None => "done".to_string(),
            }
        };

        let title = format!(" $ {} [{}] ", self.command, status);
        let border_style = match self.exit_code {
            Some(0) | None => self.theme.border_style(),
            Some(_) => self.theme.error_style(),
        };

        let visible = area.height.saturating_sub(2) as usize;
        let skip = self.output_lines.len().saturating_sub(visible);
        let lines: Vec<Line> = self.output_lines.iter().skip(skip)
            .map(|l| Line::from(Span::styled(l.clone(), self.theme.tool_output_style())))
            .collect();

        Paragraph::new(lines)
            .block(Block::default().title(title).borders(Borders::ALL).border_style(border_style))
            .wrap(Wrap { trim: false })
            .render(area, buf);
    }
}
