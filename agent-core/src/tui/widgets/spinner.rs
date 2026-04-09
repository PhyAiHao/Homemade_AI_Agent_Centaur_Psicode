//! Spinner widget — animated loading indicator.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;

use crate::tui::theme::Theme;

const BRAILLE_SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
const DOTS_SPINNER: &[&str] = &["⠄", "⠆", "⠇", "⠋", "⠙", "⠸", "⠰", "⠠"];

pub struct SpinnerWidget<'a> {
    label: &'a str,
    elapsed_secs: u64,
    theme: &'a Theme,
}

impl<'a> SpinnerWidget<'a> {
    pub fn new(label: &'a str, elapsed_secs: u64, theme: &'a Theme) -> Self {
        SpinnerWidget { label, elapsed_secs, theme }
    }
}

impl<'a> Widget for SpinnerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let frame = BRAILLE_SPINNER[(self.elapsed_secs as usize) % BRAILLE_SPINNER.len()];
        let line = Line::from(vec![
            Span::styled(
                format!("{frame} "),
                self.theme.tool_active_style(),
            ),
            Span::styled(
                self.label,
                self.theme.tool_active_style(),
            ),
        ]);

        buf.set_line(area.x, area.y, &line, area.width);
    }
}
