//! Divider — horizontal line separator.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;
use crate::tui::theme::Theme;

/// A horizontal divider line, optionally with a centered label.
pub struct DividerWidget<'a> {
    label: Option<&'a str>,
    theme: &'a Theme,
    char: char,
}

impl<'a> DividerWidget<'a> {
    pub fn new(theme: &'a Theme) -> Self {
        DividerWidget {
            label: None,
            theme,
            char: '\u{2500}', // ─
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }

    pub fn line_char(mut self, c: char) -> Self {
        self.char = c;
        self
    }
}

impl<'a> Widget for DividerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let style = self.theme.border_style();

        if let Some(label) = self.label {
            // "─── Label ───"
            let label_len = label.len() as u16;
            let total = area.width;
            let left_len = total.saturating_sub(label_len + 2) / 2;
            let right_len = total.saturating_sub(left_len + label_len + 2);

            let left_line: String =
                std::iter::repeat_n(self.char, left_len as usize).collect();
            let right_line: String =
                std::iter::repeat_n(self.char, right_len as usize).collect();

            let line = Line::from(vec![
                Span::styled(left_line, style),
                Span::styled(format!(" {} ", label), self.theme.dim_style()),
                Span::styled(right_line, style),
            ]);
            buf.set_line(area.x, area.y, &line, area.width);
        } else {
            let full_line: String =
                std::iter::repeat_n(self.char, area.width as usize).collect();
            let line = Line::from(Span::styled(full_line, style));
            buf.set_line(area.x, area.y, &line, area.width);
        }
    }
}
