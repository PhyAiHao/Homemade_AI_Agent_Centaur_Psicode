//! Context collection — gather system and git context for system prompt assembly.
//!
//! Mirrors `src/context.ts`. Collects git status, environment info,
//! working directory, and CLAUDE.md files for injection into the system prompt.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

/// System context — collected once at conversation start.
#[derive(Debug, Clone)]
pub struct SystemContext {
    pub git_status: Option<GitContext>,
    pub cwd: PathBuf,
    pub platform: String,
    pub shell: String,
    pub os_version: String,
    pub current_date: String,
    pub is_git_repo: bool,
}

/// Git context for the system prompt.
#[derive(Debug, Clone)]
pub struct GitContext {
    pub branch: String,
    pub default_branch: String,
    pub user_name: Option<String>,
    pub status_short: String,
    pub recent_commits: String,
    pub is_clean: bool,
}

/// User context — CLAUDE.md and other per-project files.
#[derive(Debug, Clone)]
pub struct UserContext {
    pub claude_md: Vec<ClaudeMdFile>,
    pub current_date: String,
}

/// A discovered CLAUDE.md or MEMORY.md file.
#[derive(Debug, Clone)]
pub struct ClaudeMdFile {
    pub path: PathBuf,
    pub content: String,
    pub scope: ClaudeMdScope,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ClaudeMdScope {
    /// Project-level (in the working directory or .claude/)
    Project,
    /// User-level (~/.claude/)
    User,
}

/// Collect system context for the current session.
pub async fn collect_system_context(cwd: &Path) -> SystemContext {
    let now = chrono::Local::now();

    let git_context = if crate::git::is_git_repo(cwd) {
        match crate::git::open_repo(cwd) {
            Ok(repo) => {
                let branch = crate::git::current_branch(&repo)
                    .unwrap_or_else(|_| "unknown".to_string());
                let default_branch = crate::git::default_branch(&repo)
                    .unwrap_or_else(|_| "main".to_string());
                let user_name = crate::git::user_name(&repo);
                let status_short = crate::git::status_short(&repo)
                    .unwrap_or_default();
                let recent_commits = crate::git::log_oneline(&repo, 5)
                    .unwrap_or_default();
                let is_clean = crate::git::is_clean(&repo)
                    .unwrap_or(true);

                Some(GitContext {
                    branch,
                    default_branch,
                    user_name,
                    status_short,
                    recent_commits,
                    is_clean,
                })
            }
            Err(_) => None,
        }
    } else {
        None
    };

    SystemContext {
        git_status: git_context,
        cwd: cwd.to_path_buf(),
        platform: std::env::consts::OS.to_string(),
        shell: std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string()),
        os_version: get_os_version(),
        current_date: now.format("%Y-%m-%d").to_string(),
        is_git_repo: crate::git::is_git_repo(cwd),
    }
}

/// Collect user context (CLAUDE.md files).
pub async fn collect_user_context(cwd: &Path) -> UserContext {
    let now = chrono::Local::now();
    let mut claude_md_files = Vec::new();

    // Search for CLAUDE.md in the project directory hierarchy
    let project_files = find_claude_md_files(cwd).await;
    claude_md_files.extend(project_files);

    // Search in ~/.claude/
    if let Some(home) = dirs::home_dir() {
        let user_claude_md = home.join(".claude").join("CLAUDE.md");
        if let Ok(content) = tokio::fs::read_to_string(&user_claude_md).await {
            claude_md_files.push(ClaudeMdFile {
                path: user_claude_md,
                content,
                scope: ClaudeMdScope::User,
            });
        }

        let user_wiki_schema = home.join(".claude").join("WIKI_SCHEMA.md");
        if let Ok(content) = tokio::fs::read_to_string(&user_wiki_schema).await {
            claude_md_files.push(ClaudeMdFile {
                path: user_wiki_schema,
                content,
                scope: ClaudeMdScope::User,
            });
        }
    }

    UserContext {
        claude_md: claude_md_files,
        current_date: now.format("%Y-%m-%d").to_string(),
    }
}

/// Find CLAUDE.md files by walking up from the working directory.
async fn find_claude_md_files(start: &Path) -> Vec<ClaudeMdFile> {
    let mut files = Vec::new();
    let mut current = start.to_path_buf();

    // Walk up the directory tree
    loop {
        for name in &["CLAUDE.md", ".claude/CLAUDE.md", "MEMORY.md", ".claude/MEMORY.md", "WIKI_SCHEMA.md", ".claude/WIKI_SCHEMA.md"] {
            let path = current.join(name);
            if let Ok(content) = tokio::fs::read_to_string(&path).await {
                files.push(ClaudeMdFile {
                    path,
                    content,
                    scope: ClaudeMdScope::Project,
                });
            }
        }

        if !current.pop() {
            break;
        }

        // Don't walk above the home directory
        if let Some(home) = dirs::home_dir() {
            if current == home {
                break;
            }
        }
    }

    // Reverse so parent CLAUDE.md comes first (lower priority)
    files.reverse();
    files
}

/// Format context for system prompt injection.
pub fn format_system_context(ctx: &SystemContext) -> String {
    let mut parts = Vec::new();

    parts.push(format!("Working directory: {}", ctx.cwd.display()));
    parts.push(format!("Platform: {} ({})", ctx.platform, ctx.os_version));
    parts.push(format!("Shell: {}", ctx.shell));
    parts.push(format!("Date: {}", ctx.current_date));

    if let Some(ref git) = ctx.git_status {
        parts.push(format!("Git branch: {} (default: {})", git.branch, git.default_branch));
        if let Some(ref name) = git.user_name {
            parts.push(format!("Git user: {name}"));
        }
        if !git.is_clean {
            parts.push(format!("Git status:\n{}", git.status_short));
        }
        if !git.recent_commits.is_empty() {
            parts.push(format!("Recent commits:\n{}", git.recent_commits));
        }
    } else if ctx.is_git_repo {
        parts.push("Git: repository detected but could not read state".to_string());
    } else {
        parts.push("Git: not a git repository".to_string());
    }

    parts.join("\n")
}

/// Format user context (CLAUDE.md) for system prompt injection.
pub fn format_user_context(ctx: &UserContext) -> String {
    if ctx.claude_md.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    for file in &ctx.claude_md {
        let scope = match file.scope {
            ClaudeMdScope::Project => "project",
            ClaudeMdScope::User => "user",
        };
        parts.push(format!(
            "# CLAUDE.md ({scope}: {})\n\n{}",
            file.path.display(),
            file.content
        ));
    }

    parts.join("\n\n---\n\n")
}

/// Get OS version string.
fn get_os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_else(|| "macOS".to_string())
    }
    #[cfg(target_os = "linux")]
    {
        std::fs::read_to_string("/etc/os-release")
            .ok()
            .and_then(|content| {
                content.lines()
                    .find(|line| line.starts_with("PRETTY_NAME="))
                    .map(|line| line.trim_start_matches("PRETTY_NAME=").trim_matches('"').to_string())
            })
            .unwrap_or_else(|| "Linux".to_string())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        std::env::consts::OS.to_string()
    }
}
