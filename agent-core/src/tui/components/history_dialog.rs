//! History dialog — searchable prompt history.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget};
use crate::tui::theme::Theme;

pub struct HistoryDialogWidget<'a> {
    entries: &'a [String],
    filter: &'a str,
    selected: usize,
    theme: &'a Theme,
}

impl<'a> HistoryDialogWidget<'a> {
    pub fn new(entries: &'a [String], filter: &'a str, selected: usize, theme: &'a Theme) -> Self {
        HistoryDialogWidget { entries, filter, selected, theme }
    }
}

impl<'a> Widget for HistoryDialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width.min(70);
        let h = area.height.min(20);
        let x = (area.width.saturating_sub(w)) / 2 + area.x;
        let y = (area.height.saturating_sub(h)) / 2 + area.y;
        let dialog = Rect::new(x, y, w, h);

        Clear.render(dialog, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(dialog);

        let input = Paragraph::new(self.filter.to_string())
            .block(Block::default().title(" History ").borders(Borders::ALL).border_style(self.theme.accent_style()));
        input.render(chunks[0], buf);

        let filtered: Vec<&String> = self.entries.iter()
            .filter(|e| self.filter.is_empty() || e.to_lowercase().contains(&self.filter.to_lowercase()))
            .collect();

        let items: Vec<ListItem> = filtered.iter().enumerate().map(|(i, entry)| {
            let style = if i == self.selected { Style::default().reversed() } else { self.theme.assistant_style() };
            let truncated = if entry.len() > 60 { format!("{}...", &entry[..60]) } else { entry.to_string() };
            ListItem::new(Span::styled(truncated, style))
        }).collect();

        Widget::render(List::new(items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM).border_style(self.theme.border_style())),
            chunks[1], buf);
    }
}
