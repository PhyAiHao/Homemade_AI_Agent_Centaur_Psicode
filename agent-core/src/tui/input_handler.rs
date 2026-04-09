//! Input handler — manages the text input buffer, cursor, and history navigation.
//!
//! Mirrors `src/hooks/useInputBuffer.ts` and `src/hooks/useHistoryNavigation.ts`.
#![allow(dead_code)]

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use crate::vim::{VimState, VimMode, VimAction, handle_transition, execute_motion};
use crate::vim::types::PastePosition;

/// Manages the input line with cursor movement, editing, and history.
pub struct InputHandler {
    /// Current input text.
    buffer: String,
    /// Cursor position (byte offset).
    cursor: usize,
    /// History of submitted inputs (most recent last).
    history: Vec<String>,
    /// Current position in history (-1 = current input, 0 = most recent).
    history_index: Option<usize>,
    /// Saved current input when browsing history.
    saved_input: String,
    /// Whether vim mode is enabled.
    pub vim_enabled: bool,
    /// Vim editor state (active when vim_enabled is true).
    pub vim_state: VimState,
    /// R4: Paste buffer for bracketed paste detection.
    pub paste_buffer: String,
    /// R10: Ghost text (typeahead) — rendered as dim text after cursor.
    pub ghost_text: Option<String>,
    /// R10: Voice input enabled (stub — no real audio).
    pub voice_enabled: bool,
}

impl InputHandler {
    pub fn new() -> Self {
        InputHandler {
            buffer: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            saved_input: String::new(),
            vim_enabled: false,
            vim_state: VimState::new(),
            paste_buffer: String::new(),
            ghost_text: None,
            voice_enabled: false,
        }
    }

    /// Toggle vim mode on/off.
    pub fn toggle_vim(&mut self) {
        self.vim_enabled = !self.vim_enabled;
        if self.vim_enabled {
            self.vim_state = VimState::new(); // start in Normal mode
        }
    }

    /// Get the current vim mode label for the status line.
    pub fn vim_mode_label(&self) -> &str {
        if !self.vim_enabled {
            return "";
        }
        match self.vim_state.mode {
            VimMode::Normal => "NORMAL",
            VimMode::Insert => "INSERT",
            VimMode::Visual => "VISUAL",
            VimMode::Replace => "REPLACE",
        }
    }

    /// Handle a key event, updating the buffer and cursor.
    pub fn handle_key(&mut self, key: KeyEvent) {
        // Vim mode dispatch
        if self.vim_enabled {
            self.handle_vim_key(key);
            return;
        }

        match (key.modifiers, key.code) {
            // ── Ctrl combos (must be before wildcard matches) ───────
            (m, KeyCode::Char('a')) if m.contains(KeyModifiers::CONTROL) => {
                self.cursor = 0;
            }
            (m, KeyCode::Char('e')) if m.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.buffer.len();
            }
            (m, KeyCode::Char('u')) if m.contains(KeyModifiers::CONTROL) => {
                self.buffer.drain(..self.cursor);
                self.cursor = 0;
            }
            (m, KeyCode::Char('k')) if m.contains(KeyModifiers::CONTROL) => {
                self.buffer.truncate(self.cursor);
            }
            (m, KeyCode::Char('w')) if m.contains(KeyModifiers::CONTROL) => {
                let boundary = self.prev_word_boundary();
                self.buffer.drain(boundary..self.cursor);
                self.cursor = boundary;
            }
            (m, KeyCode::Char('b')) if m.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.prev_word_boundary();
            }
            (m, KeyCode::Char('f')) if m.contains(KeyModifiers::CONTROL) => {
                self.cursor = self.next_word_boundary();
            }

            // ── Alt + arrow (word movement, before plain arrow) ────
            (m, KeyCode::Left) if m.contains(KeyModifiers::ALT) => {
                self.cursor = self.prev_word_boundary();
            }
            (m, KeyCode::Right) if m.contains(KeyModifiers::ALT) => {
                self.cursor = self.next_word_boundary();
            }

