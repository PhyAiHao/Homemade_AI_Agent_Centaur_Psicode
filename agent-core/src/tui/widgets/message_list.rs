//! Message list widget — renders the conversation as a virtual-scrolled list
//! with search bar support, regex match highlighting, and n/N navigation.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Widget, Wrap};

use crate::tui::{App, DisplayMessage, DisplayRole};
use crate::tui::markdown;
use crate::tui::theme::Theme;

// ─── Search state ──────────────────────────────────────────────────────────

/// Tracks the search bar state for the message list.
#[derive(Debug, Clone, Default)]
pub struct SearchState {
    /// Whether the search bar is currently visible / active.
    pub active: bool,
    /// The raw text typed in the search bar.
    pub query: String,
    /// Compiled regex (built from `query`). `None` when empty or invalid.
    compiled: Option<regex::Regex>,
    /// Flat list of (message_index, byte_start, byte_end) for every match.
    pub matches: Vec<(usize, usize, usize)>,
    /// Index into `matches` for the currently focused match.
    pub current_match: usize,
}

impl SearchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Recompile the regex from the current query and rescan messages.
    pub fn update(&mut self, messages: &[DisplayMessage]) {
        self.compiled = if self.query.is_empty() {
            None
        } else {
            regex::Regex::new(&self.query).ok()
        };
        self.rescan(messages);
    }

    fn rescan(&mut self, messages: &[DisplayMessage]) {
        self.matches.clear();
        if let Some(ref re) = self.compiled {
            for (mi, msg) in messages.iter().enumerate() {
                for mat in re.find_iter(&msg.content) {
                    self.matches.push((mi, mat.start(), mat.end()));
                }
            }
        }
        if self.current_match >= self.matches.len() {
            self.current_match = 0;
        }
    }

    /// Navigate to the next match. Returns the message index of the match.
    pub fn next_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        self.current_match = (self.current_match + 1) % self.matches.len();
        Some(self.matches[self.current_match].0)
    }

    /// Navigate to the previous match. Returns the message index of the match.
    pub fn prev_match(&mut self) -> Option<usize> {
        if self.matches.is_empty() {
            return None;
        }
        if self.current_match == 0 {
            self.current_match = self.matches.len() - 1;
        } else {
            self.current_match -= 1;
        }
        Some(self.matches[self.current_match].0)
    }

    /// Check if a given byte range in a given message is a match. If it is
    /// the current match, returns `true` for the second element.
    pub fn is_match(&self, msg_idx: usize, byte_start: usize, byte_end: usize) -> (bool, bool) {
        let mut any = false;
        let mut current = false;
        for (i, &(mi, ms, me)) in self.matches.iter().enumerate() {
            if mi == msg_idx && ms == byte_start && me == byte_end {
                any = true;
                if i == self.current_match {
                    current = true;
                }
            }
        }
        (any, current)
    }

    pub fn match_count(&self) -> usize {
        self.matches.len()
    }
}

// ─── Height cache ──────────────────────────────────────────────────────────

/// Estimated height (in terminal rows) of a rendered message at a given width.
fn estimate_message_height(msg: &DisplayMessage, width: u16) -> u16 {
    if width == 0 {
        return 1;
    }
    let w = width as usize;
    // 1 line for role header
    let header_lines: u16 = 1;
    // Content lines: each source line wraps based on width
    let content_lines: u16 = msg
        .content
        .lines()
        .map(|l| {
            let len = l.len().max(1);
            len.div_ceil(w) as u16
        })
        .sum::<u16>()
        .max(1);
    // 1 blank separator line
    header_lines + content_lines + 1
}

// ─── Widget ────────────────────────────────────────────────────────────────

/// A virtual-scrolled, searchable message list widget.
pub struct MessageListWidget<'a> {
    messages: &'a [DisplayMessage],
    streaming_text: &'a str,
    is_streaming: bool,
    theme: &'a Theme,
    scroll_offset: u16,
    search: &'a SearchState,
}

impl<'a> MessageListWidget<'a> {
    pub fn new(app: &'a App, search: &'a SearchState) -> Self {
        MessageListWidget {
            messages: &app.messages,
            streaming_text: &app.streaming_text,
            is_streaming: app.is_streaming,
            theme: &app.theme,
            scroll_offset: app.scroll_offset,
            search,
        }
    }

