//! Permission dialog widget — interactive approval prompt for tool execution.
//!
//! Mirrors `src/components/permissions/` from the original.
//!
//! Features:
//! - Selectable option list (Allow/Deny/Always Allow/Always Allow in Dir)
//! - Tool-specific display (Bash shows command, FileEdit shows diff preview)
//! - Keyboard navigation (Up/Down/Enter, Y/n/a shortcuts)
//! - Feedback mode (Tab to type instructions alongside allow/deny)
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Widget, Wrap};

use crate::tui::theme::Theme;

// ─── Permission options ─────────────────────────────────────────────────────

/// What the user chose in the permission dialog.
#[derive(Debug, Clone, PartialEq)]
pub enum PermissionChoice {
    /// Allow this one execution.
    Allow,
    /// Deny this execution.
    Deny,
    /// Always allow this tool for the rest of the session.
    AlwaysAllow,
    /// Always allow this tool in this directory.
    AlwaysAllowDir,
    /// User wants to provide feedback/instructions before allowing.
    AllowWithFeedback(String),
}

/// The interactive state of the permission dialog.
pub struct PermissionDialogState {
    /// Currently selected option index.
    pub selected: usize,
    /// Available options.
    pub options: Vec<PermissionOption>,
    /// Whether the dialog is in feedback input mode.
    pub feedback_mode: bool,
    /// Feedback text being typed.
    pub feedback_text: String,
    /// The tool name being prompted for.
    pub tool_name: String,
    /// The tool input (command, file path, etc.).
    pub tool_input: String,
    /// Optional diff preview (for file edit tools).
    pub diff_preview: Option<String>,
    /// Whether the user has made a choice.
    pub choice: Option<PermissionChoice>,
}

#[derive(Debug, Clone)]
pub struct PermissionOption {
    pub label: String,
    pub shortcut: char,
    pub choice: PermissionChoice,
}

impl PermissionDialogState {
    /// Create a new permission dialog for the given tool.
    pub fn new(tool_name: &str, tool_input: &str) -> Self {
        let mut options = vec![
            PermissionOption {
                label: "Allow".into(),
                shortcut: 'y',
                choice: PermissionChoice::Allow,
            },
            PermissionOption {
                label: "Deny".into(),
                shortcut: 'n',
                choice: PermissionChoice::Deny,
            },
            PermissionOption {
                label: "Always allow this tool".into(),
                shortcut: 'a',
                choice: PermissionChoice::AlwaysAllow,
            },
        ];

        // Add directory-scoped option for file tools
        if tool_name == "FileEdit" || tool_name == "FileWrite" || tool_name == "Bash" {
            options.push(PermissionOption {
                label: "Always allow in this directory".into(),
                shortcut: 'd',
                choice: PermissionChoice::AlwaysAllowDir,
            });
        }

        PermissionDialogState {
            selected: 0,
            options,
            feedback_mode: false,
            feedback_text: String::new(),
            tool_name: tool_name.to_string(),
            tool_input: tool_input.to_string(),
            diff_preview: None,
            choice: None,
        }
    }

    /// Create a dialog with a diff preview (for FileEdit).
    pub fn with_diff(mut self, diff: String) -> Self {
        self.diff_preview = Some(diff);
        self
    }

    /// Handle a key press. Returns true if the dialog should close.
    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) -> bool {
        use crossterm::event::KeyCode;

        if self.feedback_mode {
            match key.code {
                KeyCode::Enter => {
                    self.choice = Some(PermissionChoice::AllowWithFeedback(
                        std::mem::take(&mut self.feedback_text)
                    ));
                    return true;
                }
                KeyCode::Esc => {
                    self.feedback_mode = false;
                    self.feedback_text.clear();
                }
                KeyCode::Backspace => { self.feedback_text.pop(); }
                KeyCode::Char(c) => { self.feedback_text.push(c); }
                _ => {}
            }
            return false;
        }

        match key.code {
            // Navigation
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = (self.selected + 1).min(self.options.len().saturating_sub(1));
            }
            // Select
            KeyCode::Enter => {
                if let Some(opt) = self.options.get(self.selected) {
                    self.choice = Some(opt.choice.clone());
                    return true;
                }
            }
            // Shortcuts
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.choice = Some(PermissionChoice::Allow);
                return true;
            }
            KeyCode::Char('n') | KeyCode::Char('N') => {
                self.choice = Some(PermissionChoice::Deny);
                return true;
            }
            KeyCode::Char('a') | KeyCode::Char('A') => {
                self.choice = Some(PermissionChoice::AlwaysAllow);
                return true;
            }
            KeyCode::Char('d') | KeyCode::Char('D') => {
                if self.options.iter().any(|o| o.shortcut == 'd') {
                    self.choice = Some(PermissionChoice::AlwaysAllowDir);
                    return true;
                }
            }
            // Tab → feedback mode
            KeyCode::Tab => {
                self.feedback_mode = true;
            }
            // Escape → deny
            KeyCode::Esc => {
                self.choice = Some(PermissionChoice::Deny);
                return true;
            }
            _ => {}
        }
        false
    }
}

// ─── Rendering ──────────────────────────────────────────────────────────────

pub struct PermissionDialogWidget<'a> {
    state: &'a PermissionDialogState,
    theme: &'a Theme,
}

impl<'a> PermissionDialogWidget<'a> {
    pub fn new(state: &'a PermissionDialogState, theme: &'a Theme) -> Self {
        PermissionDialogWidget { state, theme }
    }
}

