//! Resume screen — select and resume a previous conversation.
//!
//! Mirrors `src/screens/ResumeConversation.tsx`.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::tui::theme::Theme;

/// A resumable session entry.
#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub session_id: String,
    pub project: String,
    pub timestamp: u64,
    pub message_count: usize,
    pub last_prompt: String,
}

impl SessionEntry {
    /// R8: Format timestamp as a human-readable relative time string.
    pub fn relative_time(&self) -> String {
        if self.timestamp == 0 {
            return "unknown".to_string();
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let elapsed_secs = now_ms.saturating_sub(self.timestamp) / 1000;

        if elapsed_secs < 60 {
            "just now".to_string()
        } else if elapsed_secs < 3600 {
            format!("{}m ago", elapsed_secs / 60)
        } else if elapsed_secs < 86400 {
            format!("{}h ago", elapsed_secs / 3600)
        } else if elapsed_secs < 172800 {
            "yesterday".to_string()
        } else {
            format!("{}d ago", elapsed_secs / 86400)
        }
    }
}

/// The Resume screen state.
pub struct ResumeScreen {
    pub sessions: Vec<SessionEntry>,
    pub selected: usize,
    pub filter: String,
    pub loading: bool,
}

impl ResumeScreen {
    pub fn new() -> Self {
        ResumeScreen {
            sessions: Vec::new(),
            selected: 0,
            filter: String::new(),
            loading: true,
        }
    }

    /// Load available sessions.
    pub async fn load_sessions(&mut self) {
        self.loading = true;
        match crate::session_history::SessionTranscript::list_sessions().await {
            Ok(ids) => {
                self.sessions = ids.into_iter().map(|id| {
                    SessionEntry {
                        session_id: id.clone(),
                        project: String::new(),
                        timestamp: 0,
                        message_count: 0,
                        last_prompt: format!("Session {}", &id[..8]),
                    }
                }).collect();
            }
            Err(_) => {
                self.sessions.clear();
            }
        }
        self.loading = false;
    }

    /// Get the selected session ID.
    pub fn selected_session(&self) -> Option<&str> {
        let filtered = self.filtered_sessions();
        filtered.get(self.selected).map(|s| s.session_id.as_str())
    }

    fn filtered_sessions(&self) -> Vec<&SessionEntry> {
        if self.filter.is_empty() {
            self.sessions.iter().collect()
        } else {
            let f = self.filter.to_lowercase();
            self.sessions.iter()
                .filter(|s| s.last_prompt.to_lowercase().contains(&f)
                    || s.session_id.contains(&f)
                    || s.project.to_lowercase().contains(&f))
                .collect()
        }
    }

    /// Render the resume screen.
    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Min(5),
                Constraint::Length(1),
            ])
            .split(area);

        // Title
        let title = Paragraph::new(Span::styled("Resume Conversation", theme.accent_style()))
            .alignment(Alignment::Center)
            .block(Block::default().borders(Borders::BOTTOM).border_style(theme.border_style()));
        frame.render_widget(title, chunks[0]);

        // Filter input
        let filter_display = if self.filter.is_empty() { "Type to filter..." } else { &self.filter };
        let filter = Paragraph::new(Span::styled(filter_display.to_string(),
            if self.filter.is_empty() { theme.dim_style() } else { theme.input_style() }))
            .block(Block::default().title(" Filter ").borders(Borders::ALL).border_style(theme.border_style()));
        frame.render_widget(filter, chunks[1]);

        // Session list
        if self.loading {
            let loading = Paragraph::new(Span::styled("Loading sessions...", theme.dim_style()))
                .alignment(Alignment::Center);
            frame.render_widget(loading, chunks[2]);
        } else {
            let filtered = self.filtered_sessions();
            let items: Vec<ListItem> = filtered.iter().enumerate().map(|(i, s)| {
                let style = if i == self.selected { Style::default().reversed() } else { theme.assistant_style() };
                let dim = if i == self.selected { theme.dim_style().reversed() } else { theme.dim_style() };
                let id_short = &s.session_id[..8.min(s.session_id.len())];

                // R8: Show timestamp, project path, and message count
                let time_str = s.relative_time();
                let project_str = if s.project.is_empty() { "" } else { &s.project };
                let msg_count_str = if s.message_count > 0 {
                    format!(" ({} msgs)", s.message_count)
                } else {
                    String::new()
                };

                let mut spans = vec![
                    Span::styled(format!("{id_short}  "), dim),
                    Span::styled(&s.last_prompt, style),
                    Span::styled(msg_count_str, dim),
                ];
                if !project_str.is_empty() {
                    spans.push(Span::styled(format!("  {}", project_str), dim));
                }
                spans.push(Span::styled(format!("  {}", time_str), dim));

                ListItem::new(Line::from(spans))
            }).collect();

            let list = List::new(items)
                .block(Block::default().borders(Borders::NONE));
            frame.render_widget(list, chunks[2]);
        }

        // Footer
        let footer = Paragraph::new(Span::styled(
            "Enter to resume | Esc to cancel | Type to filter",
            theme.dim_style(),
        )).alignment(Alignment::Center);
        frame.render_widget(footer, chunks[3]);
    }
}

impl Default for ResumeScreen {
    fn default() -> Self { Self::new() }
}