    /// Alternate constructor for when you have the parts individually.
    pub fn from_parts(
        messages: &'a [DisplayMessage],
        streaming_text: &'a str,
        is_streaming: bool,
        theme: &'a Theme,
        scroll_offset: u16,
        search: &'a SearchState,
    ) -> Self {
        MessageListWidget {
            messages,
            streaming_text,
            is_streaming,
            theme,
            scroll_offset,
            search,
        }
    }

    /// Compute total estimated height for all messages.
    pub fn total_height(&self, width: u16) -> u16 {
        self.messages
            .iter()
            .map(|m| estimate_message_height(m, width))
            .sum::<u16>()
            // streaming tail
            + if self.is_streaming && !self.streaming_text.is_empty() {
                let lines = self.streaming_text.lines().count().max(1) as u16 + 1;
                lines
            } else {
                0
            }
    }

    /// Determine the range of message indices that are visible given the area
    /// height and current scroll offset.  Returns `(first_visible, last_visible_exclusive)`.
    fn visible_range(&self, width: u16, height: u16) -> (usize, usize) {
        let heights: Vec<u16> = self
            .messages
            .iter()
            .map(|m| estimate_message_height(m, width))
            .collect();
        let total: u16 = heights.iter().copied().sum();

        // scroll_offset == 0 => auto-follow (show bottom)
        let scroll = if self.scroll_offset == 0 {
            total.saturating_sub(height)
        } else {
            total
                .saturating_sub(height)
                .saturating_sub(self.scroll_offset)
        };

        let mut first = 0usize;
        let mut acc: u16 = 0;
        for (i, &h) in heights.iter().enumerate() {
            if acc + h > scroll {
                first = i;
                break;
            }
            acc += h;
            first = i + 1;
        }

        let mut last = first;
        let mut cumulative: u16 = 0;
        for &h in &heights[first..] {
            cumulative += h;
            last += 1;
            if cumulative >= height + heights.get(first).copied().unwrap_or(0) {
                break;
            }
        }

        (first, last.min(self.messages.len()))
    }

    /// Render lines for a single message, applying search highlights.
    fn render_message_lines(
        &self,
        msg: &DisplayMessage,
        msg_idx: usize,
    ) -> Vec<Line<'static>> {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Role header
        let role_label = match msg.role {
            DisplayRole::User => ("You", self.theme.user_style()),
            DisplayRole::Assistant => ("Assistant", self.theme.assistant_style()),
            DisplayRole::System => ("System", self.theme.system_style()),
            DisplayRole::ToolUse => ("Tool", self.theme.tool_name_style()),
            DisplayRole::ToolResult => {
                let name = msg
                    .tool_info
                    .as_ref()
                    .map(|t| t.tool_name.as_str())
                    .unwrap_or("Result");
                (name, self.theme.tool_output_style())
            }
        };

        lines.push(Line::from(Span::styled(
            format!("{}: ", role_label.0),
            role_label.1,
        )));

        // Content
        let base_style = match msg.role {
            DisplayRole::User => self.theme.user_style(),
            DisplayRole::Assistant => self.theme.assistant_style(),
            DisplayRole::System => self.theme.system_style(),
            DisplayRole::ToolUse => self.theme.tool_output_style(),
            DisplayRole::ToolResult => {
                if msg.tool_info.as_ref().map(|t| t.is_error).unwrap_or(false) {
                    self.theme.error_style()
                } else {
                    self.theme.tool_output_style()
                }
            }
        };

        if msg.role == DisplayRole::Assistant {
            let md_lines = markdown::render_markdown(&msg.content, self.theme);
            // Apply search highlights on top of markdown lines
            if self.search.compiled.is_some() {
                lines.extend(self.highlight_lines(md_lines, msg_idx, &msg.content));
            } else {
                lines.extend(md_lines);
            }
        } else if self.search.compiled.is_some() {
            // Render with highlights
            let plain_lines: Vec<Line<'static>> = msg
                .content
                .lines()
                .map(|l| Line::from(Span::styled(l.to_string(), base_style)))
                .collect();
            lines.extend(self.highlight_lines(plain_lines, msg_idx, &msg.content));
        } else {
            for content_line in msg.content.lines() {
                lines.push(Line::from(Span::styled(
                    content_line.to_string(),
                    base_style,
                )));
            }
        }

