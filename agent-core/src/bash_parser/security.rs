//! Security analysis of bash commands via AST walking.
//!
//! Provides `parse_for_security()` which parses a bash string into an AST,
//! walks it with a fail-closed allowlist, and extracts `SimpleCommand` structs
//! with trustworthy argv. Any construct that could hide intent (subshells,
//! command substitution, expansions, loops, functions, etc.) causes the result
//! to be `TooComplex`, forcing a user prompt.
#![allow(dead_code)]

use regex::Regex;
use std::sync::LazyLock;

use super::ast::{
    BashNode, NodeType, Redirect, SecurityResult,
    SimpleCommand as AstSimpleCommand,
};
use super::parser::parse_command;

// ─── Regex patterns (compiled once) ─────────────────────────────────────────

static RE_CMD_SUBST: LazyLock<Regex> = LazyLock::new(|| {
    // $( , ${ , $[ , backticks
    Regex::new(r"(?:\$\(|\$\{|\$\[|`)").unwrap()
});

static RE_PROC_SUBST: LazyLock<Regex> = LazyLock::new(|| {
    // <( or >(
    Regex::new(r"[<>]\(").unwrap()
});

static RE_ZSH_DYNAMIC_DIR: LazyLock<Regex> = LazyLock::new(|| {
    // ~[ — zsh dynamic named directory
    Regex::new(r"~\[").unwrap()
});

// ─── Public API ─────────────────────────────────────────────────────────────

/// Main entry point: parse a bash command and determine if it is safe to
/// auto-approve, too complex for automated reasoning, or unparseable.
pub fn parse_for_security(input: &str) -> SecurityResult {
    // ── Pre-checks ──────────────────────────────────────────────────────
    if has_dangerous_chars(input) {
        return SecurityResult::TooComplex(
            "Input contains dangerous characters (control chars, null bytes, or unicode whitespace)"
                .into(),
        );
    }

    // Check for backslash-escaped whitespace that could confuse simple parsing
    if input.contains("\\\n") || input.contains("\\\r") {
        // Line continuations are actually fine in bash, but we flag them
        // here as TooComplex since they can span across visual lines
        // and hide intent.
        return SecurityResult::TooComplex(
            "Input contains backslash-escaped newlines (line continuations)".into(),
        );
    }

    // ── Parse ───────────────────────────────────────────────────────────
    let ast = match parse_command(input) {
        Some(node) => node,
        None => {
            return SecurityResult::ParseUnavailable(
                "Parser failed or timed out".into(),
            );
        }
    };

    // ── Walk AST (fail-closed allowlist) ────────────────────────────────
    if let Err(reason) = check_allowed(&ast) {
        return SecurityResult::TooComplex(reason);
    }

    // ── Extract SimpleCommand[] from allowed nodes ──────────────────────
    let commands = extract_commands(&ast);

    if commands.is_empty() {
        // e.g. bare assignments with no command — still "simple"
        return SecurityResult::Simple(commands);
    }

    SecurityResult::Simple(commands)
}

/// Check whether the input contains dangerous substitution patterns at the
/// string level (before parsing). This catches cases where the parser might
/// not model them as distinct node types.
pub fn has_dangerous_substitution(input: &str) -> bool {
    RE_CMD_SUBST.is_match(input)
        || RE_PROC_SUBST.is_match(input)
        || RE_ZSH_DYNAMIC_DIR.is_match(input)
}

/// Check whether the input contains dangerous characters that could bypass
/// parsing or hide intent.
pub fn has_dangerous_chars(input: &str) -> bool {
    for ch in input.chars() {
        // Control characters (0x00-0x1F) except \n (0x0A), \r (0x0D), \t (0x09)
        if ch as u32 <= 0x1F && ch != '\n' && ch != '\r' && ch != '\t' {
            return true;
        }
        // Null bytes
        if ch == '\0' {
            return true;
        }
        // Unicode whitespace (non-ASCII spaces that could bypass parsing)
        // These are all the Unicode "space separator" characters beyond ASCII space
        if matches!(ch,
            '\u{00A0}'  // NO-BREAK SPACE
            | '\u{1680}'  // OGHAM SPACE MARK
            | '\u{2000}'..='\u{200B}' // EN QUAD through ZERO WIDTH SPACE
            | '\u{2028}'  // LINE SEPARATOR
            | '\u{2029}'  // PARAGRAPH SEPARATOR
            | '\u{202F}'  // NARROW NO-BREAK SPACE
            | '\u{205F}'  // MEDIUM MATHEMATICAL SPACE
            | '\u{2060}'  // WORD JOINER
            | '\u{3000}'  // IDEOGRAPHIC SPACE
            | '\u{FEFF}'  // ZERO WIDTH NO-BREAK SPACE (BOM)
        ) {
            return true;
        }
    }
    false
}