impl<'a> Widget for PermissionDialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // ── Calculate dialog size ───────────────────────────────────────
        let has_diff = self.state.diff_preview.is_some();
        let dialog_width = area.width.min(if has_diff { 80 } else { 65 });
        let base_height = 8 + self.state.options.len() as u16;
        let diff_height = if has_diff { 8 } else { 0 };
        let feedback_height = if self.state.feedback_mode { 3 } else { 0 };
        let dialog_height = area.height.min(base_height + diff_height + feedback_height);

        let x = (area.width.saturating_sub(dialog_width)) / 2 + area.x;
        let y = (area.height.saturating_sub(dialog_height)) / 2 + area.y;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        // Clear background
        Clear.render(dialog_area, buf);

        // ── Build content ──────────────────────────────────────────────
        let mut lines: Vec<Line> = Vec::new();

        // Tool-specific header
        let header = match self.state.tool_name.as_str() {
            "Bash" => "Execute command?".to_string(),
            "FileEdit" => "Allow file edit?".to_string(),
            "FileWrite" => "Allow file write?".to_string(),
            "FileRead" => "Allow file read?".to_string(),
            _ => format!("Allow {}?", self.state.tool_name),
        };

        lines.push(Line::from(Span::styled(header, self.theme.accent_style().bold())));
        lines.push(Line::from(""));

        // Tool input display (tool-specific formatting)
        let display_input = format_tool_input(&self.state.tool_name, &self.state.tool_input);
        for input_line in display_input.lines().take(5) {
            lines.push(Line::from(Span::styled(
                input_line.to_string(),
                self.theme.dim_style(),
            )));
        }
        if display_input.lines().count() > 5 {
            lines.push(Line::from(Span::styled(
                "  ...(truncated)",
                self.theme.dim_style(),
            )));
        }

        lines.push(Line::from(""));

        // Diff preview (if present, for file edits)
        if let Some(ref diff) = self.state.diff_preview {
            lines.push(Line::from(Span::styled("Changes:", Style::default().bold())));
            for diff_line in diff.lines().take(6) {
                let style = if diff_line.starts_with('+') {
                    Style::default().fg(Color::Green)
                } else if diff_line.starts_with('-') {
                    Style::default().fg(Color::Red)
                } else if diff_line.starts_with("@@") {
                    Style::default().fg(Color::Cyan)
                } else {
                    self.theme.dim_style()
                };
                lines.push(Line::from(Span::styled(diff_line.to_string(), style)));
            }
            if diff.lines().count() > 6 {
                lines.push(Line::from(Span::styled(
                    format!("  ...({} more lines)", diff.lines().count() - 6),
                    self.theme.dim_style(),
                )));
            }
            lines.push(Line::from(""));
        }

        // Option list
        for (i, opt) in self.state.options.iter().enumerate() {
            let is_selected = i == self.state.selected;
            let marker = if is_selected { "▸ " } else { "  " };
            let shortcut_style = match opt.shortcut {
                'y' => Style::default().fg(Color::Green).bold(),
                'n' => Style::default().fg(Color::Red).bold(),
                'a' => Style::default().fg(Color::Yellow).bold(),
                'd' => Style::default().fg(Color::Blue).bold(),
                _ => Style::default().bold(),
            };
            let label_style = if is_selected {
                Style::default().reversed()
            } else {
                Style::default()
            };

            lines.push(Line::from(vec![
                Span::raw(marker),
                Span::styled(format!("[{}] ", opt.shortcut), shortcut_style),
                Span::styled(&opt.label, label_style),
            ]));
        }

        // Feedback mode input
        if self.state.feedback_mode {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Instructions (Enter to confirm, Esc to cancel):",
                Style::default().fg(Color::Cyan),
            )));
            lines.push(Line::from(Span::styled(
                format!("▸ {}_", self.state.feedback_text),
                Style::default().fg(Color::White),
            )));
        } else {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Tab: add instructions  Esc: deny",
                self.theme.dim_style(),
            )));
        }

        // ── Render ─────────────────────────────────────────────────────
        let title = format!(" {} Permission ", self.state.tool_name);
        let paragraph = Paragraph::new(lines)
            .block(Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(self.theme.accent_style()))
            .wrap(Wrap { trim: false });

        paragraph.render(dialog_area, buf);
    }
}

/// Format tool input for display, tool-specific.
fn format_tool_input(tool_name: &str, input: &str) -> String {
    match tool_name {
        "Bash" => {
            // Extract command from JSON input
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
                if let Some(cmd) = v.get("command").and_then(|c| c.as_str()) {
                    return format!("  $ {cmd}");
                }
            }
            format!("  {input}")
        }
        "FileEdit" | "FileWrite" => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(input) {
                let path = v.get("file_path").and_then(|p| p.as_str()).unwrap_or("?");
                return format!("  {path}");
            }
            format!("  {input}")
        }
        _ => {
            let truncated = if input.len() > 200 {
                format!("{}...", &input[..200])
            } else {
                input.to_string()
            };
            format!("  {truncated}")
        }
    }
}

// Keep the old simple widget for backward compatibility
pub struct PermissionDialog<'a> {
    tool_name: &'a str,
    tool_input: &'a str,
    theme: &'a Theme,
}

impl<'a> PermissionDialog<'a> {
    pub fn new(tool_name: &'a str, tool_input: &'a str, theme: &'a Theme) -> Self {
        PermissionDialog { tool_name, tool_input, theme }
    }
}

impl<'a> Widget for PermissionDialog<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let state = PermissionDialogState::new(self.tool_name, self.tool_input);
        PermissionDialogWidget::new(&state, self.theme).render(area, buf);
    }
}
