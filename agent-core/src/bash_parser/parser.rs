//! Bash parser — quote-aware tokenizer and recursive descent parser.
//!
//! Produces a `BashNode` AST that `parse_for_security()` can walk.
//! Has a 50ms timeout and 50K node budget to resist adversarial input.
#![allow(dead_code)]

use std::time::{Duration, Instant};
use super::ast::{BashNode, NodeType};

const PARSE_TIMEOUT: Duration = Duration::from_millis(50);
const MAX_NODES: usize = 50_000;

/// Parse a bash command string into an AST.
/// Returns None if parsing fails or times out (fail-closed).
pub fn parse_command(input: &str) -> Option<BashNode> {
    let mut state = ParserState::new(input);
    state.parse_program().ok()
}

/// Raw parse that returns the error message on failure.
pub fn parse_command_raw(input: &str) -> Result<BashNode, String> {
    let mut state = ParserState::new(input);
    state.parse_program()
}

struct ParserState<'a> {
    input: &'a str,
    pos: usize,
    start_time: Instant,
    node_count: usize,
}

impl<'a> ParserState<'a> {
    fn new(input: &'a str) -> Self {
        ParserState { input, pos: 0, start_time: Instant::now(), node_count: 0 }
    }

    fn check_limits(&self) -> Result<(), String> {
        if self.start_time.elapsed() > PARSE_TIMEOUT {
            return Err("Parse timeout".into());
        }
        if self.node_count > MAX_NODES {
            return Err("Node budget exceeded".into());
        }
        Ok(())
    }