// ─── Destructive command detection (legacy API) ─────────────────────────────

/// Reason a command was classified as destructive.
#[derive(Debug, Clone, PartialEq)]
pub enum DestructiveReason {
    RmRecursive,
    ForceOverwrite,
    DiskWipe,
    ForkBomb,
    NetworkAttack,
    PrivilegeEscalation,
    Other(String),
}

/// Returns `Some(reason)` if the command is considered destructive.
///
/// Works on both parsed and raw input. If the parser succeeds, checks each
/// extracted `SimpleCommand`; otherwise falls back to string heuristics.
pub fn is_destructive(cmd: &str) -> Option<DestructiveReason> {
    // Try AST-based approach first
    match parse_for_security(cmd) {
        SecurityResult::Simple(commands) => {
            for c in &commands {
                if let Some(reason) = check_command_destructive(c) {
                    return Some(reason);
                }
            }
            None
        }
        // If we can't parse or it's too complex, fall back to string heuristics
        _ => check_raw_destructive(cmd),
    }
}

/// Check a single extracted SimpleCommand for destructive patterns.
fn check_command_destructive(cmd: &AstSimpleCommand) -> Option<DestructiveReason> {
    let prog = cmd.program.as_str();
    let args = &cmd.args;
    let args_joined = args.join(" ");

    match prog {
        "rm" => {
            if args.iter().any(|a| a.starts_with('-') && a.contains('r')) {
                return Some(DestructiveReason::RmRecursive);
            }
        }
        "dd" => {
            if args_joined.contains("if=/dev/") || args_joined.contains("of=/dev/") {
                return Some(DestructiveReason::DiskWipe);
            }
        }
        "mkfs" | "mkfs.ext4" | "mkfs.xfs" | "mkfs.btrfs" | "mkfs.vfat" => {
            return Some(DestructiveReason::DiskWipe);
        }
        "fdisk" | "parted" | "gdisk" | "sgdisk" => {
            return Some(DestructiveReason::DiskWipe);
        }
        "chmod" => {
            if args_joined.contains("777") && args.iter().any(|a| a == "-R" || a == "--recursive")
            {
                return Some(DestructiveReason::ForceOverwrite);
            }
        }
        "sudo" | "su" | "doas" => {
            return Some(DestructiveReason::PrivilegeEscalation);
        }
        ":()" => {
            return Some(DestructiveReason::ForkBomb);
        }
        "shred" => {
            return Some(DestructiveReason::ForceOverwrite);
        }
        _ => {}
    }

    // Redirect to device file
    for redir in &cmd.redirects {
        if redir.target.starts_with("/dev/sd")
            || redir.target.starts_with("/dev/nvme")
            || redir.target.starts_with("/dev/vd")
        {
            return Some(DestructiveReason::DiskWipe);
        }
    }

    None
}

/// String-level heuristic fallback for unparseable commands.
fn check_raw_destructive(cmd: &str) -> Option<DestructiveReason> {
    let cmd = cmd.trim();

    if cmd.contains("rm ") && cmd.contains("-r") {
        return Some(DestructiveReason::RmRecursive);
    }
    if cmd.starts_with("dd ") && (cmd.contains("of=/dev/") || cmd.contains("if=/dev/")) {
        return Some(DestructiveReason::DiskWipe);
    }
    if cmd.starts_with("mkfs") || cmd.starts_with("fdisk") || cmd.starts_with("parted") {
        return Some(DestructiveReason::DiskWipe);
    }
    if cmd.starts_with("sudo ") || cmd.starts_with("su ") {
        return Some(DestructiveReason::PrivilegeEscalation);
    }
    if cmd.contains(":(){ :|:& };:") {
        return Some(DestructiveReason::ForkBomb);
    }

    None
}

// ─── AST walking ────────────────────────────────────────────────────────────

