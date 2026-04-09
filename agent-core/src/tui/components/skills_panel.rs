//! Skills panel — shows available and active skills.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};
use crate::tui::theme::Theme;

#[derive(Debug, Clone)]
pub struct SkillEntry { pub name: String, pub description: String, pub source: String }

pub struct SkillsPanelWidget<'a> {
    skills: &'a [SkillEntry],
    theme: &'a Theme,
}

impl<'a> SkillsPanelWidget<'a> {
    pub fn new(skills: &'a [SkillEntry], theme: &'a Theme) -> Self {
        SkillsPanelWidget { skills, theme }
    }
}

impl<'a> Widget for SkillsPanelWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let items: Vec<ListItem> = self.skills.iter().map(|s| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("/{} ", s.name), self.theme.accent_style()),
                Span::styled(&s.description, self.theme.dim_style()),
                Span::styled(format!(" ({})", s.source), self.theme.dim_style()),
            ]))
        }).collect();
        Widget::render(List::new(items)
            .block(Block::default().title(format!(" Skills ({}) ", self.skills.len())).borders(Borders::ALL).border_style(self.theme.border_style())),
            area, buf);
    }
}
