//! Progress bar primitive.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Gauge, Widget};
use crate::tui::theme::Theme;

pub struct ProgressBarWidget<'a> {
    progress: f64,
    label: &'a str,
    theme: &'a Theme,
}

impl<'a> ProgressBarWidget<'a> {
    pub fn new(progress: f64, label: &'a str, theme: &'a Theme) -> Self {
        ProgressBarWidget { progress: progress.clamp(0.0, 1.0), label, theme }
    }
}

impl<'a> Widget for ProgressBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let color = if self.progress > 0.9 { Color::Red }
            else if self.progress > 0.7 { Color::Yellow }
            else { Color::Green };

        Gauge::default()
            .ratio(self.progress)
            .label(format!("{} ({:.0}%)", self.label, self.progress * 100.0))
            .gauge_style(Style::default().fg(color))
            .render(area, buf);
    }
}
