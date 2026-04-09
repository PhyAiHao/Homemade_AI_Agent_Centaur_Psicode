//! Vim types — state and mode definitions.
#![allow(dead_code)]

/// Vim editing mode.
#[derive(Debug, Clone, PartialEq)]
pub enum VimMode {
    Normal,
    Insert,
    Visual,
    Replace,
}

/// Complete vim editor state.
#[derive(Debug, Clone)]
pub struct VimState {
    pub mode: VimMode,
    /// Pending operator (e.g., 'd' waiting for a motion).
    pub pending_operator: Option<Operator>,
    /// Numeric prefix for repeat count.
    pub count: Option<u32>,
    /// Register for yank/delete.
    pub register: char,
    /// Last search pattern.
    pub last_search: Option<String>,
    /// Visual mode anchor position.
    pub visual_anchor: Option<usize>,
    /// Clipboard.
    pub clipboard: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Operator {
    Delete,   // d
    Change,   // c
    Yank,     // y
    Indent,   // >
    Dedent,   // <
}

/// Actions that vim key processing can produce.
#[derive(Debug, Clone)]
pub enum VimAction {
    /// Insert a character at cursor.
    Insert(char),
    /// Move cursor by a motion.
    MoveCursor(Motion),
    /// Execute operator over a range.
    OperatorMotion(Operator, Motion),
    /// Delete a range.
    DeleteRange(usize, usize),
    /// Yank a range to clipboard.
    YankRange(usize, usize),
    /// Paste from clipboard.
    Paste(PastePosition),
    /// Switch mode.
    ChangeMode(VimMode),
    /// Undo.
    Undo,
    /// Nothing (key consumed but no action).
    None,
}

#[derive(Debug, Clone)]
pub enum Motion {
    Left,
    Right,
    WordForward,
    WordBackward,
    WordEnd,
    LineStart,
    LineEnd,
    FirstNonBlank,
    FindChar(char),
    TillChar(char),
    Top,
    Bottom,
}

#[derive(Debug, Clone)]
pub enum PastePosition {
    Before,
    After,
}

impl VimState {
    pub fn new() -> Self {
        VimState {
            mode: VimMode::Normal,
            pending_operator: None,
            count: None,
            register: '"',
            last_search: None,
            visual_anchor: None,
            clipboard: String::new(),
        }
    }

    pub fn effective_count(&self) -> u32 {
        self.count.unwrap_or(1)
    }

    pub fn reset_count(&mut self) {
        self.count = None;
    }

    pub fn is_normal(&self) -> bool { self.mode == VimMode::Normal }
    pub fn is_insert(&self) -> bool { self.mode == VimMode::Insert }
    pub fn is_visual(&self) -> bool { self.mode == VimMode::Visual }
}

impl Default for VimState {
    fn default() -> Self { Self::new() }
}
