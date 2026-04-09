"""Dream consolidation service — the actual intelligence behind memory consolidation.

Gathers context (existing memories + recent transcripts), calls the LLM with a
structured prompt, parses the JSON response, and applies memory operations via
MemoryStore.
"""

from __future__ import annotations

import json
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

from .api.client import BackendRouter, StreamRequest
from .ipc_types import MemoryRequest, MemoryResponse
from .memory.memdir import MemoryStore

logger = logging.getLogger(__name__)

# Maximum characters of transcript content to include in prompt
MAX_TRANSCRIPT_CHARS = 60_000
# Maximum characters per individual transcript file
MAX_PER_TRANSCRIPT = 12_000
# Default model for dream consolidation (cheap + capable enough)
DEFAULT_DREAM_MODEL = "claude-sonnet-4-5-20250514"


class DreamConsolidationService:
    """Runs the 4-phase memory consolidation using an LLM call."""

    def __init__(self, store: MemoryStore, backend: BackendRouter) -> None:
        self.store = store
        self.backend = backend

    async def handle(self, request: MemoryRequest) -> MemoryResponse:
        """Handle a dream_consolidate MemoryRequest end-to-end."""
        try:
            payload = request.payload
            memory_dir = str(payload.get("memory_dir", str(self.store.root_dir)))
            transcript_dir = str(payload.get("transcript_dir", ""))
            sessions_reviewed = int(payload.get("sessions_reviewed", 0) or 0)

            if not transcript_dir:
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=False,
                    error="dream_consolidate requires transcript_dir in payload",
                    payload={},
                )

            # Phase 1 — ORIENT: read existing memories
            existing_memories = self._gather_existing_memories()

            # Phase 2 — GATHER: read recent transcripts
            transcript_excerpts = self._gather_transcripts(
                Path(transcript_dir), sessions_reviewed
            )

            if not transcript_excerpts:
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=True,
                    payload={
                        "operations_applied": 0,
                        "summary": "No transcript content found to consolidate.",
                    },
                )

            # Phase 3 — CONSOLIDATE: call LLM
            prompt = self._build_prompt(existing_memories, transcript_excerpts)
            llm_response = await self._call_llm(prompt, request.request_id)

            if llm_response is None:
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=False,
                    error="LLM call returned no response",
                    payload={},
                )

            # Phase 4 — APPLY: parse and execute operations
            operations = self._parse_operations(llm_response)
            applied = self._apply_operations(operations)

            summary = self._extract_summary(llm_response, applied)

            return MemoryResponse(
                request_id=request.request_id,
                ok=True,
                payload={
                    "operations_applied": applied,
                    "summary": summary,
                },
            )

        except Exception as error:
            logger.exception("Dream consolidation failed")
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=f"Dream consolidation error: {error}",
                payload={},
            )

    def _gather_existing_memories(self) -> str:
        """Read all existing memories and format them as context."""
        lines: list[str] = []
        for scope in ("private", "team"):
            records = self.store.list_memories(scope)  # type: ignore[arg-type]
            if not records:
                continue
            lines.append(f"\n### {scope.title()} Memories ({len(records)} files)")
            for record in records:
                meta = record.metadata
                lines.append(
                    f"\n**[{meta.slug}]** ({meta.memory_type}) — {meta.description}"
                )
                body_preview = record.body[:500]
                if len(record.body) > 500:
                    body_preview += "..."
                lines.append(body_preview)

        # Also include MEMORY.md index
        entrypoint = self.store.entrypoint_path("private")
        if entrypoint.exists():
            index_content = entrypoint.read_text(encoding="utf-8").strip()
            if index_content:
                lines.insert(0, "### MEMORY.md Index\n" + index_content)

        return "\n".join(lines) if lines else "_No existing memories._"

    def _gather_transcripts(
        self, transcript_dir: Path, max_files: int
    ) -> str:
        """Read recent .jsonl transcript files, extract conversation excerpts."""
        if not transcript_dir.exists():
            return ""

        # Get .jsonl files sorted by modification time (newest first)
        jsonl_files = sorted(
            transcript_dir.glob("*.jsonl"),
            key=lambda p: p.stat().st_mtime,
            reverse=True,
        )

        if max_files > 0:
            jsonl_files = jsonl_files[:max_files]

        excerpts: list[str] = []
        total_chars = 0

        for filepath in jsonl_files:
            if total_chars >= MAX_TRANSCRIPT_CHARS:
                break

            try:
                content = filepath.read_text(encoding="utf-8", errors="replace")
            except OSError:
                continue

            session_messages: list[str] = []
            chars_this_file = 0

            for line in content.splitlines():
                if chars_this_file >= MAX_PER_TRANSCRIPT:
                    break
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue

                role = entry.get("role", "")
                text = self._extract_text_from_entry(entry)
                if not text or role not in ("user", "assistant"):
                    continue

                # Truncate individual messages
                if len(text) > 800:
                    text = text[:800] + "..."

                session_messages.append(f"  [{role}]: {text}")
                chars_this_file += len(text)

            if session_messages:
                session_name = filepath.stem
                excerpt = f"\n--- Session: {session_name} ---\n" + "\n".join(
                    session_messages
                )
                excerpts.append(excerpt)
                total_chars += chars_this_file

        return "\n".join(excerpts)

    @staticmethod
    def _extract_text_from_entry(entry: dict[str, Any]) -> str:
        """Extract readable text from a transcript JSONL entry."""
        content = entry.get("content")
        if isinstance(content, str):
            return content.strip()
        if isinstance(content, list):
            parts = []
            for block in content:
                if isinstance(block, dict):
                    if block.get("type") == "text":
                        parts.append(block.get("text", ""))
                    elif block.get("type") == "tool_use":
                        parts.append(f"[tool: {block.get('name', '?')}]")
                elif isinstance(block, str):
                    parts.append(block)
            return " ".join(parts).strip()
        return ""

    def _build_prompt(self, existing_memories: str, transcripts: str) -> str:
        """Build the consolidation prompt for the LLM."""
        today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        return f"""You are performing automatic memory consolidation ("dreaming").
Your job is to review recent session transcripts and decide what durable information
should be saved, updated, or removed from the memory system.

Today's date: {today}

## Existing Memories
{existing_memories}

## Recent Session Transcripts
{transcripts}

## Instructions

Review the transcripts above and identify information that will be useful in FUTURE
conversations. Focus on:
- Key decisions made
- Bugs encountered and how they were fixed
- User preferences expressed (coding style, communication style, tool preferences)
- Project context (deadlines, goals, architecture decisions)
- Pointers to external resources mentioned (URLs, dashboards, ticket systems)

Skip ephemeral details like exact debugging steps, file contents that can be re-read,
or temporary task state.

## Memory Tiers

Memories are stored in two tiers:
- **core** (max 10 files, max 50 lines each): Always in the system prompt. Reserved
  for user preferences, behavior feedback, and active project context.
- **archive** (unlimited): Searchable on demand via MemoryRecall tool. For historical
  decisions, old bugs, past project context, reference links.

New memories default to archive. Only "user" and "feedback" types should go to core.

## Response Format

Respond with a JSON object (and nothing else — no markdown fences, no explanation):

{{
  "operations": [
    {{
      "action": "save",
      "title": "Short descriptive title",
      "body": "Memory content (concise, < 40 lines)",
      "type": "user|feedback|project|reference",
      "tier": "core|archive",
      "description": "One-line description for the index"
    }},
    {{
      "action": "update",
      "slug": "existing-memory-slug",
      "body": "Updated full body content",
      "description": "Updated one-line description"
    }},
    {{
      "action": "delete",
      "slug": "outdated-memory-slug"
    }},
    {{
      "action": "promote",
      "slug": "frequently-accessed-archive-memory"
    }},
    {{
      "action": "demote",
      "slug": "stale-core-memory-no-longer-relevant"
    }}
  ],
  "summary": "Brief description of what was consolidated (1-2 sentences)"
}}

## Rules

- Convert relative dates to absolute dates (e.g., "yesterday" -> "{today}")
- Prefer updating existing memories over creating new ones
- Delete memories that are now contradicted by newer information
- Merge duplicate memories into one
- Each memory should be concise (< 40 lines for core, < 200 lines for archive)
- Memory types: user (role/prefs), feedback (behavior guidance), project (context), reference (external pointers)
- **Tier guidance**: user/feedback -> core; project/reference -> archive (unless very active)
- **Promote** archive memories to core if they are frequently relevant (high access_count)
- **Demote** core memories to archive if they haven't been accessed recently and aren't pinned
- **Wiki lint (Phase 5)**: also check for these structural issues:
  - If a memory has no backlinks AND hasn't been accessed in 30+ days, consider deleting it
  - If a [[slug]] reference points to a non-existent memory, flag it in your summary
  - If the same concept appears in multiple memories, merge them into one
  - If a concept is referenced by 2+ pages but has no page of its own, add a "save" operation for it
- If nothing meaningful was found, return {{"operations": [], "summary": "No new memories to consolidate"}}
- Only output valid JSON. No markdown code fences. No explanation text.
"""

    async def _call_llm(self, prompt: str, request_id: str) -> str | None:
        """Call the LLM and collect the full text response."""
        request = StreamRequest(
            request_id=f"dream-{request_id}",
            model=DEFAULT_DREAM_MODEL,
            messages=[{"role": "user", "content": prompt}],
            system_prompt="You are a memory consolidation agent. Respond only with valid JSON.",
            max_output_tokens=4096,
            provider="first_party",
        )

        text_parts: list[str] = []
        try:
            async for event in self.backend.stream_message(request):
                if isinstance(event, dict):
                    if event.get("type") == "text_delta":
                        text_parts.append(event.get("delta", ""))
                    elif event.get("type") == "message_done":
                        break
                elif hasattr(event, "type"):
                    if event.type == "text_delta":
                        text_parts.append(event.delta)
                    elif event.type == "message_done":
                        break
        except Exception as error:
            logger.error("Dream LLM call failed: %s", error, exc_info=True)
            return None

        result = "".join(text_parts).strip()
        logger.info("Dream LLM response (%d chars)", len(result))
        return result if result else None

    @staticmethod
    def _parse_operations(llm_response: str) -> list[dict[str, Any]]:
        """Parse the LLM's JSON response into a list of operations."""
        # Strip markdown fences if the LLM wrapped them anyway
        text = llm_response.strip()
        if text.startswith("```"):
            # Remove opening fence (```json or ```)
            first_newline = text.index("\n") if "\n" in text else 3
            text = text[first_newline + 1 :]
            if text.endswith("```"):
                text = text[:-3]
            text = text.strip()

        try:
            parsed = json.loads(text)
        except json.JSONDecodeError as error:
            logger.warning("Dream: failed to parse LLM JSON: %s", error)
            # Try to find JSON object in the response
            start = text.find("{")
            end = text.rfind("}") + 1
            if start >= 0 and end > start:
                try:
                    parsed = json.loads(text[start:end])
                except json.JSONDecodeError:
                    logger.error("Dream: could not recover JSON from response")
                    return []
            else:
                return []

        if not isinstance(parsed, dict):
            return []

        operations = parsed.get("operations", [])
        if not isinstance(operations, list):
            return []

        return operations

    def _apply_operations(self, operations: list[dict[str, Any]]) -> int:
        """Apply memory operations via MemoryStore. Returns count of operations applied.

        Secret scanning is handled by MemoryStore.save_memory() which redacts
        secrets from all scopes before writing to disk.
        """
        applied = 0

        for op in operations:
            action = op.get("action", "")
            try:
                if action == "save":
                    title = op.get("title", "").strip()
                    body = op.get("body", "").strip()
                    mem_type = op.get("type", "project").strip()
                    tier = op.get("tier", "archive").strip()
                    description = op.get("description", "").strip()
                    if not title or not body:
                        logger.warning("Dream: skipping save with empty title/body")
                        continue
                    if mem_type not in ("user", "feedback", "project", "reference"):
                        mem_type = "project"
                    if tier not in ("core", "archive"):
                        tier = "archive"
                    self.store.save_memory(
                        title=title,
                        body=body,
                        memory_type=mem_type,  # type: ignore[arg-type]
                        tier=tier,  # type: ignore[arg-type]
                        description=description,
                        source_type="dream",
                        tags=op.get("tags", []),
                        page_type=op.get("page_type", ""),
                    )
                    logger.info("Dream: saved memory '%s' (%s, tier=%s)", title, mem_type, tier)
                    applied += 1

                elif action == "update":
                    slug = op.get("slug", "").strip()
                    if not slug:
                        continue
                    existing = self.store.get_memory(slug)
                    if existing is None:
                        logger.warning("Dream: update target '%s' not found, skipping", slug)
                        continue
                    body = op.get("body", existing.body).strip()
                    description = op.get("description", existing.metadata.description)
                    self.store.save_memory(
                        title=existing.metadata.name,
                        body=body,
                        memory_type=existing.metadata.memory_type,
                        tier=existing.metadata.tier,
                        description=description,
                        slug=slug,
                        tags=existing.metadata.tags,
                        source_url=existing.metadata.source_url,
                        source_type=existing.metadata.source_type,
                        page_type=existing.metadata.page_type,
                        wing=existing.metadata.wing,
                        room=existing.metadata.room,
                    )
                    logger.info("Dream: updated memory '%s'", slug)
                    applied += 1

                elif action == "delete":
                    slug = op.get("slug", "").strip()
                    if not slug:
                        continue
                    deleted = self.store.delete_memory(slug)
                    if deleted:
                        logger.info("Dream: deleted memory '%s'", slug)
                        applied += 1
                    else:
                        logger.warning("Dream: delete target '%s' not found", slug)

                elif action == "promote":
                    slug = op.get("slug", "").strip()
                    if not slug:
                        continue
                    if self.store.promote_to_core(slug):
                        logger.info("Dream: promoted '%s' to core", slug)
                        applied += 1
                    else:
                        logger.warning("Dream: promote '%s' failed (not found or core full)", slug)

                elif action == "demote":
                    slug = op.get("slug", "").strip()
                    if not slug:
                        continue
                    if self.store.demote_to_archive(slug):
                        logger.info("Dream: demoted '%s' to archive", slug)
                        applied += 1
                    else:
                        logger.warning("Dream: demote '%s' failed (not found or pinned)", slug)

                else:
                    logger.warning("Dream: unknown operation action '%s'", action)

            except Exception as error:
                logger.error("Dream: operation failed (%s): %s", action, error)

        return applied

    @staticmethod
    def _extract_summary(llm_response: str, operations_applied: int) -> str:
        """Extract the summary from the LLM response, with fallback."""
        try:
            text = llm_response.strip()
            if text.startswith("```"):
                first_newline = text.index("\n") if "\n" in text else 3
                text = text[first_newline + 1 :]
                if text.endswith("```"):
                    text = text[:-3]
            parsed = json.loads(text.strip())
            if isinstance(parsed, dict) and "summary" in parsed:
                return str(parsed["summary"])
        except (json.JSONDecodeError, ValueError):
            pass
        return f"Consolidated {operations_applied} memory operations."
