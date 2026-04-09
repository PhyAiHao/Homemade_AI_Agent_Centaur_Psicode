//! Renderer — draws TUI frames using Ratatui.
//!
//! Replaces `src/ink/ink.tsx` and the React render tree. Uses Ratatui's
//! immediate-mode rendering to draw the entire UI each frame.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap, List, ListItem};

use super::{App, DisplayMessage, DisplayRole, ViewMode};
use super::buddy::BuddyWidget;
use super::widgets;
use super::components::notices::NotificationToastWidget;
use super::screens::settings::SettingsView;

/// Height of the buddy top panel (rows).
/// 12 art lines (sleeping) + 1 blank + 1 label + 2 border = 16; +1 breathing = 17.
const BUDDY_PANEL_HEIGHT: u16 = 17;
/// Minimum terminal height before the buddy top panel is hidden.
/// buddy(17) + messages(1) + status(1) + input(3) = 22 rows minimum.
const BUDDY_MIN_TERM_HEIGHT: u16 = 22;

/// Draw a complete frame.
pub fn draw_frame(frame: &mut Frame, app: &App) {
    let area = frame.area();

    // ── Vertical split: [buddy panel (top) | chat area (bottom)] ─────────
    // The top panel only appears when the buddy is enabled and the terminal
    // is tall enough. Short terminals get full-height chat.
    let show_buddy = app.buddy.enabled && area.height >= BUDDY_MIN_TERM_HEIGHT;
    let buddy_h = if show_buddy { BUDDY_PANEL_HEIGHT } else { 0 };

    let v_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(buddy_h),
            Constraint::Min(5),   // allow very short terminals to still show chat
        ])
        .split(area);

    let buddy_area = v_chunks[0];
    let chat_area  = v_chunks[1];

    // ── Vertical split of the chat area ───────────────────────────────────
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(5),       // Messages
            Constraint::Length(1),    // Status line
            Constraint::Length(3),    // Input
        ])
        .split(chat_area);

    // Draw chat sections
    draw_messages(frame, app, chunks[0]);
    draw_status_line(frame, app, chunks[1]);
    draw_input(frame, app, chunks[2]);

    // Draw buddy top panel
    if show_buddy {
        draw_buddy_panel(frame, app, buddy_area);
    }

    // Processing indicator overlay (small centred bar while streaming)
    if let ViewMode::Processing { ref message } = app.view_mode {
        let msg_text = format!(" ⠿ {} ", message);
        let width = (msg_text.len() as u16 + 4).min(chat_area.width);
        let x = chat_area.x + (chat_area.width.saturating_sub(width)) / 2;
        let y = chat_area.y + chat_area.height.saturating_sub(5);
        let indicator_area = Rect::new(x, y, width, 1);
        let indicator = Paragraph::new(msg_text)
            .style(app.theme.status_bg_style());
        frame.render_widget(indicator, indicator_area);
    }

    // Overlay: permission dialog
    if app.view_mode == ViewMode::PermissionPrompt {
        if let Some(ref dialog_state) = app.permission_dialog {
            let dialog = widgets::permission_dialog::PermissionDialogWidget::new(
                dialog_state, &app.theme
            );
            frame.render_widget(dialog, area);
        }
    }

    // R3: History search overlay
    if app.view_mode == ViewMode::HistorySearch {
        draw_history_search(frame, app, area);
    }

    // R5: Shortcuts help overlay
    if app.view_mode == ViewMode::ShortcutsHelp {
        draw_shortcuts_help(frame, app, area);
    }

    // R9: Settings overlay
    if app.view_mode == ViewMode::Settings {
        let view = SettingsView {
            model: app.model.clone(),
            permission_mode: app.permission_mode.clone(),
            theme_name: app.theme.name.clone(),
            vim_enabled: app.input.vim_enabled,
            plan_mode: app.plan_mode,
            voice_enabled: app.input.is_voice_enabled(),
        };
        view.render(frame, &app.theme);
    }

    // W6: Agent creation wizard overlay
    if app.view_mode == ViewMode::AgentWizard {
        if let Some(ref wizard_state) = app.agent_wizard {
            use super::components::agent_wizard::AgentWizardWidget;
            let wizard = AgentWizardWidget::new(wizard_state, &app.theme);
            frame.render_widget(wizard, area);
        }
    }

    // Setup wizard overlay (provider/model/API key)
    if app.view_mode == ViewMode::SetupWizard {
        if let Some(ref wizard_state) = app.setup_wizard {
            use super::screens::setup_wizard::SetupWizardWidget;
            let widget = SetupWizardWidget { state: wizard_state, theme: &app.theme };
            widget.render(frame);
        }
    }

    // R6: Notification toasts (always on top)
    if app.notification_manager.has_notifications() {
        let toast_widget = NotificationToastWidget::new(
            &app.notification_manager, &app.theme
        );
        frame.render_widget(toast_widget, area);
    }
}

