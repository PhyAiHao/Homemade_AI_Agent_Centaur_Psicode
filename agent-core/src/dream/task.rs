//! Dream task — tracks the background dream's progress for TUI display.

use std::time::Instant;

/// Current phase of the dream.
#[derive(Debug, Clone, PartialEq)]
pub enum DreamPhase {
    /// Reading existing memories.
    Starting,
    /// Actively writing/updating memory files.
    Updating,
    /// Dream finished.
    Completed,
    /// Dream failed.
    Failed,
    /// Dream was killed by user.
    Killed,
}

impl DreamPhase {
    pub fn display(&self) -> &str {
        match self {
            Self::Starting => "reading memories...",
            Self::Updating => "consolidating...",
            Self::Completed => "done",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }

    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// A single turn of the dream agent (for UI display).
#[derive(Debug, Clone)]
pub struct DreamTurn {
    pub text: String,
    pub tool_use_count: u32,
}

/// State of a running dream task.
#[derive(Debug, Clone)]
pub struct DreamTaskState {
    pub id: String,
    pub phase: DreamPhase,
    pub sessions_reviewing: u32,
    pub files_touched: Vec<String>,
    pub turns: Vec<DreamTurn>,
    pub started_at: Instant,
}

impl DreamTaskState {
    pub fn new(sessions: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            phase: DreamPhase::Starting,
            sessions_reviewing: sessions,
            files_touched: Vec::new(),
            turns: Vec::new(),
            started_at: Instant::now(),
        }
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.started_at.elapsed().as_secs()
    }

    pub fn add_turn(&mut self, text: String, tool_use_count: u32) {
        // Keep last 30 turns
        if self.turns.len() >= 30 {
            self.turns.remove(0);
        }
        self.turns.push(DreamTurn { text, tool_use_count });
    }

    pub fn touch_file(&mut self, path: String) {
        if !self.files_touched.contains(&path) {
            self.files_touched.push(path);
        }
        // Transition to Updating phase on first file write
        if self.phase == DreamPhase::Starting {
            self.phase = DreamPhase::Updating;
        }
    }

    pub fn status_line(&self) -> String {
        let elapsed = self.elapsed_secs();
        let phase = self.phase.display();
        let files = self.files_touched.len();
        format!("Dream: {phase} ({elapsed}s, {files} files)")
    }
}
