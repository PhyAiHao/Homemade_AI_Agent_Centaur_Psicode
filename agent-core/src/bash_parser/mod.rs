//! Bash AST parser — security analysis of bash commands before execution.
//! Mirrors src/utils/bash/ (9 files): ast.ts, parser.ts, bashPipeCommand.ts, etc.
#![allow(dead_code)]

pub mod ast;
pub mod parser;
pub mod read_only;
pub mod security;

// Re-export key types for ergonomic use from other modules.