/// Allowlist check: only these node types are permitted in "simple" commands.
/// Everything else triggers `TooComplex`.
fn check_allowed(node: &BashNode) -> Result<(), String> {
    match &node.node_type {
        // Allowed node types
        NodeType::Program
        | NodeType::SimpleCommand
        | NodeType::Pipeline
        | NodeType::List
        | NodeType::Sequence
        | NodeType::Word
        | NodeType::String
        | NodeType::RawString
        | NodeType::Assignment
        | NodeType::Redirect
        | NodeType::HeredocRedirect
        | NodeType::HereStringRedirect
        | NodeType::Concatenation
        | NodeType::Comment
        | NodeType::NegatedCommand
        | NodeType::SimpleExpansion => {}

        // Explicitly disallowed — too complex for automated analysis
        NodeType::Subshell => {
            return Err("Subshell detected — cannot verify command safety".into());
        }
        NodeType::CommandSubstitution => {
            return Err("Command substitution $() detected — cannot verify command safety".into());
        }
        NodeType::ProcessSubstitution => {
            return Err("Process substitution <() or >() detected — cannot verify command safety".into());
        }
        NodeType::BraceGroup => {
            return Err("Brace group { } detected — cannot verify command safety".into());
        }
        NodeType::Expansion => {
            return Err("Parameter expansion ${} detected — cannot verify command safety".into());
        }
        NodeType::ArithmeticExpansion => {
            return Err("Arithmetic expansion $(()) detected — cannot verify command safety".into());
        }
        NodeType::ForStatement => {
            return Err("For loop detected — cannot verify command safety".into());
        }
        NodeType::WhileStatement => {
            return Err("While loop detected — cannot verify command safety".into());
        }
        NodeType::IfStatement => {
            return Err("If statement detected — cannot verify command safety".into());
        }
        NodeType::CaseStatement => {
            return Err("Case statement detected — cannot verify command safety".into());
        }
        NodeType::FunctionDefinition => {
            return Err("Function definition detected — cannot verify command safety".into());
        }
        NodeType::TestCommand => {
            return Err("Test command detected — cannot verify command safety".into());
        }
        NodeType::Unknown(name) => {
            return Err(format!("Unknown node type '{name}' — cannot verify command safety"));
        }
    }

    // Recurse into children
    for child in &node.children {
        check_allowed(child)?;
    }

    Ok(())
}

/// Extract `SimpleCommand` structs from the (already validated) AST.
fn extract_commands(node: &BashNode) -> Vec<AstSimpleCommand> {
    let mut result = Vec::new();
    extract_commands_inner(node, &mut result);
    result
}

fn extract_commands_inner(node: &BashNode, out: &mut Vec<AstSimpleCommand>) {
    match &node.node_type {
        NodeType::SimpleCommand => {
            if let Some(cmd) = build_simple_command(node) {
                out.push(cmd);
            }
        }
        // Recurse into compound structures
        NodeType::Program
        | NodeType::Pipeline
        | NodeType::List
        | NodeType::Sequence
        | NodeType::NegatedCommand => {
            for child in &node.children {
                extract_commands_inner(child, out);
            }
        }
        // Leaf nodes — nothing to extract
        _ => {}
    }
}

