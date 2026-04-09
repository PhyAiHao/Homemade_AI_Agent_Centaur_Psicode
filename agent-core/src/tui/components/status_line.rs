//! Status line component — footer bar with model name, cost, and indicators.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget};

use crate::tui::theme::Theme;

pub struct StatusLineWidget<'a> {
    left: &'a str,
    center: &'a str,
    right: &'a str,
    theme: &'a Theme,
}

impl<'a> StatusLineWidget<'a> {
    pub fn new(left: &'a str, right: &'a str, theme: &'a Theme) -> Self {
        StatusLineWidget { left, center: "", right, theme }
    }

    pub fn center(mut self, center: &'a str) -> Self {
        self.center = center;
        self
    }
}

impl<'a> Widget for StatusLineWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = area.width as usize;
        let left_len = self.left.len().min(width / 3);
        let right_len = self.right.len().min(width / 3);
        let center_len = self.center.len().min(width / 3);

        let gap1 = (width / 2).saturating_sub(left_len).saturating_sub(center_len / 2);
        let gap2 = width.saturating_sub(left_len).saturating_sub(gap1).saturating_sub(center_len).saturating_sub(right_len);

        let line = Line::from(vec![
            Span::styled(&self.left[..left_len], self.theme.status_style()),
            Span::raw(" ".repeat(gap1)),
            Span::styled(self.center, self.theme.dim_style()),
            Span::raw(" ".repeat(gap2)),
            Span::styled(&self.right[..right_len], self.theme.dim_style()),
        ]);

        let paragraph = Paragraph::new(line)
            .style(self.theme.status_bg_style());
        paragraph.render(area, buf);
    }
}
