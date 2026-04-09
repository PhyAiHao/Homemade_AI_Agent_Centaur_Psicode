//! Git operations via git2 (libgit2 bindings).
//!
//! Mirrors `src/utils/git.ts` and `src/utils/gitDiff.ts`.
//! Uses native git2 instead of shelling out for performance and safety.
#![allow(dead_code)]

use anyhow::{Context, Result};
use git2::{
    BranchType, Repository,
    StatusOptions, StatusShow,
};
use std::path::{Path, PathBuf};

// ─── Repository Discovery ───────────────────────────────────────────────────

/// Find the git root directory by walking up from `start`.
pub fn find_git_root(start: &Path) -> Option<PathBuf> {
    let mut current = start.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

/// Open a git repository at the given path.
pub fn open_repo(path: &Path) -> Result<Repository> {
    Repository::discover(path)
        .with_context(|| format!("Not a git repository: {}", path.display()))
}

/// Check if a path is inside a git repository.
pub fn is_git_repo(path: &Path) -> bool {
    Repository::discover(path).is_ok()
}

// ─── Branch Operations ──────────────────────────────────────────────────────

/// Get the current branch name (or "HEAD" if detached).
pub fn current_branch(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to read HEAD")?;
    if head.is_branch() {
        Ok(head.shorthand().unwrap_or("HEAD").to_string())
    } else {
        // Detached HEAD — return short SHA
        let oid = head.target().context("HEAD has no target")?;
        Ok(format!("{:.8}", oid))
    }
}

/// Get the default branch name (main, master, or first remote branch).
pub fn default_branch(repo: &Repository) -> Result<String> {
    // Try common names first
    for name in &["main", "master"] {
        if repo.find_branch(name, BranchType::Local).is_ok() {
            return Ok(name.to_string());
        }
        let remote_name = format!("origin/{name}");
        if repo.find_branch(&remote_name, BranchType::Remote).is_ok() {
            return Ok(name.to_string());
        }
    }
    // Fallback: first local branch
    let branches = repo.branches(Some(BranchType::Local))?;
    for (branch, _) in branches.flatten() {
        if let Some(name) = branch.name()? {
            return Ok(name.to_string());
        }
    }
    Ok("main".to_string())
}

/// Get the current HEAD commit SHA.
pub fn head_sha(repo: &Repository) -> Result<String> {
    let head = repo.head().context("Failed to read HEAD")?;
    let oid = head.target().context("HEAD has no target")?;
    Ok(oid.to_string())
}

/// Check if the current branch has a remote tracking branch.
pub fn has_remote_tracking(repo: &Repository) -> bool {
    if let Ok(head) = repo.head() {
        if let Some(name) = head.shorthand() {
            if let Ok(branch) = repo.find_branch(name, BranchType::Local) {
                return branch.upstream().is_ok();
            }
        }
    }
    false
}

/// Check if there are unpushed commits.
pub fn has_unpushed_commits(repo: &Repository) -> Result<bool> {
    let head = repo.head()?;
    let branch_name = head.shorthand().unwrap_or("HEAD");

    let local_branch = repo.find_branch(branch_name, BranchType::Local)?;
    let upstream = match local_branch.upstream() {
        Ok(u) => u,
        Err(_) => return Ok(false), // No upstream — can't be unpushed
    };

    let local_oid = head.target().context("No local target")?;
    let remote_oid = upstream.get().target().context("No remote target")?;

    Ok(local_oid != remote_oid)
}

// ─── Status ─────────────────────────────────────────────────────────────────

/// File status entry.
#[derive(Debug, Clone)]
pub struct FileStatus {
    pub path: String,
    pub status: FileStatusType,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FileStatusType {
    New,
    Modified,
    Deleted,
    Renamed,
    Typechange,
    Untracked,
    Conflicted,
}

/// Get the working tree status (tracked + untracked changes).
pub fn status(repo: &Repository) -> Result<Vec<FileStatus>> {
    let mut opts = StatusOptions::new();
    opts.include_untracked(true)
        .recurse_untracked_dirs(false)
        .show(StatusShow::IndexAndWorkdir);

    let statuses = repo.statuses(Some(&mut opts))?;
    let mut result = Vec::new();

    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("?").to_string();
        let s = entry.status();

        let status_type = if s.is_index_new() || s.is_wt_new() {
            if s.is_wt_new() { FileStatusType::Untracked } else { FileStatusType::New }
        } else if s.is_index_modified() || s.is_wt_modified() {
            FileStatusType::Modified
        } else if s.is_index_deleted() || s.is_wt_deleted() {
            FileStatusType::Deleted
        } else if s.is_index_renamed() || s.is_wt_renamed() {
            FileStatusType::Renamed
        } else if s.is_index_typechange() || s.is_wt_typechange() {
            FileStatusType::Typechange
        } else if s.is_conflicted() {
            FileStatusType::Conflicted
        } else {
            continue;
        };

        result.push(FileStatus { path, status: status_type });
    }

    Ok(result)
}

/// Check if the working tree is clean.
pub fn is_clean(repo: &Repository) -> Result<bool> {
    let files = status(repo)?;
    Ok(files.is_empty())
}

/// Get a short status string (like `git status --short`).
pub fn status_short(repo: &Repository) -> Result<String> {
    let files = status(repo)?;
    if files.is_empty() {
        return Ok("Clean working tree".to_string());
    }

    let mut lines = Vec::new();
    for f in &files {
        let prefix = match f.status {
            FileStatusType::New => "A ",
            FileStatusType::Modified => "M ",
            FileStatusType::Deleted => "D ",
            FileStatusType::Renamed => "R ",
            FileStatusType::Typechange => "T ",
            FileStatusType::Untracked => "??",
            FileStatusType::Conflicted => "UU",
        };
        lines.push(format!("{prefix} {}", f.path));
    }

    // Cap at 2000 chars (matches original)
    let mut output = lines.join("\n");
    if output.len() > 2000 {
        output.truncate(2000);
        output.push_str("\n... (truncated)");
    }

    Ok(output)
}

// ─── Log ────────────────────────────────────────────────────────────────────

/// A commit entry from the log.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub sha: String,
    pub short_sha: String,
    pub message: String,
    pub author: String,
    pub timestamp: i64,
}

