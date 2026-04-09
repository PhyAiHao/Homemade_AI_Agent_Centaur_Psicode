//! Pane primitive — bordered content area with title.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Widget};
use crate::tui::theme::Theme;

pub struct PaneWidget<'a> {
    title: &'a str,
    theme: &'a Theme,
    focused: bool,
}

impl<'a> PaneWidget<'a> {
    pub fn new(title: &'a str, theme: &'a Theme) -> Self {
        PaneWidget { title, theme, focused: false }
    }
    pub fn focused(mut self, focused: bool) -> Self { self.focused = focused; self }

    pub fn inner_area(&self, area: Rect) -> Rect {
        let block = Block::default().borders(Borders::ALL);
        block.inner(area)
    }
}

impl<'a> Widget for PaneWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let border_style = if self.focused { self.theme.accent_style() } else { self.theme.border_style() };
        Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(border_style)
            .render(area, buf);
    }
}
