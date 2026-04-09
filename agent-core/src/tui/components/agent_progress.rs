//! Agent progress component — shows sub-agent execution status.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget};
use crate::tui::theme::Theme;

pub struct AgentProgressWidget<'a> {
    agent_name: &'a str,
    description: &'a str,
    elapsed_secs: u64,
    is_background: bool,
    theme: &'a Theme,
}

impl<'a> AgentProgressWidget<'a> {
    pub fn new(agent_name: &'a str, description: &'a str, elapsed_secs: u64, theme: &'a Theme) -> Self {
        AgentProgressWidget { agent_name, description, elapsed_secs, is_background: false, theme }
    }
    pub fn background(mut self, bg: bool) -> Self { self.is_background = bg; self }
}

impl<'a> Widget for AgentProgressWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let spinner = ['⠋','⠙','⠹','⠸','⠼','⠴','⠦','⠧','⠇','⠏'][(self.elapsed_secs as usize) % 10];
        let bg_label = if self.is_background { " (bg)" } else { "" };
        let line = Line::from(vec![
            Span::styled(format!(" {spinner} "), self.theme.tool_active_style()),
            Span::styled(format!("{}{bg_label}: ", self.agent_name), self.theme.accent_style()),
            Span::styled(self.description, self.theme.dim_style()),
            Span::styled(format!(" ({:.0}s)", self.elapsed_secs), self.theme.dim_style()),
        ]);
        Paragraph::new(line).render(area, buf);
    }
}
