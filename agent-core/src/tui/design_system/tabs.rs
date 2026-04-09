//! Tabs primitive — horizontal tab bar.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Tabs as RatatuiTabs, Widget, Block, Borders};
use crate::tui::theme::Theme;

pub struct TabsWidget<'a> {
    titles: &'a [String],
    selected: usize,
    theme: &'a Theme,
}

impl<'a> TabsWidget<'a> {
    pub fn new(titles: &'a [String], selected: usize, theme: &'a Theme) -> Self {
        TabsWidget { titles, selected, theme }
    }
}

impl<'a> Widget for TabsWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let titles: Vec<Line> = self.titles.iter()
            .map(|t| Line::from(t.as_str()))
            .collect();

        RatatuiTabs::new(titles)
            .block(Block::default().borders(Borders::BOTTOM).border_style(self.theme.border_style()))
            .select(self.selected)
            .style(self.theme.dim_style())
            .highlight_style(self.theme.accent_style())
            .divider(" | ")
            .render(area, buf);
    }
}
