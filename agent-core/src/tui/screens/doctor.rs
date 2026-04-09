//! Doctor screen — environment diagnostic checks.
//!
//! Mirrors `src/screens/Doctor.tsx`. Runs checks on API key, IPC socket,
//! git installation, model availability, and system requirements.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

use crate::tui::theme::Theme;

/// A single diagnostic check.
#[derive(Debug, Clone)]
pub struct DiagnosticCheck {
    pub name: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CheckStatus {
    Pass,
    Fail,
    Warn,
    Running,
    Skipped,
}

/// The Doctor screen state.
pub struct DoctorScreen {
    pub checks: Vec<DiagnosticCheck>,
    pub is_running: bool,
}

impl DoctorScreen {
    pub fn new() -> Self {
        DoctorScreen {
            checks: Vec::new(),
            is_running: false,
        }
    }

    /// Initialize and run all diagnostic checks.
    pub async fn run_checks(&mut self) {
        self.is_running = true;
        self.checks.clear();

        // API Key check
        let api_key = std::env::var("ANTHROPIC_API_KEY").unwrap_or_default();
        self.checks.push(DiagnosticCheck {
            name: "API Key".to_string(),
            status: if api_key.is_empty() { CheckStatus::Fail } else { CheckStatus::Pass },
            detail: if api_key.is_empty() {
                "ANTHROPIC_API_KEY not set".to_string()
            } else {
                format!("Set ({}...)", &api_key[..8.min(api_key.len())])
            },
        });

        // Git check
        let git_ok = tokio::process::Command::new("git")
            .arg("--version")
            .output()
            .await
            .map(|o| o.status.success())
            .unwrap_or(false);
        self.checks.push(DiagnosticCheck {
            name: "Git".to_string(),
            status: if git_ok { CheckStatus::Pass } else { CheckStatus::Warn },
            detail: if git_ok { "Available".to_string() } else { "Not found".to_string() },
        });

        // IPC Socket check
        let socket_path = std::env::var("AGENT_IPC_SOCKET")
            .unwrap_or_else(|_| "/tmp/agent-ipc.sock".to_string());
        let socket_exists = std::path::Path::new(&socket_path).exists();
        self.checks.push(DiagnosticCheck {
            name: "IPC Socket".to_string(),
            status: if socket_exists { CheckStatus::Pass } else { CheckStatus::Warn },
            detail: format!("{socket_path} ({})", if socket_exists { "connected" } else { "not found — run `make dev-python`" }),
        });

        // Working directory check
        let cwd = std::env::current_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        self.checks.push(DiagnosticCheck {
            name: "Working Directory".to_string(),
            status: CheckStatus::Pass,
            detail: cwd,
        });

        // Model check
        let model = std::env::var("CLAUDE_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        self.checks.push(DiagnosticCheck {
            name: "Model".to_string(),
            status: CheckStatus::Pass,
            detail: model,
        });

        // Config directory
        let config_dir = crate::config::agent_dir()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|_| "unknown".to_string());
        let config_exists = crate::config::config_path()
            .map(|p| p.exists())
            .unwrap_or(false);
        self.checks.push(DiagnosticCheck {
            name: "Config".to_string(),
            status: if config_exists { CheckStatus::Pass } else { CheckStatus::Warn },
            detail: format!("{config_dir} ({})", if config_exists { "found" } else { "default" }),
        });

        self.is_running = false;
    }

    /// Render the doctor screen.
    pub fn render(&self, frame: &mut Frame, theme: &Theme) {
        let area = frame.area();

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),   // Title
                Constraint::Min(5),      // Checks
                Constraint::Length(2),   // Footer
            ])
            .split(area);

        // Title
        let title = Paragraph::new(Line::from(Span::styled(
            "Agent Doctor — Environment Diagnostics",
            theme.accent_style(),
        )))
        .alignment(Alignment::Center)
        .block(Block::default().borders(Borders::BOTTOM).border_style(theme.border_style()));
        frame.render_widget(title, chunks[0]);

        // Checks
        let items: Vec<ListItem> = self.checks.iter().map(|check| {
            let (icon, style) = match check.status {
                CheckStatus::Pass => ("✓", Style::default().fg(Color::Green)),
                CheckStatus::Fail => ("✗", Style::default().fg(Color::Red).bold()),
                CheckStatus::Warn => ("!", Style::default().fg(Color::Yellow)),
                CheckStatus::Running => ("⠋", Style::default().fg(Color::Blue)),
                CheckStatus::Skipped => ("—", theme.dim_style()),
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {icon} "), style),
                Span::styled(format!("{:<20}", check.name), style),
                Span::styled(&check.detail, theme.dim_style()),
            ]))
        }).collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::NONE));
        frame.render_widget(list, chunks[1]);

        // Footer
        let all_pass = self.checks.iter().all(|c| c.status == CheckStatus::Pass || c.status == CheckStatus::Skipped);
        let footer_text = if self.is_running {
            "Running checks..."
        } else if all_pass {
            "All checks passed! Press q to return."
        } else {
            "Some checks need attention. Press q to return."
        };
        let footer = Paragraph::new(Span::styled(footer_text, theme.dim_style()))
            .alignment(Alignment::Center);
        frame.render_widget(footer, chunks[2]);
    }
}

impl Default for DoctorScreen {
    fn default() -> Self { Self::new() }
}
