//! Status icon — colored circle/symbol for status indication.
//!
//! green = ok, yellow = warn, red = error, gray = inactive.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;

/// Status level for the icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLevel {
    Ok,
    Warn,
    Error,
    Inactive,
    Active,
}

/// A colored status indicator symbol.
pub struct StatusIconWidget {
    level: StatusLevel,
    /// Optional label shown after the icon.
    label: Option<String>,
}

impl StatusIconWidget {
    pub fn new(level: StatusLevel) -> Self {
        StatusIconWidget { level, label: None }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    fn icon_and_color(&self) -> (&'static str, Color) {
        match self.level {
            StatusLevel::Ok => ("\u{25cf}", Color::Green),       // ●
            StatusLevel::Warn => ("\u{25cf}", Color::Yellow),    // ●
            StatusLevel::Error => ("\u{00d7}", Color::Red),      // ×
            StatusLevel::Inactive => ("\u{25cb}", Color::DarkGray), // ○
            StatusLevel::Active => ("\u{25cf}", Color::Blue),    // ●
        }
    }
}

impl Widget for StatusIconWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if area.width == 0 || area.height == 0 {
            return;
        }
        let (icon, color) = self.icon_and_color();
        let mut spans = vec![Span::styled(icon, Style::default().fg(color))];
        if let Some(ref label) = self.label {
            spans.push(Span::raw(" "));
            spans.push(Span::styled(label.clone(), Style::default().fg(color)));
        }
        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
