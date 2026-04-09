//! Dialog primitive — centered modal container with title and border.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Widget};
use crate::tui::theme::Theme;

pub struct DialogWidget<'a> {
    title: &'a str,
    width: u16,
    height: u16,
    theme: &'a Theme,
}

impl<'a> DialogWidget<'a> {
    pub fn new(title: &'a str, width: u16, height: u16, theme: &'a Theme) -> Self {
        DialogWidget { title, width, height, theme }
    }

    /// Calculate the centered area for this dialog.
    pub fn centered_area(&self, container: Rect) -> Rect {
        let w = self.width.min(container.width);
        let h = self.height.min(container.height);
        let x = (container.width.saturating_sub(w)) / 2 + container.x;
        let y = (container.height.saturating_sub(h)) / 2 + container.y;
        Rect::new(x, y, w, h)
    }
}

impl<'a> Widget for DialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let dialog_area = self.centered_area(area);
        Clear.render(dialog_area, buf);
        Block::default()
            .title(format!(" {} ", self.title))
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style())
            .render(dialog_area, buf);
    }
}
