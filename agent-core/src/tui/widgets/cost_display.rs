//! Cost display widget — shows token usage and cost.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;

use crate::tui::theme::Theme;

pub struct CostDisplayWidget<'a> {
    cost_text: &'a str,
    theme: &'a Theme,
}

impl<'a> CostDisplayWidget<'a> {
    pub fn new(cost_text: &'a str, theme: &'a Theme) -> Self {
        CostDisplayWidget { cost_text, theme }
    }
}

impl<'a> Widget for CostDisplayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.cost_text.is_empty() {
            return;
        }

        let line = Line::from(Span::styled(
            self.cost_text,
            self.theme.dim_style(),
        ));

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
