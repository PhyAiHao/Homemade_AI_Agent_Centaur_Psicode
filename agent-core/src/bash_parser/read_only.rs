//! Read-only command validation — determines whether a parsed command is safe
//! to auto-approve without user confirmation.
//!
//! The allowlist is intentionally conservative. Commands not explicitly listed
//! here will require user approval.
#![allow(dead_code)]

use super::ast::SimpleCommand;

/// Returns `true` if the command is known to be read-only and safe to
/// auto-approve (no filesystem mutations, no network side-effects).
pub fn is_read_only_command(cmd: &SimpleCommand) -> bool {
    if cmd.program.is_empty() {
        // Bare assignments (FOO=bar) — read-only by themselves
        return cmd.redirects.is_empty();
    }

    // Commands with output redirects are never auto-approved
    for redir in &cmd.redirects {
        let op = redir.operator.trim();
        if op.contains('>') {
            return false;
        }
    }

    let prog = cmd.program.as_str();
    let args: Vec<&str> = cmd.args.iter().map(|s| s.as_str()).collect();

    match prog {
        // ── Git read-only commands ──────────────────────────────────────
        "git" => is_read_only_git(&args),

        // ── GitHub CLI read-only commands ───────────────────────────────
        "gh" => is_read_only_gh(&args),

        // ── General safe (read-only) commands ──────────────────────────
        "ls" | "cat" | "head" | "tail" | "wc" | "sort" | "uniq" | "diff"
        | "file" | "stat" | "du" | "df" | "which" | "type" | "echo"
        | "printf" | "date" | "uname" | "whoami" | "pwd" | "env"
        | "printenv" | "id" | "hostname" | "uptime" => true,

        // Explicit non-match — requires user approval
        _ => false,
    }
}

/// Check if `git <subcommand> [args...]` is read-only.
fn is_read_only_git(args: &[&str]) -> bool {
    if args.is_empty() {
        return false;
    }

    let subcommand = args[0];
    let sub_args = &args[1..];

    match subcommand {
        // Always read-only (no flags change this)
        "diff" | "log" | "show" | "status" | "blame" | "ls-files"
        | "rev-parse" | "rev-list" | "describe" | "cat-file"
        | "for-each-ref" | "grep" => true,

        // Read-only unless specific mutating flags are present
        "branch" => {
            // git branch (list) is safe; -d/-D/-m/-M/-c/-C create/delete/rename
            let dangerous = ["-d", "-D", "--delete", "-m", "-M", "--move", "-c", "-C", "--copy"];
            !has_any_flag(sub_args, &dangerous)
        }

        "tag" => {
            // git tag (list) is safe; -d (delete), -a (annotate/create) are not
            let dangerous = ["-d", "--delete", "-a", "--annotate", "-s", "--sign", "-f"];
            !has_any_flag(sub_args, &dangerous)
        }

        "stash" => {
            // Only `stash list` and `stash show` are read-only
            matches!(sub_args.first(), Some(&"list") | Some(&"show"))
        }

        "remote" => {
            // `git remote` (list) and `git remote -v` are safe;
            // `git remote add/remove/rename/set-url` are not
            if sub_args.is_empty() {
                return true;
            }
            // Allow -v/--verbose as sole argument
            sub_args.iter().all(|a| *a == "-v" || *a == "--verbose")
        }

        "worktree" => {
            // Only `worktree list` is read-only
            matches!(sub_args.first(), Some(&"list"))
        }

        _ => false,
    }
}

/// Check if `gh <subcommand> [args...]` is read-only.
fn is_read_only_gh(args: &[&str]) -> bool {
    if args.is_empty() {
        return false;
    }

    let resource = args[0];
    let action = args.get(1).copied();

    match resource {
        "pr" => matches!(
            action,
            Some("view") | Some("list") | Some("diff") | Some("checks") | Some("status")
        ),
        "issue" => matches!(action, Some("view") | Some("list")),
        "repo" => matches!(action, Some("view")),
        "run" => matches!(action, Some("list") | Some("view")),
        "auth" => matches!(action, Some("status")),
        "release" => matches!(action, Some("list") | Some("view")),
        _ => false,
    }
}

