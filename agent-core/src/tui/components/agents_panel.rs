//! Agents panel — shows team members and their status, with a detail
//! view for individual agent configuration, status, and progress.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, Widget, Wrap};
use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct AgentDisplay {
    pub name: String,
    pub role: String,
    pub status: String,
    pub color: Option<Color>,
    /// Agent's model / configuration.
    pub model: Option<String>,
    /// Current task description.
    pub current_task: Option<String>,
    /// Progress 0.0..1.0 (if deterministic).
    pub progress: Option<f64>,
    /// Total tokens consumed by this agent.
    pub tokens_used: Option<u64>,
    /// Uptime in seconds.
    pub uptime_secs: Option<u64>,
    /// Allowed tools.
    pub allowed_tools: Vec<String>,
}

/// View mode for the agents panel.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum AgentPanelMode {
    #[default]
    List,
    Detail { agent_name: String },
}


pub struct AgentsPanelWidget<'a> {
    agents: &'a [AgentDisplay],
    theme: &'a Theme,
    mode: &'a AgentPanelMode,
    selected_index: usize,
}

impl<'a> AgentsPanelWidget<'a> {
    pub fn new(agents: &'a [AgentDisplay], theme: &'a Theme) -> Self {
        AgentsPanelWidget {
            agents,
            theme,
            mode: &AgentPanelMode::List,
            selected_index: 0,
        }
    }

    pub fn mode(mut self, mode: &'a AgentPanelMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }
}

impl<'a> Widget for AgentsPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.mode {
            AgentPanelMode::List => self.render_list(area, buf),
            AgentPanelMode::Detail { ref agent_name } => {
                self.render_detail(area, buf, agent_name)
            }
        }
    }
}

impl<'a> AgentsPanelWidget<'a> {
    fn render_list(self, area: Rect, buf: &mut Buffer) {
        let active_count = self
            .agents
            .iter()
            .filter(|a| a.status == "active")
            .count();

        let items: Vec<ListItem> = self
            .agents
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let color = a.color.unwrap_or(self.theme.colors.fg);
                let status_icon = match a.status.as_str() {
                    "active" => "\u{25cf}",
                    "idle" => "\u{25cb}",
                    "stopped" => "\u{00d7}",
                    "error" => "!",
                    _ => "?",
                };
                let base_style = if i == self.selected_index {
                    Style::default().fg(color).reversed()
                } else {
                    Style::default().fg(color)
                };
                let dim = if i == self.selected_index {
                    self.theme.dim_style().reversed()
                } else {
                    self.theme.dim_style()
                };

                let mut spans = vec![
                    Span::styled(
                        format!("{status_icon} "),
                        base_style,
                    ),
                    Span::styled(a.name.clone(), base_style.bold()),
                    Span::styled(format!(" ({}) ", a.role), dim),
                ];

                // Show current task if available
                if let Some(ref task) = a.current_task {
                    let truncated = if task.len() > 30 {
                        format!("{}...", &task[..27])
                    } else {
                        task.clone()
                    };
                    spans.push(Span::styled(truncated, dim));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let title = format!(
            " Team ({} active / {}) ",
            active_count,
            self.agents.len()
        );
        Widget::render(
            List::new(items).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(self.theme.border_style()),
            ),
            area,
            buf,
        );
    }

    fn render_detail(&self, area: Rect, buf: &mut Buffer, agent_name: &str) {
        let agent = self.agents.iter().find(|a| a.name == agent_name);

        let block = Block::default()
            .title(format!(" Agent: {} ", agent_name))
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style());

        let inner = block.inner(area);
        block.render(area, buf);

        let Some(agent) = agent else {
            let msg = Paragraph::new("Agent not found.")
                .style(self.theme.dim_style());
            msg.render(inner, buf);
            return;
        };

        // Split inner area: info at top, progress bar (if any), tools at bottom
        let mut lines: Vec<Line> = Vec::new();

