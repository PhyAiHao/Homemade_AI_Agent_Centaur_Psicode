//! Notices component — displays status notices, warnings, and toast notifications.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget};
use std::collections::VecDeque;
use std::time::Instant;

use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct Notice {
    pub text: String,
    pub level: NoticeLevel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NoticeLevel {
    Info,
    Warning,
    Error,
}

pub struct NoticesWidget<'a> {
    notices: &'a [Notice],
    theme: &'a Theme,
}

impl<'a> NoticesWidget<'a> {
    pub fn new(notices: &'a [Notice], theme: &'a Theme) -> Self {
        NoticesWidget { notices, theme }
    }
}

impl<'a> Widget for NoticesWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        for notice in self.notices.iter().take(area.height as usize) {
            let (prefix, style) = match notice.level {
                NoticeLevel::Info => ("i", self.theme.dim_style()),
                NoticeLevel::Warning => ("!", Style::default().fg(Color::Yellow)),
                NoticeLevel::Error => ("x", self.theme.error_style()),
            };
            lines.push(Line::from(vec![
                Span::styled(format!("[{prefix}] "), style),
                Span::styled(&notice.text, style),
            ]));
        }
        Paragraph::new(lines).render(area, buf);
    }
}

// ─── R6: Notification Toast System ─────────────────────────────────────────

/// A single notification toast.
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NoticeLevel,
    pub created_at: Instant,
    pub duration_ms: u64,
}

/// Manages active toast notifications displayed in the top-right corner.
pub struct NotificationManager {
    /// Active notifications (oldest first).
    pub notifications: VecDeque<Notification>,
    /// Maximum number of visible toasts.
    pub max_visible: usize,
}

impl NotificationManager {
    pub fn new() -> Self {
        NotificationManager {
            notifications: VecDeque::new(),
            max_visible: 5,
        }
    }

    /// Push a new notification.
    pub fn push(&mut self, message: impl Into<String>, level: NoticeLevel) {
        self.notifications.push_back(Notification {
            message: message.into(),
            level,
            created_at: Instant::now(),
            duration_ms: 5000,
        });
        // Cap the queue
        while self.notifications.len() > 20 {
            self.notifications.pop_front();
        }
    }

    /// Push a notification with a custom duration.
    pub fn push_with_duration(&mut self, message: impl Into<String>, level: NoticeLevel, duration_ms: u64) {
        self.notifications.push_back(Notification {
            message: message.into(),
            level,
            created_at: Instant::now(),
            duration_ms,
        });
        while self.notifications.len() > 20 {
            self.notifications.pop_front();
        }
    }

    /// Remove expired notifications. Called on each tick.
    pub fn tick(&mut self) {
        self.notifications.retain(|n| {
            n.created_at.elapsed().as_millis() < n.duration_ms as u128
        });
    }

    /// Whether there are any active notifications.
    pub fn has_notifications(&self) -> bool {
        !self.notifications.is_empty()
    }

    /// Get the active notifications (most recent last, capped at max_visible).
    pub fn active(&self) -> Vec<&Notification> {
        self.notifications.iter()
            .rev()
            .take(self.max_visible)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Widget that renders active notifications as stacked toasts in the top-right corner.
pub struct NotificationToastWidget<'a> {
    manager: &'a NotificationManager,
    theme: &'a Theme,
}

impl<'a> NotificationToastWidget<'a> {
    pub fn new(manager: &'a NotificationManager, theme: &'a Theme) -> Self {
        NotificationToastWidget { manager, theme }
    }
}

impl<'a> Widget for NotificationToastWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let active = self.manager.active();
        if active.is_empty() {
            return;
        }

        let toast_width = 40u16.min(area.width.saturating_sub(2));
        let toast_x = area.x + area.width.saturating_sub(toast_width + 1);

        for (i, notification) in active.iter().enumerate() {
            let toast_y = area.y + (i as u16) * 3;
            if toast_y + 3 > area.y + area.height {
                break;
            }

            let toast_area = Rect::new(toast_x, toast_y, toast_width, 3);
            Clear.render(toast_area, buf);

            let (border_color, prefix) = match notification.level {
                NoticeLevel::Info => (Color::Blue, "i"),
                NoticeLevel::Warning => (Color::Yellow, "!"),
                NoticeLevel::Error => (Color::Red, "x"),
            };

            let block = Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(format!(" [{prefix}] "));

            let inner = block.inner(toast_area);
            block.render(toast_area, buf);

            let msg = &notification.message;
            let truncated = if msg.len() > (inner.width as usize).saturating_sub(1) {
                format!("{}...", &msg[..inner.width as usize - 4])
            } else {
                msg.clone()
            };

            let style = match notification.level {
                NoticeLevel::Info => self.theme.assistant_style(),
                NoticeLevel::Warning => Style::default().fg(Color::Yellow),
                NoticeLevel::Error => self.theme.error_style(),
            };

            Paragraph::new(Span::styled(truncated, style))
                .render(inner, buf);
        }
    }
}