/// Draw the buddy sidebar panel.
fn draw_buddy_panel(frame: &mut Frame, app: &App, area: Rect) {
    let widget = BuddyWidget::new(&app.buddy, &app.theme);
    frame.render_widget(widget, area);
}

/// Format a message timestamp (ms since epoch) as a short "HH:MM" UTC string.
fn format_msg_timestamp(msg: &DisplayMessage) -> String {
    if msg.timestamp == 0 {
        return String::new();
    }
    let secs = msg.timestamp / 1_000;
    let seconds_in_day = secs % 86_400;
    let h = seconds_in_day / 3_600;
    let m = (seconds_in_day % 3_600) / 60;
    format!("{h:02}:{m:02} ")
}

/// Draw the message list area.
fn draw_messages(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    // Build lines from messages
    let mut lines: Vec<Line> = Vec::new();

    for msg in &app.messages {
        let (prefix, style) = match msg.role {
            DisplayRole::User => {
                // Header line: "> HH:MM" (timestamp is dim, prefix is user-styled)
                let ts = format_msg_timestamp(msg);
                lines.push(Line::from(vec![
                    Span::styled("> ", theme.user_style()),
                    Span::styled(ts, theme.dim_style()),
                ]));
                // Content lines are indented under the header
                ("  ", theme.user_style())
            }
            DisplayRole::Assistant => {
                ("", theme.assistant_style())
            }
            DisplayRole::System => {
                ("* ", theme.system_style())
            }
            DisplayRole::ToolUse => {
                let tool_name = msg.tool_info.as_ref()
                    .map(|t| t.tool_name.as_str())
                    .unwrap_or("tool");
                lines.push(Line::from(vec![
                    Span::styled(
                        format!("[{tool_name}] "),
                        theme.tool_name_style()
                    ),
                ]));
                ("  ", theme.tool_output_style())
            }
            DisplayRole::ToolResult => {
                let info = msg.tool_info.as_ref();
                let tool_name = info.map(|t| t.tool_name.as_str()).unwrap_or("result");
                let duration = info.and_then(|t| t.duration_ms)
                    .map(|d| format!(" ({d}ms)"))
                    .unwrap_or_default();
                // Show the tool_id (first 8 chars) for traceability
                let id_hint = info
                    .map(|t| format!(" #{}", &t.tool_id[..t.tool_id.len().min(8)]))
                    .unwrap_or_default();
                let is_error = info.map(|t| t.is_error).unwrap_or(false);

                let header_style = if is_error {
                    theme.error_style()
                } else {
                    theme.tool_name_style()
                };

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("[{tool_name}{duration}{id_hint}] "),
                        header_style,
                    ),
                ]));
                ("  ", theme.tool_output_style())
            }
        };

        // Render content lines
        for content_line in msg.content.lines() {
            lines.push(Line::from(vec![
                Span::styled(prefix, style),
                Span::styled(content_line, style),
            ]));
        }

        // Blank line between messages
        lines.push(Line::from(""));
    }

    // Add streaming text (currently being received)
    if app.is_streaming && !app.streaming_text.is_empty() {
        for line in app.streaming_text.lines() {
            lines.push(Line::from(Span::styled(line, theme.assistant_style())));
        }
        // Cursor indicator
        lines.push(Line::from(Span::styled("▌", theme.cursor_style())));
    }

    // Show active tools
    for tool in &app.active_tools {
        let elapsed = tool.started_at.elapsed().as_secs();
        let spinner = spinner_char(elapsed);
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {spinner} {} ({elapsed}s)", tool.name),
                theme.tool_active_style(),
            ),
        ]));
    }

    // Calculate scroll
    let visible_height = area.height as usize;
    let total_lines = lines.len();
    let scroll = if app.scroll_offset == 0 {
        // Auto-follow: show the bottom
        total_lines.saturating_sub(visible_height) as u16
    } else {
        total_lines.saturating_sub(visible_height).saturating_sub(app.scroll_offset as usize) as u16
    };

    let messages_widget = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));

    frame.render_widget(messages_widget, area);
}

/// Draw the status line.
fn draw_status_line(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let left = Span::styled(&app.status_left, theme.status_style());
    let right = Span::styled(&app.status_right, theme.status_style());

    // Fill the gap with spaces
    let gap = area.width as usize
        - app.status_left.len().min(area.width as usize / 2)
        - app.status_right.len().min(area.width as usize / 2);
    let fill = " ".repeat(gap.max(1));

    let line = Line::from(vec![left, Span::raw(fill), right]);
    let status = Paragraph::new(line)
        .style(theme.status_bg_style());

    frame.render_widget(status, area);
}

