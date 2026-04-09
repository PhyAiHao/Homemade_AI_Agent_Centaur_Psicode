//! Layout engine — flex layout via the `taffy` crate + split pane support.
//!
//! Mirrors `src/ink/layout/` (yoga-layout bindings). Provides a thin
//! wrapper around taffy for CSS-like flexbox layout calculations.
//!
//! Also provides a `SplitLayout` for a resizable split-pane mode:
//! message area (left) | right panel (optional, for tasks/agents).
#![allow(dead_code)]

use taffy::prelude::*;

// ─── Taffy-based flex engine ───────────────────────────────────────────────

/// A layout tree for computing positions of nested UI elements.
pub struct LayoutEngine {
    tree: TaffyTree<()>,
}

impl LayoutEngine {
    pub fn new() -> Self {
        LayoutEngine {
            tree: TaffyTree::new(),
        }
    }

    /// Create a flex container node.
    pub fn flex_container(
        &mut self,
        direction: FlexDirection,
        children: &[NodeId],
    ) -> NodeId {
        let style = Style {
            display: Display::Flex,
            flex_direction: direction,
            size: Size {
                width: Dimension::Auto,
                height: Dimension::Auto,
            },
            ..Default::default()
        };

        self.tree.new_with_children(style, children).unwrap()
    }

    /// Create a fixed-size node.
    pub fn fixed_node(&mut self, width: f32, height: f32) -> NodeId {
        let style = Style {
            size: Size {
                width: length(width),
                height: length(height),
            },
            ..Default::default()
        };

        self.tree.new_leaf(style).unwrap()
    }

    /// Create a flex-grow node (fills remaining space).
    pub fn flex_grow_node(&mut self, grow: f32) -> NodeId {
        let style = Style {
            flex_grow: grow,
            size: Size {
                width: Dimension::Auto,
                height: Dimension::Auto,
            },
            ..Default::default()
        };

        self.tree.new_leaf(style).unwrap()
    }

    /// Compute layout for a tree rooted at `root` within available space.
    pub fn compute(&mut self, root: NodeId, available_width: f32, available_height: f32) {
        self.tree
            .compute_layout(
                root,
                Size {
                    width: AvailableSpace::Definite(available_width),
                    height: AvailableSpace::Definite(available_height),
                },
            )
            .unwrap();
    }

    /// Get the computed layout for a node.
    pub fn get_layout(&self, node: NodeId) -> &taffy::Layout {
        self.tree.layout(node).unwrap()
    }
}

impl Default for LayoutEngine {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Split pane mode ───────────────────────────────────────────────────────

/// What the right panel should display.
#[derive(Debug, Clone, PartialEq)]
pub enum RightPanelContent {
    /// No right panel — full-width message area.
    None,
    /// Active tools list.
    ActiveTools,
    /// Background tasks.
    Tasks,
    /// Agent / team panel.
    Agents,
}

/// State for the resizable split pane layout.
#[derive(Debug, Clone)]
pub struct SplitLayout {
    /// Whether the right panel is visible.
    pub right_panel: RightPanelContent,
    /// Width percentage of the left (message) panel, 0..100.
    /// The right panel gets `100 - split_pct`.
    pub split_pct: u16,
}

impl Default for SplitLayout {
    fn default() -> Self {
        SplitLayout {
            right_panel: RightPanelContent::None,
            split_pct: 70,
        }
    }
}

impl SplitLayout {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle the right panel content. If already showing the requested
    /// content, close the panel.
    pub fn toggle(&mut self, content: RightPanelContent) {
        if self.right_panel == content {
            self.right_panel = RightPanelContent::None;
        } else {
            self.right_panel = content;
        }
    }

    /// Adjust the split — move the divider left (more room for right panel).
    pub fn shrink_left(&mut self, amount: u16) {
        self.split_pct = self.split_pct.saturating_sub(amount).max(30);
    }

    /// Adjust the split — move the divider right (more room for left panel).
    pub fn grow_left(&mut self, amount: u16) {
        self.split_pct = (self.split_pct + amount).min(90);
    }

    /// Compute the two `ratatui::layout::Rect` regions for left and right panels.
    /// If the right panel is `None`, left gets the full area.
    pub fn split(&self, area: ratatui::layout::Rect) -> (ratatui::layout::Rect, Option<ratatui::layout::Rect>) {
        if self.right_panel == RightPanelContent::None {
            return (area, None);
        }

        let left_width =
            (area.width as u32 * self.split_pct as u32 / 100) as u16;
        let right_width = area.width.saturating_sub(left_width);

        let left = ratatui::layout::Rect::new(area.x, area.y, left_width, area.height);
        let right = ratatui::layout::Rect::new(
            area.x + left_width,
            area.y,
            right_width,
            area.height,
        );
        (left, Some(right))
    }

    /// Whether the right panel is currently visible.
    pub fn has_right_panel(&self) -> bool {
        self.right_panel != RightPanelContent::None
    }
}

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_flex_layout() {
        let mut engine = LayoutEngine::new();
        let child1 = engine.fixed_node(100.0, 30.0);
        let child2 = engine.flex_grow_node(1.0);
        let root = engine.flex_container(FlexDirection::Column, &[child1, child2]);

        engine.compute(root, 200.0, 100.0);

        // Auto-sized root shrinks to fit the widest child (100px).
        let root_layout = engine.get_layout(root);
        assert_eq!(root_layout.size.width, 100.0);

        let child1_layout = engine.get_layout(child1);
        assert_eq!(child1_layout.size.height, 30.0);
    }

    #[test]
    fn test_split_layout_no_panel() {
        let layout = SplitLayout::default();
        let area = ratatui::layout::Rect::new(0, 0, 120, 40);
        let (left, right) = layout.split(area);
        assert_eq!(left, area);
        assert!(right.is_none());
    }

    #[test]
    fn test_split_layout_with_panel() {
        let mut layout = SplitLayout::default();
        layout.right_panel = RightPanelContent::Tasks;
        let area = ratatui::layout::Rect::new(0, 0, 100, 40);
        let (left, right) = layout.split(area);
        assert_eq!(left.width, 70);
        assert_eq!(right.unwrap().width, 30);
    }

    #[test]
    fn test_split_resize() {
        let mut layout = SplitLayout::default();
        layout.right_panel = RightPanelContent::ActiveTools;
        layout.shrink_left(10);
        assert_eq!(layout.split_pct, 60);
        layout.grow_left(5);
        assert_eq!(layout.split_pct, 65);
    }
}