    fn remaining(&self) -> &str { &self.input[self.pos..] }
    fn peek(&self) -> Option<char> { self.remaining().chars().next() }
    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(' ') | Some('\t')) { self.advance(); }
    }

    fn skip_ws_nl(&mut self) {
        loop {
            match self.peek() {
                Some(' ') | Some('\t') | Some('\n') | Some('\r') => { self.advance(); }
                Some('#') => { while !matches!(self.peek(), Some('\n') | None) { self.advance(); } }
                _ => break,
            }
        }
    }

    fn node(&mut self, t: NodeType, text: &str, start: usize) -> BashNode {
        self.node_count += 1;
        BashNode::new(t, text, start, self.pos)
    }

    // ─── Grammar ────────────────────────────────────────────────────────

    fn parse_program(&mut self) -> Result<BashNode, String> {
        let start = self.pos;
        let mut cmds = Vec::new();
        self.skip_ws_nl();
        while self.pos < self.input.len() {
            self.check_limits()?;
            cmds.push(self.parse_list()?);
            self.skip_ws();
            match self.peek() {
                Some(';') | Some('\n') | Some('&') => { self.advance(); }
                _ => break,
            }
            self.skip_ws_nl();
        }
        if cmds.len() == 1 {
            Ok(cmds.into_iter().next().unwrap())
        } else {
            Ok(self.node(NodeType::Program, &self.input[start..self.pos], start)
                .with_children(cmds))
        }
    }

    fn parse_list(&mut self) -> Result<BashNode, String> {
        self.check_limits()?;
        let start = self.pos;
        let mut left = self.parse_pipeline()?;
        loop {
            self.skip_ws();
            let r = self.remaining();
            if r.starts_with("&&") || r.starts_with("||") {
                self.pos += 2;
                self.skip_ws_nl();
                let right = self.parse_pipeline()?;
                left = self.node(NodeType::List, &self.input[start..self.pos], start)
                    .with_children(vec![left, right]);
            } else { break; }
        }
        Ok(left)
    }

    fn parse_pipeline(&mut self) -> Result<BashNode, String> {
        self.check_limits()?;
        let start = self.pos;
        let mut cmds = vec![self.parse_command()?];
        loop {
            self.skip_ws();
            if self.peek() == Some('|') && !self.remaining().starts_with("||") {
                self.advance();
                self.skip_ws_nl();
                cmds.push(self.parse_command()?);
            } else { break; }
        }
        if cmds.len() == 1 {
            Ok(cmds.into_iter().next().unwrap())
        } else {
            Ok(self.node(NodeType::Pipeline, &self.input[start..self.pos], start)
                .with_children(cmds))
        }
    }

    fn parse_command(&mut self) -> Result<BashNode, String> {
        self.check_limits()?;
        self.skip_ws();
        match self.peek() {
            Some('(') => self.parse_subshell(),
            Some('{') => self.parse_brace_group(),
            Some('!') => {
                let s = self.pos; self.advance(); self.skip_ws();
                let inner = self.parse_command()?;
                Ok(self.node(NodeType::NegatedCommand, &self.input[s..self.pos], s)
                    .with_children(vec![inner]))
            }
            _ => self.parse_simple_command(),
        }
    }

    fn parse_subshell(&mut self) -> Result<BashNode, String> {
        let s = self.pos; self.advance(); self.skip_ws_nl();
        let inner = self.parse_program()?;
        self.skip_ws_nl();
        if self.peek() == Some(')') { self.advance(); }
        Ok(self.node(NodeType::Subshell, &self.input[s..self.pos], s)
            .with_children(vec![inner]))
    }

    fn parse_brace_group(&mut self) -> Result<BashNode, String> {
        let s = self.pos; self.advance(); self.skip_ws_nl();
        let inner = self.parse_program()?;
        self.skip_ws_nl();
        if self.peek() == Some('}') { self.advance(); }
        Ok(self.node(NodeType::BraceGroup, &self.input[s..self.pos], s)
            .with_children(vec![inner]))
    }

    fn parse_simple_command(&mut self) -> Result<BashNode, String> {
        self.check_limits()?;
        let start = self.pos;
        let mut children = Vec::new();

        // Env assignments
        while self.is_assignment() {
            children.push(self.parse_assignment()?);
            self.skip_ws();
        }

        // Words + redirects
        while self.pos < self.input.len() {
            self.skip_ws();
            match self.peek() {
                None | Some(';') | Some('\n') | Some('|') | Some('&')
                | Some(')') | Some('}') | Some('#') => break,
                Some('>') | Some('<') => children.push(self.parse_redirect()?),
                _ => {
                    let w = self.parse_word()?;
                    if w.text.is_empty() { break; }
                    children.push(w);
                }
            }
        }
        Ok(self.node(NodeType::SimpleCommand, &self.input[start..self.pos], start)
            .with_children(children))
    }

    fn is_assignment(&self) -> bool {
        let r = self.remaining();
        if let Some(eq) = r.find('=') {
            let before = &r[..eq];
            !before.is_empty()
                && !before.contains(' ')
                && before.chars().all(|c| c.is_alphanumeric() || c == '_')
                && before.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        } else { false }
    }

    fn parse_assignment(&mut self) -> Result<BashNode, String> {
        let s = self.pos;
        while self.peek() != Some('=') { self.advance(); }
        self.advance(); // =
        let val = self.parse_word()?;
        Ok(self.node(NodeType::Assignment, &self.input[s..self.pos], s).with_children(vec![val]))
    }

    fn parse_word(&mut self) -> Result<BashNode, String> {
        self.check_limits()?;
        let s = self.pos;
        let mut text = String::new();

        loop {
            match self.peek() {
                None | Some(' ') | Some('\t') | Some('\n') | Some(';')
                | Some('|') | Some('&') | Some(')') | Some('}')
                | Some('>') | Some('<') | Some('#') => break,

                Some('\'') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '\'' { self.advance(); break; }
                        text.push(c); self.advance();
                    }
                }
                Some('"') => {
                    self.advance();
                    while let Some(c) = self.peek() {
                        if c == '"' { self.advance(); break; }
                        if c == '\\' { self.advance(); if let Some(e) = self.peek() { text.push(e); self.advance(); } }
                        else { text.push(c); self.advance(); }
                    }
                }
                Some('\\') => { self.advance(); if let Some(c) = self.peek() { text.push(c); self.advance(); } }
                Some('$') => {
                    text.push('$'); self.advance();
                    match self.peek() {
                        Some('{') => { self.consume_balanced('{', '}', &mut text); }
                        Some('(') => {
                            if self.remaining().starts_with("((") {
                                text.push('('); self.advance();
                                self.consume_balanced('(', ')', &mut text);
                            } else {
                                self.consume_balanced('(', ')', &mut text);
                            }
                        }
                        Some(c) if c.is_alphanumeric() || c == '_' => {
                            while matches!(self.peek(), Some(c) if c.is_alphanumeric() || c == '_') {
                                text.push(self.advance().unwrap());
                            }
                        }
                        _ => {}
                    }
                }
                Some(c) => { text.push(c); self.advance(); }
            }
        }
        Ok(self.node(NodeType::Word, &text, s))
    }

    fn consume_balanced(&mut self, open: char, close: char, text: &mut String) {
        text.push(open); self.advance();
        let mut depth = 1;
        while depth > 0 {
            match self.peek() {
                Some(c) if c == open => { depth += 1; text.push(c); self.advance(); }
                Some(c) if c == close => { depth -= 1; if depth > 0 { text.push(c); } self.advance(); }
                Some(c) => { text.push(c); self.advance(); }
                None => break,
            }
        }
    }

    fn parse_redirect(&mut self) -> Result<BashNode, String> {
        let s = self.pos;
        while matches!(self.peek(), Some('>') | Some('<') | Some('&')) { self.advance(); }
        self.skip_ws();
        let target = self.parse_word()?;
        Ok(self.node(NodeType::Redirect, &self.input[s..self.pos], s).with_children(vec![target]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple() {
        let n = parse_command("ls -la").unwrap();
        assert_eq!(n.node_type, NodeType::SimpleCommand);
        assert_eq!(n.children.len(), 2);
    }

    #[test]
    fn test_pipeline() {
        let n = parse_command("cat f | grep x | wc").unwrap();
        assert_eq!(n.node_type, NodeType::Pipeline);
        assert_eq!(n.children.len(), 3);
    }

    #[test]
    fn test_and() {
        let n = parse_command("make && make install").unwrap();
        assert_eq!(n.node_type, NodeType::List);
    }

    #[test]
    fn test_quotes() {
        let n = parse_command("echo 'hello world'").unwrap();
        assert_eq!(n.children.len(), 2);
        assert_eq!(n.children[1].text, "hello world");
    }

    #[test]
    fn test_subshell() {
        let n = parse_command("(echo hi)").unwrap();
        assert_eq!(n.node_type, NodeType::Subshell);
    }

    #[test]
    fn test_sequence() {
        let n = parse_command("echo a; echo b").unwrap();
        assert_eq!(n.node_type, NodeType::Program);
        assert_eq!(n.children.len(), 2);
    }

    #[test]
    fn test_redirect() {
        let n = parse_command("echo hi > f.txt").unwrap();
        assert!(n.children.iter().any(|c| c.node_type == NodeType::Redirect));
    }

    #[test]
    fn test_assignment() {
        let n = parse_command("FOO=bar echo").unwrap();
        assert!(n.children.iter().any(|c| c.node_type == NodeType::Assignment));
    }
}
