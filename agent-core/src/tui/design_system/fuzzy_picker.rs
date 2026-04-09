//! Fuzzy picker — filtered list with keyboard selection.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget};
use crate::tui::theme::Theme;

pub struct FuzzyPickerWidget<'a> {
    query: &'a str,
    items: &'a [String],
    selected: usize,
    title: &'a str,
    theme: &'a Theme,
}

impl<'a> FuzzyPickerWidget<'a> {
    pub fn new(query: &'a str, items: &'a [String], selected: usize, theme: &'a Theme) -> Self {
        FuzzyPickerWidget { query, items, selected, title: "Select", theme }
    }
    pub fn title(mut self, title: &'a str) -> Self { self.title = title; self }
}

impl<'a> FuzzyPickerWidget<'a> {
    fn filtered_items(&self) -> Vec<(usize, &'a String)> {
        if self.query.is_empty() {
            self.items.iter().enumerate().collect()
        } else {
            let q = self.query.to_lowercase();
            self.items.iter().enumerate()
                .filter(|(_, item)| item.to_lowercase().contains(&q))
                .collect()
        }
    }
}

impl<'a> Widget for FuzzyPickerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let w = area.width.min(50);
        let h = area.height.min(15);
        let x = (area.width.saturating_sub(w)) / 2 + area.x;
        let y = (area.height.saturating_sub(h)) / 2 + area.y;
        let dialog = Rect::new(x, y, w, h);
        Clear.render(dialog, buf);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(dialog);

        Paragraph::new(self.query.to_string())
            .block(Block::default().title(format!(" {} ", self.title)).borders(Borders::ALL).border_style(self.theme.accent_style()))
            .render(chunks[0], buf);

        let filtered = self.filtered_items();
        let items: Vec<ListItem> = filtered.iter().enumerate().map(|(i, (_, item))| {
            let style = if i == self.selected { Style::default().reversed() } else { self.theme.assistant_style() };
            ListItem::new(Span::styled(item.to_string(), style))
        }).collect();

        Widget::render(List::new(items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM).border_style(self.theme.border_style())),
            chunks[1], buf);
    }
}
