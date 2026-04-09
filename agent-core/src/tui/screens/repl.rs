//! REPL screen — the main interactive chat screen.
//!
//! Mirrors `src/screens/REPL.tsx`. This is the primary screen where
//! the user interacts with the agent.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;

use crate::tui::App;
use crate::tui::renderer::draw_frame;
use crate::tui::components;

/// The REPL screen state.
pub struct ReplScreen {
    /// Whether the welcome message has been shown.
    pub welcome_shown: bool,
    /// Whether suggestions are visible.
    pub show_suggestions: bool,
    /// Available suggestions.
    pub suggestions: Vec<String>,
    /// Selected suggestion index.
    pub suggestion_index: usize,
}

impl ReplScreen {
    pub fn new() -> Self {
        ReplScreen {
            welcome_shown: false,
            show_suggestions: false,
            suggestions: vec![
                "Help me understand this codebase".to_string(),
                "Find and fix bugs".to_string(),
                "Write tests for the recent changes".to_string(),
                "Explain the architecture".to_string(),
            ],
            suggestion_index: 0,
        }
    }

    /// Render the REPL screen.
    pub fn render(&self, frame: &mut Frame, app: &App) {
        let area = frame.area();

        if !self.welcome_shown && app.messages.is_empty() && !app.is_streaming {
            // Show welcome screen with suggestions
            self.render_welcome(frame, app, area);
        } else {
            // Normal chat view — delegate to main renderer
            draw_frame(frame, app);
        }
    }

    fn render_welcome(&self, frame: &mut Frame, app: &App, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(30),  // Top padding
                Constraint::Length(3),       // Title
                Constraint::Length(2),       // Subtitle
                Constraint::Min(6),          // Suggestions
                Constraint::Length(1),       // Status
                Constraint::Length(3),       // Input
            ])
            .split(area);

        // Title
        let title = Paragraph::new(Line::from(vec![
            Span::styled("Centaur Psicode", app.theme.accent_style()),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(title, chunks[1]);

        // Subtitle
        let subtitle = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("Model: {} | Session: {}", app.model, &app.session_id[..8]),
                app.theme.dim_style(),
            ),
        ]))
        .alignment(Alignment::Center);
        frame.render_widget(subtitle, chunks[2]);

        // Suggestions
        if self.show_suggestions {
            let suggestions_widget = components::suggestions::SuggestionsWidget::new(
                &self.suggestions,
                self.suggestion_index,
                &app.theme,
            );
            frame.render_widget(suggestions_widget, chunks[3]);
        }

        // Status line
        let status = components::status_line::StatusLineWidget::new(
            &app.status_left,
            &app.status_right,
            &app.theme,
        );
        frame.render_widget(status, chunks[4]);

        // Input
        let input = components::text_input::TextInputWidget::new(
            app.input.display_text(),
            app.input.cursor_position(),
            &app.theme,
        ).placeholder("What would you like to do?");
        frame.render_widget(input, chunks[5]);

        // Cursor
        frame.set_cursor_position(Position::new(
            chunks[5].x + app.input.cursor_position() as u16 + 3,
            chunks[5].y + 1,
        ));
    }
}

impl Default for ReplScreen {
    fn default() -> Self { Self::new() }
}
