//! Memory viewer — displays saved memories from the memory system.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};
use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct MemoryEntry { pub name: String, pub mem_type: String, pub description: String }

pub struct MemoryViewerWidget<'a> {
    entries: &'a [MemoryEntry],
    selected: usize,
    theme: &'a Theme,
}

impl<'a> MemoryViewerWidget<'a> {
    pub fn new(entries: &'a [MemoryEntry], selected: usize, theme: &'a Theme) -> Self {
        MemoryViewerWidget { entries, selected, theme }
    }
}

impl<'a> Widget for MemoryViewerWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self.entries.iter().enumerate().map(|(i, e)| {
            let style = if i == self.selected { Style::default().reversed() } else { self.theme.assistant_style() };
            ListItem::new(Line::from(vec![
                Span::styled(format!("[{}] ", e.mem_type), self.theme.tool_name_style()),
                Span::styled(&e.name, style),
                Span::styled(format!(" — {}", e.description), self.theme.dim_style()),
            ]))
        }).collect();
        Widget::render(List::new(items)
            .block(Block::default().title(" Memories ").borders(Borders::ALL).border_style(self.theme.border_style())),
            area, buf);
    }
}
