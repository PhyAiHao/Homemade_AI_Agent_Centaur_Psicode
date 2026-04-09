//! Tasks panel component — shows background tasks and todo items.
//!
//! Supports a list view and a detail view that shows stdout/stderr
//! when a task is selected.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap};
use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct TaskDisplay {
    pub id: String,
    pub subject: String,
    pub status: String,
    pub active_form: Option<String>,
    /// Standard output captured from the task (shown in detail view).
    pub stdout: Option<String>,
    /// Standard error captured from the task (shown in detail view).
    pub stderr: Option<String>,
    /// Exit code, if completed.
    pub exit_code: Option<i32>,
}

/// Detail view mode for the tasks panel.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum TaskPanelMode {
    /// List of all tasks.
    #[default]
    List,
    /// Detail view for a selected task.
    Detail { task_id: String },
}


pub struct TasksPanelWidget<'a> {
    tasks: &'a [TaskDisplay],
    title: &'a str,
    theme: &'a Theme,
    mode: &'a TaskPanelMode,
    selected_index: usize,
}

impl<'a> TasksPanelWidget<'a> {
    pub fn new(tasks: &'a [TaskDisplay], theme: &'a Theme) -> Self {
        TasksPanelWidget {
            tasks,
            title: "Tasks",
            theme,
            mode: &TaskPanelMode::List,
            selected_index: 0,
        }
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = title;
        self
    }

    pub fn mode(mut self, mode: &'a TaskPanelMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }
}

impl<'a> Widget for TasksPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.mode {
            TaskPanelMode::List => self.render_list(area, buf),
            TaskPanelMode::Detail { ref task_id } => {
                self.render_detail(area, buf, task_id)
            }
        }
    }
}

impl<'a> TasksPanelWidget<'a> {
    fn render_list(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self
            .tasks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let (icon, style) = match t.status.as_str() {
                    "completed" => ("\u{2713}", Style::default().fg(Color::Green)),
                    "in_progress" => ("\u{25cf}", self.theme.tool_active_style()),
                    "pending" => ("\u{25cb}", self.theme.dim_style()),
                    "failed" => ("\u{00d7}", self.theme.error_style()),
                    _ => ("?", self.theme.dim_style()),
                };
                let display = t.active_form.as_deref().unwrap_or(&t.subject);
                let base_style = if i == self.selected_index {
                    style.reversed()
                } else {
                    style
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {icon} "), base_style),
                    Span::styled(display.to_string(), base_style),
                ]))
            })
            .collect();

        let block = Block::default()
            .title(format!(" {} ({}) ", self.title, self.tasks.len()))
            .borders(Borders::ALL)
            .border_style(self.theme.border_style());

        Widget::render(List::new(items).block(block), area, buf);
    }

    fn render_detail(&self, area: Rect, buf: &mut Buffer, task_id: &str) {
        let task = self.tasks.iter().find(|t| t.id == task_id);

        let block = Block::default()
            .title(format!(" Task: {} ", task_id))
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style());

        let inner = block.inner(area);
        block.render(area, buf);

        let Some(task) = task else {
            let msg =
                Paragraph::new("Task not found.").style(self.theme.dim_style());
            msg.render(inner, buf);
            return;
        };

        let mut lines: Vec<Line> = Vec::new();

        // Header
        let (icon, status_style) = match task.status.as_str() {
            "completed" => ("\u{2713}", Style::default().fg(Color::Green)),
            "in_progress" => ("\u{25cf}", self.theme.tool_active_style()),
            "failed" => ("\u{00d7}", self.theme.error_style()),
            _ => ("\u{25cb}", self.theme.dim_style()),
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{icon} "), status_style),
            Span::styled(&task.subject, self.theme.assistant_style().bold()),
            Span::styled(
                format!("  [{}]", task.status),
                status_style,
            ),
        ]));

        if let Some(code) = task.exit_code {
            lines.push(Line::from(Span::styled(
                format!("Exit code: {code}"),
                if code == 0 {
                    Style::default().fg(Color::Green)
                } else {
                    self.theme.error_style()
                },
            )));
        }

        lines.push(Line::from(""));

        // stdout
        if let Some(ref stdout) = task.stdout {
            lines.push(Line::from(Span::styled(
                "\u{2500}\u{2500} stdout \u{2500}\u{2500}",
                self.theme.border_style(),
            )));
            for line in stdout.lines().take(50) {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    self.theme.tool_output_style(),
                )));
            }
            if stdout.lines().count() > 50 {
                lines.push(Line::from(Span::styled(
                    format!("... ({} more lines)", stdout.lines().count() - 50),
                    self.theme.dim_style(),
                )));
            }
            lines.push(Line::from(""));
        }

        // stderr
        if let Some(ref stderr) = task.stderr {
            if !stderr.is_empty() {
                lines.push(Line::from(Span::styled(
                    "\u{2500}\u{2500} stderr \u{2500}\u{2500}",
                    self.theme.error_style(),
                )));
                for line in stderr.lines().take(30) {
                    lines.push(Line::from(Span::styled(
                        line.to_string(),
                        self.theme.error_style(),
                    )));
                }
                lines.push(Line::from(""));
            }
        }

        // Footer hint
        lines.push(Line::from(Span::styled(
            "Press Esc to return to list",
            self.theme.dim_style(),
        )));

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }
}