/// Build a `SimpleCommand` from a `SimpleCommand` AST node.
fn build_simple_command(node: &BashNode) -> Option<AstSimpleCommand> {
    debug_assert_eq!(node.node_type, NodeType::SimpleCommand);

    let mut env_vars = Vec::new();
    let mut words = Vec::new();
    let mut redirects = Vec::new();

    for child in &node.children {
        match &child.node_type {
            NodeType::Assignment => {
                // Assignment text is "KEY=value", child[0] is the value word
                let text = &child.text;
                if let Some(eq_pos) = text.find('=') {
                    let key = text[..eq_pos].to_string();
                    let val = if !child.children.is_empty() {
                        child.children[0].text.clone()
                    } else {
                        text[eq_pos + 1..].to_string()
                    };
                    env_vars.push((key, val));
                }
            }
            NodeType::Redirect | NodeType::HeredocRedirect | NodeType::HereStringRedirect => {
                let target = if !child.children.is_empty() {
                    child.children[0].text.clone()
                } else {
                    String::new()
                };
                // Extract operator from the text before the target
                let op = child.text.trim().to_string();
                redirects.push(Redirect {
                    operator: op,
                    target,
                });
            }
            NodeType::Word | NodeType::String | NodeType::RawString | NodeType::Concatenation => {
                words.push(child.text.clone());
            }
            _ => {}
        }
    }

    if words.is_empty() {
        // Bare assignments with no command
        if !env_vars.is_empty() {
            return Some(AstSimpleCommand {
                program: String::new(),
                args: Vec::new(),
                env_vars,
                redirects,
            });
        }
        return None;
    }

    let program = words.remove(0);
    Some(AstSimpleCommand {
        program,
        args: words,
        env_vars,
        redirects,
    })
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_command_parses() {
        match parse_for_security("ls -la") {
            SecurityResult::Simple(cmds) => {
                assert_eq!(cmds.len(), 1);
                assert_eq!(cmds[0].program, "ls");
                assert_eq!(cmds[0].args, vec!["-la"]);
            }
            other => panic!("Expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn pipeline_parses() {
        match parse_for_security("cat file.txt | grep foo | wc -l") {
            SecurityResult::Simple(cmds) => {
                assert_eq!(cmds.len(), 3);
                assert_eq!(cmds[0].program, "cat");
                assert_eq!(cmds[1].program, "grep");
                assert_eq!(cmds[2].program, "wc");
            }
            other => panic!("Expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn and_list_parses() {
        match parse_for_security("make && make install") {
            SecurityResult::Simple(cmds) => {
                assert_eq!(cmds.len(), 2);
                assert_eq!(cmds[0].program, "make");
                assert_eq!(cmds[1].program, "make");
                assert_eq!(cmds[1].args, vec!["install"]);
            }
            other => panic!("Expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn subshell_is_too_complex() {
        match parse_for_security("(echo hi)") {
            SecurityResult::TooComplex(reason) => {
                assert!(reason.contains("Subshell"), "reason: {reason}");
            }
            other => panic!("Expected TooComplex, got {other:?}"),
        }
    }

    #[test]
    fn brace_group_is_too_complex() {
        match parse_for_security("{ echo hi; }") {
            SecurityResult::TooComplex(reason) => {
                assert!(reason.contains("Brace group"), "reason: {reason}");
            }
            other => panic!("Expected TooComplex, got {other:?}"),
        }
    }

    #[test]
    fn rm_rf_is_destructive() {
        assert!(is_destructive("rm -rf /tmp/foo").is_some());
    }

    #[test]
    fn ls_is_safe() {
        assert!(is_destructive("ls -la").is_none());
    }

    #[test]
    fn dd_disk_wipe() {
        assert!(is_destructive("dd if=/dev/zero of=/dev/sda").is_some());
    }

    #[test]
    fn sudo_blocked() {
        assert!(is_destructive("sudo rm foo").is_some());
    }

    #[test]
    fn dangerous_chars_null() {
        assert!(has_dangerous_chars("echo \x00hello"));
    }

    #[test]
    fn dangerous_chars_control() {
        assert!(has_dangerous_chars("echo \x01hello"));
    }

    #[test]
    fn dangerous_chars_unicode_space() {
        // NO-BREAK SPACE
        assert!(has_dangerous_chars("echo\u{00A0}hello"));
    }

    #[test]
    fn safe_chars() {
        assert!(!has_dangerous_chars("echo hello world\n"));
    }

    #[test]
    fn dangerous_substitution_dollar_paren() {
        assert!(has_dangerous_substitution("echo $(whoami)"));
    }

    #[test]
    fn dangerous_substitution_backtick() {
        assert!(has_dangerous_substitution("echo `whoami`"));
    }

    #[test]
    fn no_dangerous_substitution() {
        assert!(!has_dangerous_substitution("echo hello world"));
    }

    #[test]
    fn env_vars_extracted() {
        match parse_for_security("FOO=bar echo test") {
            SecurityResult::Simple(cmds) => {
                assert_eq!(cmds.len(), 1);
                assert_eq!(cmds[0].env_vars.len(), 1);
                assert_eq!(cmds[0].env_vars[0].0, "FOO");
                assert_eq!(cmds[0].program, "echo");
            }
            other => panic!("Expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn redirect_extracted() {
        match parse_for_security("echo hi > out.txt") {
            SecurityResult::Simple(cmds) => {
                assert_eq!(cmds.len(), 1);
                assert!(!cmds[0].redirects.is_empty());
                assert_eq!(cmds[0].redirects[0].target, "out.txt");
            }
            other => panic!("Expected Simple, got {other:?}"),
        }
    }

    #[test]
    fn backslash_newline_is_too_complex() {
        match parse_for_security("echo \\\nhello") {
            SecurityResult::TooComplex(reason) => {
                assert!(
                    reason.contains("backslash-escaped newlines"),
                    "reason: {reason}"
                );
            }
            other => panic!("Expected TooComplex, got {other:?}"),
        }
    }
}