/// Draw the input area.
fn draw_input(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let input_text = app.input.display_text();
    let cursor_pos = app.input.cursor_position();

    // R10: Build input line with ghost text shown as dim text after cursor
    let mut spans = vec![
        Span::styled(input_text, theme.input_style()),
    ];
    if let Some(ghost) = app.input.ghost_text_str() {
        spans.push(Span::styled(ghost, theme.dim_style()));
    }

    let input_line = Line::from(spans);
    let input_widget = Paragraph::new(input_line)
        .block(Block::default()
            .borders(Borders::TOP)
            .border_style(theme.border_style())
            .title(" > "))
        .style(theme.input_style());

    frame.render_widget(input_widget, area);

    // Position cursor
    frame.set_cursor_position(Position::new(
        area.x + cursor_pos as u16 + 3, // +3 for " > " prefix
        area.y + 1,
    ));
}

/// Animated spinner character.
fn spinner_char(elapsed_secs: u64) -> char {
    const CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    CHARS[(elapsed_secs as usize) % CHARS.len()]
}

// ─── R3: History search overlay ────────────────────────────────────────────

fn draw_history_search(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 15u16.min(area.height.saturating_sub(4));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" History Search (Ctrl+R) ")
        .borders(Borders::ALL)
        .border_style(theme.accent_style());

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    // Layout: search input (2 lines) + results list + footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(inner);

    // Search query
    let query_display = if app.history_search_query.is_empty() {
        "Type to search history..."
    } else {
        &app.history_search_query
    };
    let query_style = if app.history_search_query.is_empty() {
        theme.dim_style()
    } else {
        theme.input_style()
    };
    let query_widget = Paragraph::new(Span::styled(
        format!("> {}", query_display),
        query_style,
    ));
    frame.render_widget(query_widget, chunks[0]);

    // Results
    let items: Vec<ListItem> = app.history_search_results.iter().enumerate().map(|(i, entry)| {
        let style = if i == app.history_search_selected {
            Style::default().reversed()
        } else {
            theme.assistant_style()
        };
        let truncated = if entry.len() > (chunks[1].width as usize).saturating_sub(2) {
            format!("{}...", &entry[..(chunks[1].width as usize).saturating_sub(5)])
        } else {
            entry.clone()
        };
        ListItem::new(Span::styled(truncated, style))
    }).collect();

    let list = List::new(items);
    frame.render_widget(list, chunks[1]);

    // Footer
    let footer = Paragraph::new(Span::styled(
        "Enter to select | Esc to cancel | Up/Down to navigate",
        theme.dim_style(),
    ));
    frame.render_widget(footer, chunks[2]);
}

// ─── R5: Shortcuts help overlay ────────────────────────────────────────────

fn draw_shortcuts_help(frame: &mut Frame, app: &App, area: Rect) {
    let theme = &app.theme;

    let width = 60u16.min(area.width.saturating_sub(4));
    let height = 26u16.min(area.height.saturating_sub(2));
    let x = (area.width.saturating_sub(width)) / 2 + area.x;
    let y = (area.height.saturating_sub(height)) / 2 + area.y;
    let dialog_area = Rect::new(x, y, width, height);

    frame.render_widget(Clear, dialog_area);

    let block = Block::default()
        .title(" Keyboard Shortcuts ")
        .borders(Borders::ALL)
        .border_style(theme.accent_style());

    let inner = block.inner(dialog_area);
    frame.render_widget(block, dialog_area);

    let lines = vec![
        Line::from(""),
        Line::from(Span::styled("  Navigation", theme.accent_style().bold())),
        Line::from(vec![
            Span::styled("    PageUp/PageDown   ", theme.assistant_style()),
            Span::styled("Scroll messages", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl+R            ", theme.assistant_style()),
            Span::styled("Search history", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    F1                ", theme.assistant_style()),
            Span::styled("Show this help", theme.dim_style()),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Editing", theme.accent_style().bold())),
        Line::from(vec![
            Span::styled("    Ctrl+A            ", theme.assistant_style()),
            Span::styled("Start of line", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl+E            ", theme.assistant_style()),
            Span::styled("End of line", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl+U            ", theme.assistant_style()),
            Span::styled("Delete to start", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl+K            ", theme.assistant_style()),
            Span::styled("Delete to end", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Ctrl+W            ", theme.assistant_style()),
            Span::styled("Delete previous word", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Tab               ", theme.assistant_style()),
            Span::styled("Accept ghost text", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Shift+Enter       ", theme.assistant_style()),
            Span::styled("Insert newline", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Up/Down           ", theme.assistant_style()),
            Span::styled("History navigation", theme.dim_style()),
        ]),
        Line::from(""),
        Line::from(Span::styled("  Commands", theme.accent_style().bold())),
        Line::from(vec![
            Span::styled("    Ctrl+C            ", theme.assistant_style()),
            Span::styled("Quit", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    Enter             ", theme.assistant_style()),
            Span::styled("Submit input", theme.dim_style()),
        ]),
        Line::from(vec![
            Span::styled("    /help             ", theme.assistant_style()),
            Span::styled("Show commands", theme.dim_style()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  Press Esc to close",
            theme.dim_style(),
        )),
    ];

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}