            // ── Character input (accept any modifier combo with a char) ──
            (_, KeyCode::Char(c)) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
                self.history_index = None;
            }

            // ── Backspace ──────────────────────────────────────────
            (_, KeyCode::Backspace) => {
                if self.cursor > 0 {
                    let prev = self.buffer[..self.cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                    self.buffer.drain(prev..self.cursor);
                    self.cursor = prev;
                }
            }

            // ── Delete ─────────────────────────────────────────────
            (_, KeyCode::Delete) => {
                if self.cursor < self.buffer.len() {
                    let next = self.buffer[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.buffer.len());
                    self.buffer.drain(self.cursor..next);
                }
            }

            // ── Arrow keys ─────────────────────────────────────────
            (_, KeyCode::Left) => {
                if self.cursor > 0 {
                    self.cursor = self.buffer[..self.cursor]
                        .char_indices()
                        .last()
                        .map(|(i, _)| i)
                        .unwrap_or(0);
                }
            }
            (_, KeyCode::Right) => {
                if self.cursor < self.buffer.len() {
                    self.cursor = self.buffer[self.cursor..]
                        .char_indices()
                        .nth(1)
                        .map(|(i, _)| self.cursor + i)
                        .unwrap_or(self.buffer.len());
                }
            }
            (_, KeyCode::Home) => self.cursor = 0,
            (_, KeyCode::End) => self.cursor = self.buffer.len(),

            // ── History navigation ─────────────────────────────────
            (_, KeyCode::Up) => self.history_prev(),
            (_, KeyCode::Down) => self.history_next(),

            _ => {}
        }
    }

    /// Submit the current input. Returns the text and resets the buffer.
    pub fn submit(&mut self) -> String {
        let text = std::mem::take(&mut self.buffer);
        self.cursor = 0;
        self.history_index = None;
        self.saved_input.clear();

        if !text.trim().is_empty() {
            self.history.push(text.clone());
        }

        text
    }

    /// Get the current display text.
    pub fn display_text(&self) -> &str {
        &self.buffer
    }

    /// Get the cursor position (in characters, for display).
    pub fn cursor_position(&self) -> usize {
        self.buffer[..self.cursor].chars().count()
    }

    /// Navigate to the previous history entry.
    fn history_prev(&mut self) {
        if self.history.is_empty() {
            return;
        }

        match self.history_index {
            None => {
                // Save current input and go to most recent history
                self.saved_input = self.buffer.clone();
                self.history_index = Some(self.history.len() - 1);
            }
            Some(idx) if idx > 0 => {
                self.history_index = Some(idx - 1);
            }
            _ => return,
        }

        if let Some(idx) = self.history_index {
            self.buffer = self.history[idx].clone();
            self.cursor = self.buffer.len();
        }
    }

    /// Navigate to the next history entry (or back to current input).
    fn history_next(&mut self) {
        match self.history_index {
            Some(idx) if idx + 1 < self.history.len() => {
                self.history_index = Some(idx + 1);
                self.buffer = self.history[idx + 1].clone();
                self.cursor = self.buffer.len();
            }
            Some(_) => {
                // Back to current input
                self.history_index = None;
                self.buffer = std::mem::take(&mut self.saved_input);
                self.cursor = self.buffer.len();
            }
            None => {}
        }
    }

    /// Find the previous word boundary (char-boundary safe).
    fn prev_word_boundary(&self) -> usize {
        let s = &self.buffer[..self.cursor];
        // Skip trailing whitespace
        let trimmed = s.trim_end();
        if trimmed.is_empty() {
            return 0;
        }
        // Find the last whitespace before the last word
        match trimmed.rfind(|c: char| c.is_whitespace()) {
            Some(pos) => {
                // Advance past the whitespace char to get to the start of the word
                let mut boundary = pos;
                if let Some((i, _)) = trimmed[pos..].char_indices().nth(1) {
                    boundary = pos + i;
                }
                boundary
            }
            None => 0,
        }
    }

    /// Find the next word boundary (char-boundary safe).
    fn next_word_boundary(&self) -> usize {
        let s = &self.buffer[self.cursor..];
        // Skip current word (non-whitespace)
        let after_word = s.trim_start_matches(|c: char| !c.is_whitespace());
        // Skip whitespace
        let after_ws = after_word.trim_start();
        self.buffer.len() - after_ws.len()
    }

    // ─── R3: History search ──────────────────────────────────────────────

    /// Search history for entries matching the query (case-insensitive).
    /// Returns all history items if query is empty.
    pub fn search_history(&self, query: &str) -> Vec<String> {
        if query.is_empty() {
            return self.history.clone();
        }
        let q = query.to_lowercase();
        self.history.iter()
            .filter(|entry| entry.to_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    /// Set the buffer content directly (used by history search injection).
    pub fn set_buffer(&mut self, text: String) {
        self.cursor = text.len();
        self.buffer = text;
        self.history_index = None;
    }

    // ─── R4: Paste handling ────────────────────────────────────────────

    /// Handle pasted text from bracketed paste.
    /// If text > 800 chars, truncates and adds a reference note.
    /// Otherwise inserts directly into the buffer.
    pub fn handle_paste(&mut self, text: &str) {
        self.paste_buffer = text.to_string();
        let insert_text = if text.len() > 800 {
            let truncated = &text[..800];
            // Find a safe char boundary for truncation
            let safe_end = truncated.char_indices()
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(800);
            format!("{}... [Pasted {} chars]", &text[..safe_end], text.len())
        } else {
            text.to_string()
        };
        self.buffer.insert_str(self.cursor, &insert_text);
        self.cursor += insert_text.len();
    }

    // ─── R10: Ghost text (typeahead) ───────────────────────────────────

    /// Set ghost text (rendered as dim text after cursor).
    pub fn set_ghost_text(&mut self, text: Option<String>) {
        self.ghost_text = text;
    }

    /// Whether ghost text is currently set.
    pub fn has_ghost_text(&self) -> bool {
        self.ghost_text.is_some()
    }

    /// Accept ghost text — inserts it into the buffer at the cursor position.
    pub fn accept_ghost_text(&mut self) {
        if let Some(ghost) = self.ghost_text.take() {
            self.buffer.insert_str(self.cursor, &ghost);
            self.cursor += ghost.len();
        }
    }

    /// Get the current ghost text, if any.
    pub fn ghost_text_str(&self) -> Option<&str> {
        self.ghost_text.as_deref()
    }

    // ─── R10: Voice stub ───────────────────────────────────────────────

    /// Check if voice input is enabled.
    pub fn is_voice_enabled(&self) -> bool {
        self.voice_enabled
    }

    /// Enable or disable voice input (stub — no real audio).
    pub fn set_voice_enabled(&mut self, enabled: bool) {
        self.voice_enabled = enabled;
    }

    /// Set history from loaded entries (for session resume).
    pub fn load_history(&mut self, entries: Vec<String>) {
        self.history = entries;
    }

    // ─── Vim mode key handling ──────────────────────────────────────────

    /// Process a key event through the vim state machine.
    fn handle_vim_key(&mut self, key: KeyEvent) {
        let action = handle_transition(&mut self.vim_state, key, &self.buffer, self.cursor);
        self.apply_vim_action(action);
    }

    /// Apply a VimAction to the buffer/cursor.
    fn apply_vim_action(&mut self, action: VimAction) {
        match action {
            VimAction::Insert(c) => {
                self.buffer.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            VimAction::MoveCursor(motion) => {
                self.cursor = execute_motion(&self.buffer, self.cursor, &motion, 1);
            }
            VimAction::DeleteRange(start, end) => {
                let s = start.min(self.buffer.len());
                let e = end.min(self.buffer.len());
                if s < e {
                    self.vim_state.clipboard = self.buffer[s..e].to_string();
                    self.buffer.drain(s..e);
                    self.cursor = s.min(self.buffer.len());
                }
            }
            VimAction::YankRange(start, end) => {
                let s = start.min(self.buffer.len());
                let e = end.min(self.buffer.len());
                if s < e {
                    self.vim_state.clipboard = self.buffer[s..e].to_string();
                }
            }
            VimAction::Paste(pos) => {
                let text = self.vim_state.clipboard.clone();
                if !text.is_empty() {
                    let insert_at = match pos {
                        PastePosition::Before => self.cursor,
                        PastePosition::After => {
                            // Move past current char
                            self.buffer[self.cursor..]
                                .char_indices()
                                .nth(1)
                                .map(|(i, _)| self.cursor + i)
                                .unwrap_or(self.buffer.len())
                        }
                    };
                    self.buffer.insert_str(insert_at, &text);
                    self.cursor = insert_at + text.len().saturating_sub(1);
                }
            }
            VimAction::OperatorMotion(op, motion) => {
                let target = execute_motion(&self.buffer, self.cursor, &motion, 1);
                let (start, end) = if target < self.cursor {
                    (target, self.cursor)
                } else {
                    (self.cursor, target)
                };
                match op {
                    crate::vim::types::Operator::Delete | crate::vim::types::Operator::Change => {
                        self.apply_vim_action(VimAction::DeleteRange(start, end));
                        if matches!(op, crate::vim::types::Operator::Change) {
                            self.vim_state.mode = VimMode::Insert;
                        }
                    }
                    crate::vim::types::Operator::Yank => {
                        self.apply_vim_action(VimAction::YankRange(start, end));
                    }
                    _ => {}
                }
            }
            VimAction::ChangeMode(mode) => {
                self.vim_state.mode = mode;
            }
            VimAction::Undo => {
                // Simple undo: clear buffer (no undo stack yet)
            }
            VimAction::None => {}
        }
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn test_basic_typing() {
        let mut handler = InputHandler::new();
        handler.handle_key(key(KeyCode::Char('h')));
        handler.handle_key(key(KeyCode::Char('i')));
        assert_eq!(handler.display_text(), "hi");
        assert_eq!(handler.cursor_position(), 2);
    }

    #[test]
    fn test_backspace() {
        let mut handler = InputHandler::new();
        handler.handle_key(key(KeyCode::Char('a')));
        handler.handle_key(key(KeyCode::Char('b')));
        handler.handle_key(key(KeyCode::Backspace));
        assert_eq!(handler.display_text(), "a");
    }

    #[test]
    fn test_submit_and_history() {
        let mut handler = InputHandler::new();
        handler.handle_key(key(KeyCode::Char('x')));
        let text = handler.submit();
        assert_eq!(text, "x");
        assert_eq!(handler.display_text(), "");

        // History navigation
        handler.handle_key(key(KeyCode::Up));
        assert_eq!(handler.display_text(), "x");

        handler.handle_key(key(KeyCode::Down));
        assert_eq!(handler.display_text(), "");
    }

    #[test]
    fn test_cursor_movement() {
        let mut handler = InputHandler::new();
        handler.handle_key(key(KeyCode::Char('a')));
        handler.handle_key(key(KeyCode::Char('b')));
        handler.handle_key(key(KeyCode::Char('c')));
        handler.handle_key(key(KeyCode::Left));
        handler.handle_key(key(KeyCode::Left));
        assert_eq!(handler.cursor_position(), 1);

        handler.handle_key(key(KeyCode::Char('X')));
        assert_eq!(handler.display_text(), "aXbc");
    }

    #[test]
    fn test_kill_line() {
        let mut handler = InputHandler::new();
        for c in "hello world".chars() {
            handler.handle_key(key(KeyCode::Char(c)));
        }
        handler.handle_key(ctrl('u'));
        assert_eq!(handler.display_text(), "");
    }
}
