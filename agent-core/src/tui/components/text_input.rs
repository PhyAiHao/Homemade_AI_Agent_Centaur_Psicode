//! Text input component — multiline text input with prompt.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use crate::tui::theme::Theme;

pub struct TextInputWidget<'a> {
    text: &'a str,
    cursor_pos: usize,
    placeholder: &'a str,
    theme: &'a Theme,
    focused: bool,
}

impl<'a> TextInputWidget<'a> {
    pub fn new(text: &'a str, cursor_pos: usize, theme: &'a Theme) -> Self {
        TextInputWidget {
            text,
            cursor_pos,
            placeholder: "Type a message...",
            theme,
            focused: true,
        }
    }

    pub fn placeholder(mut self, text: &'a str) -> Self {
        self.placeholder = text;
        self
    }

    pub fn focused(mut self, focused: bool) -> Self {
        self.focused = focused;
        self
    }
}

impl<'a> Widget for TextInputWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let display = if self.text.is_empty() {
            self.placeholder
        } else {
            self.text
        };

        let style = if self.text.is_empty() {
            self.theme.dim_style()
        } else {
            self.theme.input_style()
        };

        let border_style = if self.focused {
            self.theme.accent_style()
        } else {
            self.theme.border_style()
        };

        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(border_style)
            .title(" > ");

        let paragraph = Paragraph::new(Span::styled(display.to_string(), style))
            .block(block);

        paragraph.render(area, buf);
    }
}
