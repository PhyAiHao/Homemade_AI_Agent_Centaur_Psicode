//! Message component — renders a single conversation message.
//!
//! Supports User, Assistant (with markdown), Tool use/result, and System messages.
//! Also supports message grouping: consecutive tool_use + tool_result messages
//! can be collapsed under a summary line.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::tui::{DisplayMessage, DisplayRole};
use crate::tui::markdown;
use crate::tui::theme::Theme;

// ─── Tool group (collapsible) ──────────────────────────────────────────────

/// Information about a consecutive group of tool messages.
#[derive(Debug, Clone)]
pub struct ToolGroup {
    /// The tool names used in this group.
    pub tool_names: Vec<String>,
    /// Total duration (sum of all tool_result durations).
    pub total_duration_ms: u64,
    /// Number of messages in the group (tool_use + tool_result pairs).
    pub message_count: usize,
    /// Whether this group is collapsed.
    pub collapsed: bool,
}

impl ToolGroup {
    /// Build a summary string like "Ran 3 tools (Bash, FileRead, Grep) — 2.3s"
    pub fn summary(&self) -> String {
        let tool_count = self.tool_names.len();
        let names = if tool_count <= 4 {
            self.tool_names.join(", ")
        } else {
            let first3: Vec<&str> = self.tool_names.iter().take(3).map(|s| s.as_str()).collect();
            format!("{}, +{} more", first3.join(", "), tool_count - 3)
        };
        let dur = self.total_duration_ms as f64 / 1000.0;
        format!(
            "Ran {} tool{} ({}) \u{2014} {:.1}s",
            tool_count,
            if tool_count == 1 { "" } else { "s" },
            names,
            dur,
        )
    }
}

/// Identify consecutive tool groups in a message list.
/// Returns `Vec<(start_index, end_index_exclusive, ToolGroup)>`.
pub fn identify_tool_groups(
    messages: &[DisplayMessage],
    collapsed_set: &std::collections::HashSet<usize>,
) -> Vec<(usize, usize, ToolGroup)> {
    let mut groups = Vec::new();
    let mut i = 0;
    while i < messages.len() {
        if matches!(
            messages[i].role,
            DisplayRole::ToolUse | DisplayRole::ToolResult
        ) {
            let start = i;
            let mut tool_names: Vec<String> = Vec::new();
            let mut total_dur: u64 = 0;
            while i < messages.len()
                && matches!(
                    messages[i].role,
                    DisplayRole::ToolUse | DisplayRole::ToolResult
                )
            {
                if let Some(ref info) = messages[i].tool_info {
                    if !tool_names.contains(&info.tool_name) {
                        tool_names.push(info.tool_name.clone());
                    }
                    if let Some(d) = info.duration_ms {
                        total_dur += d;
                    }
                }
                i += 1;
            }
            let msg_count = i - start;
            if msg_count >= 2 {
                groups.push((
                    start,
                    i,
                    ToolGroup {
                        tool_names,
                        total_duration_ms: total_dur,
                        message_count: msg_count,
                        collapsed: collapsed_set.contains(&start),
                    },
                ));
            }
        } else {
            i += 1;
        }
    }
    groups
}

// ─── Single message widget ─────────────────────────────────────────────────

/// Renders a single conversation message with role-appropriate styling.
pub struct MessageWidget<'a> {
    msg: &'a DisplayMessage,
    theme: &'a Theme,
    width: u16,
    /// If true, render with dimmed style (for collapsed tool output).
    dimmed: bool,
}

impl<'a> MessageWidget<'a> {
    pub fn new(msg: &'a DisplayMessage, theme: &'a Theme, width: u16) -> Self {
        MessageWidget {
            msg,
            theme,
            width,
            dimmed: false,
        }
    }

    pub fn dimmed(mut self, dimmed: bool) -> Self {
        self.dimmed = dimmed;
        self
    }

    fn role_header(&self) -> (String, Style) {
        match self.msg.role {
            DisplayRole::User => ("You".to_string(), self.theme.user_style()),
            DisplayRole::Assistant => ("Assistant".to_string(), self.theme.assistant_style()),
            DisplayRole::System => ("System".to_string(), self.theme.system_style()),
            DisplayRole::ToolUse => {
                let name = self
                    .msg
                    .tool_info
                    .as_ref()
                    .map(|t| t.tool_name.as_str())
                    .unwrap_or("Tool");
                (format!("[{name}]"), self.theme.tool_name_style())
            }
            DisplayRole::ToolResult => {
                let info = self.msg.tool_info.as_ref();
                let name = info.map(|t| t.tool_name.as_str()).unwrap_or("Result");
                let duration = info
                    .and_then(|t| t.duration_ms)
                    .map(|d| format!(" {d}ms"))
                    .unwrap_or_default();
                let is_error = info.map(|t| t.is_error).unwrap_or(false);
                let style = if is_error {
                    self.theme.error_style()
                } else {
                    self.theme.tool_name_style()
                };
                (format!("[{name}{duration}]"), style)
            }
        }
    }
}

impl<'a> Widget for MessageWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let (header, mut header_style) = self.role_header();
        if self.dimmed {
            header_style = self.theme.dim_style();
        }
        let mut lines = vec![Line::from(Span::styled(header, header_style))];

        // Render content
        if self.msg.role == DisplayRole::Assistant && !self.dimmed {
            lines.extend(markdown::render_markdown(&self.msg.content, self.theme));
        } else {
            let content_style = if self.dimmed {
                self.theme.dim_style()
            } else {
                match self.msg.role {
                    DisplayRole::User => self.theme.user_style(),
                    DisplayRole::ToolResult
                        if self
                            .msg
                            .tool_info
                            .as_ref()
                            .map(|t| t.is_error)
                            .unwrap_or(false) =>
                    {
                        self.theme.error_style()
                    }
                    DisplayRole::ToolResult | DisplayRole::ToolUse => {
                        self.theme.tool_output_style()
                    }
                    DisplayRole::System => self.theme.system_style(),
                    _ => self.theme.assistant_style(),
                }
            };
            // Truncate long tool outputs
            let content = if (self.msg.role == DisplayRole::ToolResult
                || self.msg.role == DisplayRole::ToolUse)
                && self.msg.content.len() > 2000
            {
                format!(
                    "{}...\n(truncated {} bytes)",
                    &self.msg.content[..2000],
                    self.msg.content.len()
                )
            } else {
                self.msg.content.clone()
            };
            for line in content.lines() {
                lines.push(Line::from(Span::styled(line.to_string(), content_style)));
            }
        }

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(area, buf);
    }
}

// ─── Tool group summary widget ─────────────────────────────────────────────

/// Renders the collapsed summary line for a tool group.
pub struct ToolGroupSummaryWidget<'a> {
    group: &'a ToolGroup,
    theme: &'a Theme,
}

impl<'a> ToolGroupSummaryWidget<'a> {
    pub fn new(group: &'a ToolGroup, theme: &'a Theme) -> Self {
        ToolGroupSummaryWidget { group, theme }
    }
}

impl<'a> Widget for ToolGroupSummaryWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let toggle = if self.group.collapsed {
            "\u{25b6}" // right-pointing triangle
        } else {
            "\u{25bc}" // down-pointing triangle
        };
        let summary = self.group.summary();
        let line = Line::from(vec![
            Span::styled(
                format!(" {toggle} "),
                self.theme.tool_name_style(),
            ),
            Span::styled(summary, self.theme.dim_style()),
        ]);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
