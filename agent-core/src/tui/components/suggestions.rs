//! Suggestions component — shows contextual prompt suggestions.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::theme::Theme;

pub struct SuggestionsWidget<'a> {
    suggestions: &'a [String],
    selected: usize,
    theme: &'a Theme,
}

impl<'a> SuggestionsWidget<'a> {
    pub fn new(suggestions: &'a [String], selected: usize, theme: &'a Theme) -> Self {
        SuggestionsWidget { suggestions, selected, theme }
    }
}

impl<'a> Widget for SuggestionsWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        for (i, suggestion) in self.suggestions.iter().enumerate().take(area.height as usize) {
            let style = if i == self.selected {
                Style::default().fg(self.theme.colors.accent).bold().reversed()
            } else {
                self.theme.dim_style()
            };
            lines.push(Line::from(Span::styled(format!("  {suggestion}"), style)));
        }
        Paragraph::new(lines).render(area, buf);
    }
}
