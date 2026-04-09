//! Theme — color and style definitions for the TUI.
//!
//! Provides dark, light, and system-adaptive themes.
#![allow(dead_code)]

use ratatui::prelude::*;

/// Active theme configuration.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: String,
    pub colors: ThemeColors,
}

#[derive(Debug, Clone)]
pub struct ThemeColors {
    pub bg: Color,
    pub fg: Color,
    pub user_fg: Color,
    pub assistant_fg: Color,
    pub system_fg: Color,
    pub tool_name_fg: Color,
    pub tool_output_fg: Color,
    pub tool_active_fg: Color,
    pub error_fg: Color,
    pub status_fg: Color,
    pub status_bg: Color,
    pub input_fg: Color,
    pub border_fg: Color,
    pub cursor_fg: Color,
    pub dim_fg: Color,
    pub accent: Color,
}

impl Theme {
    /// Create a theme by name.
    pub fn from_name(name: &str) -> Self {
        match name {
            "light" => Self::light(),
            "system" => {
                // Detect terminal background — fallback to dark
                Self::dark()
            }
            _ => Self::dark(),
        }
    }

    pub fn dark() -> Self {
        Theme {
            name: "dark".to_string(),
            colors: ThemeColors {
                bg: Color::Reset,
                fg: Color::White,
                user_fg: Color::Cyan,
                assistant_fg: Color::White,
                system_fg: Color::DarkGray,
                tool_name_fg: Color::Yellow,
                tool_output_fg: Color::DarkGray,
                tool_active_fg: Color::Blue,
                error_fg: Color::Red,
                status_fg: Color::White,
                status_bg: Color::DarkGray,
                input_fg: Color::White,
                border_fg: Color::DarkGray,
                cursor_fg: Color::Green,
                dim_fg: Color::DarkGray,
                accent: Color::Magenta,
            },
        }
    }

    pub fn light() -> Self {
        Theme {
            name: "light".to_string(),
            colors: ThemeColors {
                bg: Color::Reset,
                fg: Color::Black,
                user_fg: Color::Blue,
                assistant_fg: Color::Black,
                system_fg: Color::Gray,
                tool_name_fg: Color::Rgb(128, 100, 0),
                tool_output_fg: Color::Gray,
                tool_active_fg: Color::Blue,
                error_fg: Color::Red,
                status_fg: Color::Black,
                status_bg: Color::Rgb(230, 230, 230),
                input_fg: Color::Black,
                border_fg: Color::Gray,
                cursor_fg: Color::Green,
                dim_fg: Color::Gray,
                accent: Color::Magenta,
            },
        }
    }

    // ─── Style accessors ────────────────────────────────────────────────

    pub fn user_style(&self) -> Style {
        Style::default().fg(self.colors.user_fg).bold()
    }

    pub fn assistant_style(&self) -> Style {
        Style::default().fg(self.colors.assistant_fg)
    }

    pub fn system_style(&self) -> Style {
        Style::default().fg(self.colors.system_fg).italic()
    }

    pub fn tool_name_style(&self) -> Style {
        Style::default().fg(self.colors.tool_name_fg).bold()
    }

    pub fn tool_output_style(&self) -> Style {
        Style::default().fg(self.colors.tool_output_fg)
    }

    pub fn tool_active_style(&self) -> Style {
        Style::default().fg(self.colors.tool_active_fg)
    }

    pub fn error_style(&self) -> Style {
        Style::default().fg(self.colors.error_fg).bold()
    }

    pub fn status_style(&self) -> Style {
        Style::default().fg(self.colors.status_fg)
    }

    pub fn status_bg_style(&self) -> Style {
        Style::default()
            .fg(self.colors.status_fg)
            .bg(self.colors.status_bg)
    }

    pub fn input_style(&self) -> Style {
        Style::default().fg(self.colors.input_fg)
    }

    pub fn border_style(&self) -> Style {
        Style::default().fg(self.colors.border_fg)
    }

    pub fn cursor_style(&self) -> Style {
        Style::default().fg(self.colors.cursor_fg)
    }

    pub fn dim_style(&self) -> Style {
        Style::default().fg(self.colors.dim_fg)
    }

    pub fn accent_style(&self) -> Style {
        Style::default().fg(self.colors.accent).bold()
    }
}