        // Blank separator
        lines.push(Line::from(""));
        lines
    }

    /// Apply search-match highlighting to pre-rendered lines.
    /// We re-scan the original content and highlight matches.
    fn highlight_lines(
        &self,
        rendered: Vec<Line<'static>>,
        msg_idx: usize,
        _original: &str,
    ) -> Vec<Line<'static>> {
        let re = match &self.search.compiled {
            Some(r) => r,
            None => return rendered,
        };

        let highlight = Style::default()
            .bg(Color::Yellow)
            .fg(Color::Black);
        let current_highlight = Style::default()
            .bg(Color::Rgb(255, 140, 0))
            .fg(Color::Black)
            .bold();

        rendered
            .into_iter()
            .map(|line| {
                // Flatten the line to a plain string to search
                let plain: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                if !re.is_match(&plain) {
                    return line;
                }
                // Rebuild with highlights
                let mut spans: Vec<Span<'static>> = Vec::new();
                let mut last = 0usize;
                let default_style = if line.spans.is_empty() {
                    Style::default()
                } else {
                    line.spans[0].style
                };
                for mat in re.find_iter(&plain) {
                    if mat.start() > last {
                        spans.push(Span::styled(
                            plain[last..mat.start()].to_string(),
                            default_style,
                        ));
                    }
                    let (_any, is_current) =
                        self.search.is_match(msg_idx, mat.start(), mat.end());
                    let hl = if is_current { current_highlight } else { highlight };
                    spans.push(Span::styled(
                        plain[mat.start()..mat.end()].to_string(),
                        hl,
                    ));
                    last = mat.end();
                }
                if last < plain.len() {
                    spans.push(Span::styled(plain[last..].to_string(), default_style));
                }
                Line::from(spans)
            })
            .collect()
    }
}

impl<'a> Widget for MessageListWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Reserve space for search bar if active
        let (search_area, messages_area) = if self.search.active {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(1), Constraint::Min(1)])
                .split(area);
            (Some(chunks[0]), chunks[1])
        } else {
            (None, area)
        };

        // ── Search bar ─────────────────────────────────────────────────
        if let Some(sa) = search_area {
            let match_info = if self.search.matches.is_empty() {
                if self.search.query.is_empty() {
                    String::new()
                } else {
                    " (no matches)".to_string()
                }
            } else {
                format!(
                    " ({}/{})",
                    self.search.current_match + 1,
                    self.search.matches.len()
                )
            };
            let search_line = Line::from(vec![
                Span::styled(
                    " /",
                    Style::default().fg(Color::Yellow).bold(),
                ),
                Span::styled(
                    self.search.query.clone(),
                    Style::default().fg(Color::White),
                ),
                Span::styled(match_info, Style::default().fg(Color::DarkGray)),
            ]);
            buf.set_line(sa.x, sa.y, &search_line, sa.width);
        }

        // ── Virtual scroll: only render visible messages ───────────────
        let width = messages_area.width;
        let height = messages_area.height;
        let (first, last) = self.visible_range(width, height);

        let mut lines: Vec<Line<'static>> = Vec::new();

        for i in first..last {
            let msg = &self.messages[i];
            lines.extend(self.render_message_lines(msg, i));
        }

        // Streaming content (always at the bottom)
        if self.is_streaming && !self.streaming_text.is_empty() {
            let md_lines = markdown::render_markdown(self.streaming_text, self.theme);
            lines.extend(md_lines);
            lines.push(Line::from(Span::styled(
                "\u{258c}",
                self.theme.cursor_style(),
            )));
        }

        // Compute auto-scroll offset
        let auto_scroll = if self.scroll_offset == 0 {
            // Follow mode — show bottom
            lines.len().saturating_sub(height as usize) as u16
        } else {
            0
        };

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((auto_scroll, 0));

        paragraph.render(messages_area, buf);
    }
}