/// Get the last N commits from HEAD.
pub fn log(repo: &Repository, max_count: usize) -> Result<Vec<LogEntry>> {
    let mut revwalk = repo.revwalk()?;
    revwalk.push_head()?;
    revwalk.set_sorting(git2::Sort::TIME)?;

    let mut entries = Vec::new();
    for oid_result in revwalk.take(max_count) {
        let oid = oid_result?;
        let commit = repo.find_commit(oid)?;

        entries.push(LogEntry {
            sha: oid.to_string(),
            short_sha: format!("{:.7}", oid),
            message: commit.summary().unwrap_or("").to_string(),
            author: commit.author().name().unwrap_or("unknown").to_string(),
            timestamp: commit.time().seconds(),
        });
    }

    Ok(entries)
}

/// Format recent commits as a one-line summary (like `git log --oneline -n N`).
pub fn log_oneline(repo: &Repository, count: usize) -> Result<String> {
    let entries = log(repo, count)?;
    let lines: Vec<String> = entries.iter()
        .map(|e| format!("{} {}", e.short_sha, e.message))
        .collect();
    Ok(lines.join("\n"))
}

// ─── Diff ───────────────────────────────────────────────────────────────────

/// Diff statistics.
#[derive(Debug, Clone, Default)]
pub struct DiffStats {
    pub files_changed: usize,
    pub lines_added: usize,
    pub lines_removed: usize,
}

/// Per-file diff statistics.
#[derive(Debug, Clone)]
pub struct FileDiffStats {
    pub path: String,
    pub added: usize,
    pub removed: usize,
    pub is_binary: bool,
}

/// Get diff stats between HEAD and working tree.
pub fn diff_stats(repo: &Repository) -> Result<DiffStats> {
    let head_tree = repo.head()?.peel_to_tree()?;
    let diff = repo.diff_tree_to_workdir_with_index(Some(&head_tree), None)?;

    let stats = diff.stats()?;
    Ok(DiffStats {
        files_changed: stats.files_changed(),
        lines_added: stats.insertions(),
        lines_removed: stats.deletions(),
    })
}

