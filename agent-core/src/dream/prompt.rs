//! Dream consolidation prompt — the 4-phase instruction sent to the dream agent.

use std::path::Path;

/// Build the consolidation prompt for the dream agent.
///
/// The prompt has 4 phases:
///   Phase 1 — Orient: read existing memories
///   Phase 2 — Gather: scan recent session transcripts
///   Phase 3 — Consolidate: write/update topic memory files
///   Phase 4 — Prune: maintain MEMORY.md index
pub fn build_consolidation_prompt(
    memory_dir: &Path,
    transcript_dir: &Path,
    sessions_reviewed: u32,
) -> String {
    let mem_dir = memory_dir.to_string_lossy();
    let trans_dir = transcript_dir.to_string_lossy();

    format!(r#"You are performing automatic memory consolidation ("dreaming").
Your job is to review recent session transcripts and organize important information
into well-structured memory files.

Memory directory: {mem_dir}
Transcript directory: {trans_dir}
Sessions to review: {sessions_reviewed}

Execute the following 4 phases in order:

## Phase 1 — Orient

1. List the memory directory: `ls {mem_dir}`
2. Read `MEMORY.md` (the index file) if it exists
3. Skim existing topic files to understand what's already recorded
4. Note any gaps or outdated information

## Phase 2 — Gather Recent Signal

1. List transcript files modified recently in `{trans_dir}`
2. For each recent transcript, scan for:
   - Key decisions made
   - Bugs encountered and fixed
   - Architecture or design patterns discussed
   - User preferences expressed
   - Project context (deadlines, goals, constraints)
   - Configuration or setup changes
3. Focus on information that will be useful in FUTURE conversations
4. Skip ephemeral details (exact file contents, debugging steps, etc.)

## Phase 3 — Consolidate

For each piece of durable information found:
1. Check if it belongs in an existing topic file → update that file
2. If it's a new topic → create a new file with frontmatter:
   ```markdown
   ---
   name: {{topic name}}
   description: {{one-line description}}
   type: {{user|feedback|project|reference}}
   ---

   {{content}}
   ```
3. Convert relative dates to absolute dates (e.g., "yesterday" → "2026-04-05")
4. Delete information that is now contradicted by newer sessions
5. Merge duplicate entries

## Phase 4 — Prune & Index

1. Update `MEMORY.md`:
   - Keep it under 200 lines
   - Each entry: `- [Title](file.md) — one-line description`
   - Remove entries for files that no longer exist
   - Add entries for newly created files
2. Delete memory files that are completely outdated
3. Resolve any contradictions between files

## Rules

- Only write to files inside {mem_dir}
- Do NOT write to MEMORY.md directly with content — it's an INDEX only
- Convert relative dates to absolute dates
- Be concise — each memory file should be < 50 lines
- Prefer updating existing files over creating new ones
- If nothing meaningful was found in the transcripts, do nothing and report "No new memories to consolidate"
"#)
}
