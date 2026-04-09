//! Loading state — spinner + message (for "Thinking..." etc.)
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;
use crate::tui::theme::Theme;

const BRAILLE_SPINNER: &[char] = &[
    '\u{280b}', '\u{2819}', '\u{2839}', '\u{2838}',
    '\u{283c}', '\u{2834}', '\u{2826}', '\u{2827}',
    '\u{2807}', '\u{280f}',
];

/// A loading indicator: animated spinner + text message.
pub struct LoadingStateWidget<'a> {
    message: &'a str,
    /// Current tick count — used to animate the spinner.
    tick: u64,
    theme: &'a Theme,
}

impl<'a> LoadingStateWidget<'a> {
    pub fn new(message: &'a str, tick: u64, theme: &'a Theme) -> Self {
        LoadingStateWidget {
            message,
            tick,
            theme,
        }
    }
}

impl<'a> Widget for LoadingStateWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }

        let frame_char =
            BRAILLE_SPINNER[(self.tick as usize) % BRAILLE_SPINNER.len()];

        let line = Line::from(vec![
            Span::styled(
                format!("{} ", frame_char),
                self.theme.tool_active_style(),
            ),
            Span::styled(
                self.message.to_string(),
                self.theme.dim_style(),
            ),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