/// Get per-file diff stats.
pub fn diff_per_file(repo: &Repository) -> Result<Vec<FileDiffStats>> {
    let head_tree = repo.head()?.peel_to_tree()?;
    let diff = repo.diff_tree_to_workdir_with_index(Some(&head_tree), None)?;

    // Use print_cb-style iteration which avoids the double-mutable-borrow
    // problem with diff.foreach's multiple closure arguments.
    let _stats = diff.stats()?;
    let mut file_stats: Vec<FileDiffStats> = Vec::new();

    for delta_idx in 0..diff.deltas().len() {
        let delta = diff.deltas().nth(delta_idx).unwrap();
        let path = delta.new_file().path()
            .or_else(|| delta.old_file().path())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "?".to_string());
        let is_binary = delta.new_file().is_binary() || delta.old_file().is_binary();
        file_stats.push(FileDiffStats {
            path,
            added: 0,
            removed: 0,
            is_binary,
        });
    }

    // Second pass: count added/removed lines per file using the patch API
    for (i, delta_idx) in (0..diff.deltas().len()).enumerate() {
        if let Ok(Some(patch)) = git2::Patch::from_diff(&diff, delta_idx) {
            let (_, adds, dels) = patch.line_stats().unwrap_or((0, 0, 0));
            if let Some(fs) = file_stats.get_mut(i) {
                fs.added = adds;
                fs.removed = dels;
            }
        }
    }

    Ok(file_stats)
}

// ─── Blame ──────────────────────────────────────────────────────────────────

/// A blame entry for a line range.
#[derive(Debug, Clone)]
pub struct BlameEntry {
    pub commit_sha: String,
    pub author: String,
    pub start_line: usize,
    pub end_line: usize,
    pub summary: String,
}

/// Get blame information for a file.
pub fn blame(repo: &Repository, file_path: &str) -> Result<Vec<BlameEntry>> {
    let blame = repo.blame_file(Path::new(file_path), None)
        .with_context(|| format!("Failed to blame: {file_path}"))?;

    let mut entries = Vec::new();
    for hunk in blame.iter() {
        let sig = hunk.final_signature();
        let oid = hunk.final_commit_id();

        // Try to get commit message
        let summary = repo.find_commit(oid)
            .map(|c| c.summary().unwrap_or("").to_string())
            .unwrap_or_default();

        entries.push(BlameEntry {
            commit_sha: format!("{:.7}", oid),
            author: sig.name().unwrap_or("unknown").to_string(),
            start_line: hunk.final_start_line(),
            end_line: hunk.final_start_line() + hunk.lines_in_hunk() - 1,
            summary,
        });
    }

    Ok(entries)
}

// ─── Remote ─────────────────────────────────────────────────────────────────

/// Get the remote URL for "origin".
pub fn remote_url(repo: &Repository) -> Option<String> {
    repo.find_remote("origin")
        .ok()
        .and_then(|r| r.url().map(|s| s.to_string()))
}

/// Get the git user name from config.
pub fn user_name(repo: &Repository) -> Option<String> {
    repo.config().ok()
        .and_then(|cfg| cfg.get_string("user.name").ok())
}

/// Get the git user email from config.
pub fn user_email(repo: &Repository) -> Option<String> {
    repo.config().ok()
        .and_then(|cfg| cfg.get_string("user.email").ok())
}

// ─── Stash ──────────────────────────────────────────────────────────────────

/// Stash all changes (including untracked) to get a clean working tree.
pub fn stash_all(repo: &mut Repository, message: &str) -> Result<git2::Oid> {
    let sig = repo.signature()?;
    let oid = repo.stash_save(
        &sig,
        message,
        Some(git2::StashFlags::INCLUDE_UNTRACKED),
    )?;
    Ok(oid)
}

// ─── Composite State ───────────────────────────────────────────────────────

/// Complete git repository state snapshot.
#[derive(Debug, Clone)]
pub struct GitState {
    pub branch: String,
    pub head_sha: String,
    pub is_clean: bool,
    pub remote_url: Option<String>,
    pub has_remote_tracking: bool,
    pub user_name: Option<String>,
    pub status_short: String,
    pub recent_commits: String,
}

/// Collect full git state for system prompt injection.
pub fn collect_state(path: &Path) -> Result<GitState> {
    let repo = open_repo(path)?;

    Ok(GitState {
        branch: current_branch(&repo).unwrap_or_else(|_| "unknown".to_string()),
        head_sha: head_sha(&repo).unwrap_or_else(|_| "unknown".to_string()),
        is_clean: is_clean(&repo).unwrap_or(true),
        remote_url: remote_url(&repo),
        has_remote_tracking: has_remote_tracking(&repo),
        user_name: user_name(&repo),
        status_short: status_short(&repo).unwrap_or_default(),
        recent_commits: log_oneline(&repo, 5).unwrap_or_default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_find_git_root_none() {
        let dir = tempdir().unwrap();
        assert!(find_git_root(dir.path()).is_none());
    }

    #[test]
    fn test_is_git_repo() {
        let dir = tempdir().unwrap();
        assert!(!is_git_repo(dir.path()));
    }
}