/// Validate that a command's flags are within an allowed set and none are
/// in the dangerous set.
///
/// Returns `true` if the command passes validation.
pub fn validate_flags(
    cmd: &SimpleCommand,
    allowed_flags: &[&str],
    dangerous_flags: &[&str],
) -> bool {
    for arg in &cmd.args {
        if !arg.starts_with('-') {
            continue;
        }
        // Check dangerous first — takes priority
        if dangerous_flags.iter().any(|f| arg == f) {
            return false;
        }
        // If allowed_flags is non-empty, enforce the allowlist
        if !allowed_flags.is_empty() && !allowed_flags.iter().any(|f| arg == f) {
            return false;
        }
    }
    true
}

/// Returns true if any of the flags appear in the args list.
fn has_any_flag(args: &[&str], flags: &[&str]) -> bool {
    args.iter().any(|a| flags.contains(a))
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bash_parser::ast::Redirect;

    fn cmd(program: &str, args: &[&str]) -> SimpleCommand {
        SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env_vars: Vec::new(),
            redirects: Vec::new(),
        }
    }

    fn cmd_with_redirect(program: &str, args: &[&str], op: &str, target: &str) -> SimpleCommand {
        SimpleCommand {
            program: program.to_string(),
            args: args.iter().map(|s| s.to_string()).collect(),
            env_vars: Vec::new(),
            redirects: vec![Redirect {
                operator: op.to_string(),
                target: target.to_string(),
            }],
        }
    }

    // ── General commands ────────────────────────────────────────────────

    #[test]
    fn ls_is_read_only() {
        assert!(is_read_only_command(&cmd("ls", &["-la"])));
    }

    #[test]
    fn cat_is_read_only() {
        assert!(is_read_only_command(&cmd("cat", &["file.txt"])));
    }

    #[test]
    fn echo_is_read_only() {
        assert!(is_read_only_command(&cmd("echo", &["hello"])));
    }

    #[test]
    fn rm_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("rm", &["file.txt"])));
    }

    #[test]
    fn mkdir_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("mkdir", &["new_dir"])));
    }

    #[test]
    fn redirect_output_blocks_read_only() {
        assert!(!is_read_only_command(&cmd_with_redirect(
            "echo", &["hi"], ">", "out.txt"
        )));
    }

    // ── Git commands ────────────────────────────────────────────────────

    #[test]
    fn git_status_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["status"])));
    }

    #[test]
    fn git_diff_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["diff", "--staged"])));
    }

    #[test]
    fn git_log_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["log", "--oneline", "-10"])));
    }

    #[test]
    fn git_branch_list_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["branch"])));
    }

    #[test]
    fn git_branch_delete_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("git", &["branch", "-D", "feature"])));
    }

    #[test]
    fn git_tag_list_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["tag"])));
    }

    #[test]
    fn git_tag_create_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("git", &["tag", "-a", "v1.0"])));
    }

    #[test]
    fn git_stash_list_is_read_only() {
        assert!(is_read_only_command(&cmd("git", &["stash", "list"])));
    }

    #[test]
    fn git_stash_push_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("git", &["stash", "push"])));
    }

    #[test]
    fn git_push_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("git", &["push", "origin", "main"])));
    }

    #[test]
    fn git_commit_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("git", &["commit", "-m", "test"])));
    }

    // ── GitHub CLI commands ─────────────────────────────────────────────

    #[test]
    fn gh_pr_view_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["pr", "view", "123"])));
    }

    #[test]
    fn gh_pr_create_is_not_read_only() {
        assert!(!is_read_only_command(&cmd("gh", &["pr", "create"])));
    }

    #[test]
    fn gh_issue_list_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["issue", "list"])));
    }

    #[test]
    fn gh_repo_view_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["repo", "view"])));
    }

    #[test]
    fn gh_run_view_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["run", "view", "12345"])));
    }

    #[test]
    fn gh_auth_status_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["auth", "status"])));
    }

    #[test]
    fn gh_release_list_is_read_only() {
        assert!(is_read_only_command(&cmd("gh", &["release", "list"])));
    }

    // ── validate_flags ──────────────────────────────────────────────────

    #[test]
    fn validate_flags_passes_allowed() {
        let c = cmd("git", &["branch", "-v", "--list"]);
        assert!(validate_flags(&c, &["-v", "--list", "-a"], &["-D", "-d"]));
    }

    #[test]
    fn validate_flags_rejects_dangerous() {
        let c = cmd("git", &["branch", "-D", "feature"]);
        assert!(!validate_flags(&c, &[], &["-D", "-d"]));
    }

    #[test]
    fn validate_flags_rejects_unlisted() {
        let c = cmd("git", &["branch", "--unknown"]);
        assert!(!validate_flags(&c, &["-v", "--list"], &[]));
    }
}