        // Status line
        let color = agent.color.unwrap_or(self.theme.colors.fg);
        let status_icon = match agent.status.as_str() {
            "active" => "\u{25cf}",
            "idle" => "\u{25cb}",
            "stopped" => "\u{00d7}",
            _ => "?",
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{status_icon} "),
                Style::default().fg(color),
            ),
            Span::styled(
                agent.name.clone(),
                Style::default().fg(color).bold(),
            ),
            Span::styled(
                format!("  [{}]", agent.status),
                self.theme.dim_style(),
            ),
        ]));

        // Role
        lines.push(Line::from(vec![
            Span::styled("Role: ", self.theme.assistant_style()),
            Span::styled(agent.role.clone(), self.theme.dim_style()),
        ]));

        // Model
        if let Some(ref model) = agent.model {
            lines.push(Line::from(vec![
                Span::styled("Model: ", self.theme.assistant_style()),
                Span::styled(model.clone(), self.theme.dim_style()),
            ]));
        }

        // Tokens
        if let Some(tokens) = agent.tokens_used {
            lines.push(Line::from(vec![
                Span::styled("Tokens: ", self.theme.assistant_style()),
                Span::styled(
                    format!("{}", tokens),
                    self.theme.dim_style(),
                ),
            ]));
        }

        // Uptime
        if let Some(secs) = agent.uptime_secs {
            let display = if secs >= 3600 {
                format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
            } else if secs >= 60 {
                format!("{}m {}s", secs / 60, secs % 60)
            } else {
                format!("{secs}s")
            };
            lines.push(Line::from(vec![
                Span::styled("Uptime: ", self.theme.assistant_style()),
                Span::styled(display, self.theme.dim_style()),
            ]));
        }

        // Current task
        if let Some(ref task) = agent.current_task {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Current task:",
                self.theme.assistant_style().bold(),
            )));
            lines.push(Line::from(Span::styled(
                task.clone(),
                self.theme.tool_output_style(),
            )));
        }

        lines.push(Line::from(""));

        // Render the text portion
        let has_progress = agent.progress.is_some();
        let text_height = if has_progress {
            inner.height.saturating_sub(4) // reserve space for progress + tools header
        } else {
            inner.height.saturating_sub(2)
        };

        let text_area = Rect::new(inner.x, inner.y, inner.width, text_height);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(text_area, buf);

        // Progress bar
        let mut y = inner.y + text_height;
        if let Some(progress) = agent.progress {
            if y < inner.y + inner.height {
                let gauge_area = Rect::new(inner.x, y, inner.width, 1);
                let pct = (progress * 100.0).clamp(0.0, 100.0);
                let gauge_color = if pct > 90.0 {
                    Color::Green
                } else if pct > 50.0 {
                    Color::Yellow
                } else {
                    Color::Blue
                };
                Gauge::default()
                    .ratio(progress.clamp(0.0, 1.0))
                    .label(format!("{:.0}%", pct))
                    .gauge_style(Style::default().fg(gauge_color))
                    .render(gauge_area, buf);
                y += 1;
            }
        }

        // Allowed tools summary
        if !agent.allowed_tools.is_empty() && y < inner.y + inner.height {
            let tools_line = Line::from(vec![
                Span::styled("Tools: ", self.theme.assistant_style()),
                Span::styled(
                    agent.allowed_tools.join(", "),
                    self.theme.dim_style(),
                ),
            ]);
            buf.set_line(inner.x, y, &tools_line, inner.width);
            // y += 1; // not needed since it's the last line
        }

        // Footer
        let footer_y = inner.y + inner.height.saturating_sub(1);
        if footer_y > y {
            let footer = Line::from(Span::styled(
                "Press Esc to return to list",
                self.theme.dim_style(),
            ));
            buf.set_line(inner.x, footer_y, &footer, inner.width);
        }
    }
}
