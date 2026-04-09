//! Settings screen — read-only display of current configuration.
//!
//! Shows model, permission mode, theme, and vim mode status.
//! Editing is done via /commands or the config file.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::tui::theme::Theme;

/// Settings display state. All fields are read from the App struct.
pub struct SettingsView {
    pub model: String,
    pub permission_mode: String,
    pub theme_name: String,
    pub vim_enabled: bool,
    pub plan_mode: bool,
    pub voice_enabled: bool,
}

impl SettingsView {
    /// Render the settings as a centered dialog overlay.
    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();

        let width = 50u16.min(area.width.saturating_sub(4));
        let height = 16u16.min(area.height.saturating_sub(4));
        let x = (area.width.saturating_sub(width)) / 2 + area.x;
        let y = (area.height.saturating_sub(height)) / 2 + area.y;
        let dialog_area = Rect::new(x, y, width, height);

        // Clear the background
        frame.render_widget(Clear, dialog_area);

        let block = Block::default()
            .title(" Settings (read-only) ")
            .borders(Borders::ALL)
            .border_style(theme.accent_style());

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);

        let vim_label = if self.vim_enabled { "enabled" } else { "disabled" };
        let plan_label = if self.plan_mode { "enabled" } else { "disabled" };
        let voice_label = if self.voice_enabled { "enabled" } else { "disabled" };

        let lines = vec![
            Line::from(""),
            Line::from(vec![
                Span::styled("  Model:           ", theme.assistant_style()),
                Span::styled(&self.model, theme.accent_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Permission mode: ", theme.assistant_style()),
                Span::styled(&self.permission_mode, theme.dim_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Theme:           ", theme.assistant_style()),
                Span::styled(&self.theme_name, theme.dim_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Vim mode:        ", theme.assistant_style()),
                Span::styled(vim_label, theme.dim_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Plan mode:       ", theme.assistant_style()),
                Span::styled(plan_label, theme.dim_style()),
            ]),
            Line::from(""),
            Line::from(vec![
                Span::styled("  Voice input:     ", theme.assistant_style()),
                Span::styled(voice_label, theme.dim_style()),
            ]),
            Line::from(""),
            Line::from(Span::styled(
                "  Press Esc or 'q' to close",
                theme.dim_style(),
            )),
        ];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, inner);
    }
}
