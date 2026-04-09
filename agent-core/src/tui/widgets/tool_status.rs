//! Tool status widget — shows currently running tools with spinners.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::Widget;

use crate::tui::ActiveTool;
use crate::tui::theme::Theme;

pub struct ToolStatusWidget<'a> {
    tools: &'a [ActiveTool],
    theme: &'a Theme,
}

impl<'a> ToolStatusWidget<'a> {
    pub fn new(tools: &'a [ActiveTool], theme: &'a Theme) -> Self {
        ToolStatusWidget { tools, theme }
    }
}

impl<'a> Widget for ToolStatusWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        if self.tools.is_empty() {
            return;
        }

        let spinners = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

        for (i, tool) in self.tools.iter().enumerate() {
            if i as u16 >= area.height {
                break;
            }

            let elapsed = tool.started_at.elapsed().as_secs();
            let spinner = spinners[(elapsed as usize) % spinners.len()];

            let line = Line::from(vec![
                Span::styled(
                    format!(" {spinner} "),
                    self.theme.tool_active_style(),
                ),
                Span::styled(
                    format!("{} ({elapsed}s)", tool.name),
                    self.theme.tool_active_style(),
                ),
            ]);

            let y = area.y + i as u16;
            buf.set_line(area.x, y, &line, area.width);
        }
    }
}
