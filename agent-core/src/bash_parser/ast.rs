//! Bash AST node types — mirrors the tree-sitter-bash AST.
//!
//! Designed to produce output structurally compatible with tree-sitter-bash
//! for the `parse_for_security()` walker.
#![allow(dead_code)]

/// AST node type names matching tree-sitter-bash conventions.
#[derive(Debug, Clone, PartialEq)]
pub enum NodeType {
    Program,
    SimpleCommand,
    Pipeline,
    List,             // && or ||
    Sequence,         // ;
    Subshell,         // ( ... )
    CommandSubstitution, // $( ... )
    ProcessSubstitution, // <( ... ) or >( ... )
    Redirect,
    HeredocRedirect,
    HereStringRedirect,
    Assignment,
    IfStatement,
    WhileStatement,
    ForStatement,
    CaseStatement,
    FunctionDefinition,
    BraceGroup,
    NegatedCommand,
    TestCommand,
    Word,
    String,           // "..." (double-quoted)
    RawString,        // '...' (single-quoted)
    Expansion,        // ${...}
    SimpleExpansion,  // $VAR
    ArithmeticExpansion, // $(( ... ))
    Concatenation,
    Comment,
    Unknown(String),
}

/// A node in the bash AST.
#[derive(Debug, Clone)]
pub struct BashNode {
    pub node_type: NodeType,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub children: Vec<BashNode>,
}

impl BashNode {
    pub fn new(node_type: NodeType, text: impl Into<String>, start: usize, end: usize) -> Self {
        BashNode {
            node_type,
            text: text.into(),
            start,
            end,
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<BashNode>) -> Self {
        self.children = children;
        self
    }

    /// Check if this node is a specific type.
    pub fn is_type(&self, t: &NodeType) -> bool {
        self.node_type == *t
    }

    /// Get the first child of a given type.
    pub fn child_by_type(&self, t: &NodeType) -> Option<&BashNode> {
        self.children.iter().find(|c| c.is_type(t))
    }

    /// Iterate all descendants depth-first.
    pub fn walk(&self) -> Vec<&BashNode> {
        let mut result = vec![self];
        for child in &self.children {
            result.extend(child.walk());
        }
        result
    }
}

// ─── Extracted command types (output of parse_for_security) ─────────────────

/// A simple command extracted from the AST with trustworthy argv.
#[derive(Debug, Clone)]
pub struct SimpleCommand {
    /// The command name (argv[0]).
    pub program: String,
    /// All arguments (argv[1..]).
    pub args: Vec<String>,
    /// Environment variable assignments before the command.
    pub env_vars: Vec<(String, String)>,
    /// Redirects attached to this command.
    pub redirects: Vec<Redirect>,
}

impl SimpleCommand {
    /// Get the full argv (program + args).
    pub fn argv(&self) -> Vec<&str> {
        let mut v = vec![self.program.as_str()];
        v.extend(self.args.iter().map(|s| s.as_str()));
        v
    }

    /// Get the command prefix (first 1-2 words for permission matching).
    /// e.g., "git status --short" → "git status"
    pub fn prefix(&self, depth: usize) -> String {
        let mut parts = vec![self.program.as_str()];
        for arg in self.args.iter().take(depth.saturating_sub(1)) {
            if arg.starts_with('-') {
                break;
            }
            parts.push(arg.as_str());
        }
        parts.join(" ")
    }
}

/// A redirect attached to a command.
#[derive(Debug, Clone)]
pub struct Redirect {
    pub operator: String, // >, >>, <, 2>, etc.
    pub target: String,
}

/// Result of security analysis.
#[derive(Debug, Clone)]
pub enum SecurityResult {
    /// Command was fully parsed into simple commands with trustworthy argv.
    Simple(Vec<SimpleCommand>),
    /// Command is too complex to analyze safely — prompt user.
    TooComplex(String),
    /// Parser failed or timed out — prompt user (fail-closed).
    ParseUnavailable(String),
}
