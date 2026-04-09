//! Selectable list — a generic interactive list with keyboard navigation,
//! highlight, and selection.
#![allow(dead_code)]

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, Widget};
use crate::tui::theme::Theme;

/// State for a selectable list. Kept in the parent component.
#[derive(Debug, Clone)]
pub struct SelectableListState {
    /// Currently highlighted (focused) index.
    pub selected: usize,
    /// Total number of items.
    pub len: usize,
    /// Scroll offset for the list viewport.
    pub offset: usize,
}

impl SelectableListState {
    pub fn new(len: usize) -> Self {
        SelectableListState {
            selected: 0,
            len,
            offset: 0,
        }
    }

    pub fn select_next(&mut self) {
        if self.len == 0 {
            return;
        }
        self.selected = (self.selected + 1).min(self.len - 1);
    }

    pub fn select_prev(&mut self) {
        self.selected = self.selected.saturating_sub(1);
    }

    pub fn select_first(&mut self) {
        self.selected = 0;
    }

    pub fn select_last(&mut self) {
        if self.len > 0 {
            self.selected = self.len - 1;
        }
    }

    /// Ensure the selected item is visible within the given viewport height.
    pub fn ensure_visible(&mut self, viewport_height: usize) {
        if self.selected < self.offset {
            self.offset = self.selected;
        }
        if self.selected >= self.offset + viewport_height {
            self.offset = self.selected.saturating_sub(viewport_height - 1);
        }
    }

    pub fn update_len(&mut self, new_len: usize) {
        self.len = new_len;
        if self.selected >= new_len && new_len > 0 {
            self.selected = new_len - 1;
        }
    }
}

/// An item for the selectable list.
#[derive(Debug, Clone)]
pub struct SelectableItem {
    pub label: String,
    pub detail: Option<String>,
    pub icon: Option<String>,
    pub style: Option<Style>,
}

impl SelectableItem {
    pub fn new(label: impl Into<String>) -> Self {
        SelectableItem {
            label: label.into(),
            detail: None,
            icon: None,
            style: None,
        }
    }

    pub fn detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn icon(mut self, icon: impl Into<String>) -> Self {
        self.icon = Some(icon.into());
        self
    }

    pub fn with_style(mut self, style: Style) -> Self {
        self.style = Some(style);
        self
    }
}

/// A generic selectable list widget.
pub struct SelectableListWidget<'a> {
    items: &'a [SelectableItem],
    state: &'a SelectableListState,
    title: Option<&'a str>,
    theme: &'a Theme,
}

impl<'a> SelectableListWidget<'a> {
    pub fn new(
        items: &'a [SelectableItem],
        state: &'a SelectableListState,
        theme: &'a Theme,
    ) -> Self {
        SelectableListWidget {
            items,
            state,
            title: None,
            theme,
        }
    }

    pub fn title(mut self, title: &'a str) -> Self {
        self.title = Some(title);
        self
    }
}

impl<'a> Widget for SelectableListWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = if let Some(title) = self.title {
            Block::default()
                .title(format!(" {} ", title))
                .borders(Borders::ALL)
                .border_style(self.theme.border_style())
        } else {
            Block::default()
                .borders(Borders::ALL)
                .border_style(self.theme.border_style())
        };

        let inner = block.inner(area);
        block.render(area, buf);

        let viewport_h = inner.height as usize;
        let offset = self.state.offset;

        let visible: Vec<ListItem> = self
            .items
            .iter()
            .enumerate()
            .skip(offset)
            .take(viewport_h)
            .map(|(i, item)| {
                let is_selected = i == self.state.selected;
                let base_style = item.style.unwrap_or_else(|| self.theme.assistant_style());
                let style = if is_selected {
                    base_style.reversed()
                } else {
                    base_style
                };

                let mut spans = Vec::new();
                if let Some(ref icon) = item.icon {
                    spans.push(Span::styled(
                        format!("{} ", icon),
                        style,
                    ));
                }
                spans.push(Span::styled(item.label.clone(), style));
                if let Some(ref detail) = item.detail {
                    spans.push(Span::styled(
                        format!("  {}", detail),
                        if is_selected {
                            self.theme.dim_style().reversed()
                        } else {
                            self.theme.dim_style()
                        },
                    ));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();

        Widget::render(List::new(visible), inner, buf);
    }
}
