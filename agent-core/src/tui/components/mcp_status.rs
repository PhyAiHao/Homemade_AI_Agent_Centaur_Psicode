//! MCP status component — shows connected MCP servers with connection
//! details and tool counts per server.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Widget, Wrap};
use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct McpServerDisplay {
    pub name: String,
    pub status: McpStatus,
    pub tool_count: usize,
    /// List of tool names registered by this server.
    pub tool_names: Vec<String>,
    /// Connection URI or transport description.
    pub transport: Option<String>,
    /// Uptime in seconds, if connected.
    pub uptime_secs: Option<u64>,
    /// Protocol version string.
    pub protocol_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum McpStatus {
    Connected,
    Connecting,
    Error(String),
    Disabled,
}

/// View mode for the MCP panel.
#[derive(Debug, Clone, PartialEq)]
#[derive(Default)]
pub enum McpPanelMode {
    #[default]
    List,
    Detail { server_name: String },
}


pub struct McpStatusWidget<'a> {
    servers: &'a [McpServerDisplay],
    theme: &'a Theme,
    mode: &'a McpPanelMode,
    selected_index: usize,
}

impl<'a> McpStatusWidget<'a> {
    pub fn new(servers: &'a [McpServerDisplay], theme: &'a Theme) -> Self {
        McpStatusWidget {
            servers,
            theme,
            mode: &McpPanelMode::List,
            selected_index: 0,
        }
    }

    pub fn mode(mut self, mode: &'a McpPanelMode) -> Self {
        self.mode = mode;
        self
    }

    pub fn selected(mut self, index: usize) -> Self {
        self.selected_index = index;
        self
    }
}

impl<'a> Widget for McpStatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.mode {
            McpPanelMode::List => self.render_list(area, buf),
            McpPanelMode::Detail { ref server_name } => {
                self.render_detail(area, buf, server_name)
            }
        }
    }
}

impl<'a> McpStatusWidget<'a> {
    fn render_list(self, area: Rect, buf: &mut Buffer) {
        // Summary line: total connected / total servers
        let connected = self
            .servers
            .iter()
            .filter(|s| s.status == McpStatus::Connected)
            .count();
        let total_tools: usize = self.servers.iter().map(|s| s.tool_count).sum();

        let items: Vec<ListItem> = self
            .servers
            .iter()
            .enumerate()
            .map(|(i, s)| {
                let (icon, style) = match &s.status {
                    McpStatus::Connected => {
                        ("\u{25cf}", Style::default().fg(Color::Green))
                    }
                    McpStatus::Connecting => {
                        ("\u{25cc}", self.theme.tool_active_style())
                    }
                    McpStatus::Error(_) => {
                        ("\u{00d7}", self.theme.error_style())
                    }
                    McpStatus::Disabled => {
                        ("\u{25cb}", self.theme.dim_style())
                    }
                };
                let base_style = if i == self.selected_index {
                    style.reversed()
                } else {
                    style
                };

                let mut spans = vec![
                    Span::styled(format!(" {icon} "), base_style),
                    Span::styled(s.name.clone(), base_style),
                ];

                // Tool count badge
                spans.push(Span::styled(
                    format!(" ({} tools)", s.tool_count),
                    if i == self.selected_index {
                        self.theme.dim_style().reversed()
                    } else {
                        self.theme.dim_style()
                    },
                ));

                // Show error message inline if present
                if let McpStatus::Error(ref msg) = s.status {
                    spans.push(Span::styled(
                        format!(" - {}", msg),
                        self.theme.error_style(),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let title = format!(
            " MCP Servers ({}/{} connected, {} tools) ",
            connected,
            self.servers.len(),
            total_tools
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

    fn render_detail(&self, area: Rect, buf: &mut Buffer, server_name: &str) {
        let server = self.servers.iter().find(|s| s.name == server_name);

        let block = Block::default()
            .title(format!(" MCP: {} ", server_name))
            .borders(Borders::ALL)
            .border_style(self.theme.accent_style());

        let inner = block.inner(area);
        block.render(area, buf);

        let Some(server) = server else {
            let msg = Paragraph::new("Server not found.")
                .style(self.theme.dim_style());
            msg.render(inner, buf);
            return;
        };

        let mut lines: Vec<Line> = Vec::new();

        // Status
        let (icon, status_text, status_style) = match &server.status {
            McpStatus::Connected => {
                ("\u{25cf}", "Connected", Style::default().fg(Color::Green))
            }
            McpStatus::Connecting => {
                ("\u{25cc}", "Connecting...", self.theme.tool_active_style())
            }
            McpStatus::Error(msg) => {
                lines.push(Line::from(vec![
                    Span::styled(
                        "\u{00d7} Error: ",
                        self.theme.error_style(),
                    ),
                    Span::styled(msg.clone(), self.theme.error_style()),
                ]));
                ("\u{00d7}", "Error", self.theme.error_style())
            }
            McpStatus::Disabled => {
                ("\u{25cb}", "Disabled", self.theme.dim_style())
            }
        };

        lines.insert(
            0,
            Line::from(vec![
                Span::styled(
                    format!("{icon} Status: "),
                    self.theme.assistant_style(),
                ),
                Span::styled(status_text, status_style),
            ]),
        );

        // Transport
        if let Some(ref transport) = server.transport {
            lines.push(Line::from(vec![
                Span::styled("Transport: ", self.theme.assistant_style()),
                Span::styled(transport.clone(), self.theme.dim_style()),
            ]));
        }

        // Protocol version
        if let Some(ref version) = server.protocol_version {
            lines.push(Line::from(vec![
                Span::styled("Protocol: ", self.theme.assistant_style()),
                Span::styled(version.clone(), self.theme.dim_style()),
            ]));
        }

        // Uptime
        if let Some(secs) = server.uptime_secs {
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

        lines.push(Line::from(""));

        // Tool list
        lines.push(Line::from(Span::styled(
            format!(
                "\u{2500}\u{2500} Tools ({}) \u{2500}\u{2500}",
                server.tool_count
            ),
            self.theme.border_style(),
        )));
        for name in &server.tool_names {
            lines.push(Line::from(vec![
                Span::styled("  \u{2022} ", self.theme.accent_style()),
                Span::styled(name.clone(), self.theme.assistant_style()),
            ]));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "Press Esc to return to list",
            self.theme.dim_style(),
        )));

        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        paragraph.render(inner, buf);
    }
}
