//! Search dialog — global search with fuzzy matching.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget};
use crate::tui::theme::Theme;

pub struct SearchDialogWidget<'a> {
    query: &'a str,
    results: &'a [SearchResult],
    selected: usize,
    theme: &'a Theme,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub label: String,
    pub description: String,
    pub score: f64,
}

impl<'a> SearchDialogWidget<'a> {
    pub fn new(query: &'a str, results: &'a [SearchResult], selected: usize, theme: &'a Theme) -> Self {
        SearchDialogWidget { query, results, selected, theme }
    }
}

impl<'a> Widget for SearchDialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width.min(60);
        let h = area.height.min(20);
        let x = (area.width.saturating_sub(w)) / 2 + area.x;
        let y = (area.height.saturating_sub(h)) / 2 + area.y;
        let dialog = Rect::new(x, y, w, h);

        Clear.render(dialog, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(dialog);

        // Search input
        let input = Paragraph::new(self.query.to_string())
            .block(Block::default().title(" Search ").borders(Borders::ALL).border_style(self.theme.accent_style()))
            .style(self.theme.input_style());
        input.render(chunks[0], buf);

        // Results
        let items: Vec<ListItem> = self.results.iter().enumerate().map(|(i, r)| {
            let style = if i == self.selected {
                Style::default().reversed()
            } else {
                self.theme.assistant_style()
            };
            ListItem::new(Line::from(vec![
                Span::styled(&r.label, style.bold()),
                Span::styled(format!("  {}", r.description), self.theme.dim_style()),
            ]))
        }).collect();

        Widget::render(List::new(items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM).border_style(self.theme.border_style())),
            chunks[1], buf);
    }
}
