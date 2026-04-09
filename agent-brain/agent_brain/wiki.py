"""Wiki service — ingest, query, and lint operations for the LLM Wiki.

The wiki is a persistent, compounding knowledge base built on top of MemoryStore.
Raw sources (URLs, files, transcripts) are ingested into structured wiki pages
with cross-references, tags, and source lineage. Queries can be filed back as
new pages so that explorations compound.

Operations:
  - wiki_ingest: source -> LLM extraction -> wiki pages + cross-refs + log
  - wiki_query: question -> LLM synthesis from wiki pages -> optionally save
  - wiki_lint: structural health check (orphans, broken refs, stale, missing)
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

# Cap source content sent to LLM
MAX_SOURCE_CHARS = 40_000
# Default model for wiki operations
DEFAULT_WIKI_MODEL = "claude-sonnet-4-5-20250514"


class WikiService:
    """Handles wiki_ingest, wiki_query, and wiki_lint IPC actions."""

    def __init__(self, store: MemoryStore, backend: BackendRouter) -> None:
        self.store = store
        self.backend = backend
        self.log_path = store.root_dir / "wiki" / "log.md"

    # ── Dispatch ────────────────────────────────────────────────────────

    async def handle(self, request: MemoryRequest) -> MemoryResponse:
        """Route wiki_* actions."""
        action = request.action
        try:
            if action == "wiki_ingest":
                return await self.ingest(request)
            elif action == "wiki_query":
                return await self.query(request)
            elif action == "wiki_lint":
                return await self.lint(request)
            else:
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=False,
                    error=f"Unknown wiki action: {action}",
                    payload={},
                )
        except Exception as e:
            logger.exception("Wiki %s failed", action)
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=f"Wiki {action} error: {e}",
                payload={},
            )

    # ── Ingest ──────────────────────────────────────────────────────────

    async def ingest(self, request: MemoryRequest) -> MemoryResponse:
        """Ingest a source into wiki pages."""
        payload = request.payload
        content = str(payload.get("content", ""))
        title = str(payload.get("title", "Untitled Source"))
        tags = payload.get("tags", [])
        if isinstance(tags, str):
            tags = [t.strip() for t in tags.split(",") if t.strip()]
        source_url = str(payload.get("source_url", ""))
        source_type = str(payload.get("source_type", "manual"))

        if not content.strip():
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error="wiki_ingest requires non-empty 'content' in payload",
                payload={},
            )

        # Truncate source
        if len(content) > MAX_SOURCE_CHARS:
            content = content[:MAX_SOURCE_CHARS] + "\n\n[Source truncated at 40K chars]"

        # Get existing wiki index for context
        index_content = self._read_index()

        # Build LLM prompt
        today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        prompt = f"""You are ingesting a new source into the wiki.

## Existing Wiki Index
{index_content}

## Source Content
Title: {title}
URL: {source_url or "(none)"}
Type: {source_type}
Tags: {', '.join(tags) if tags else "(none)"}

{content}

## Instructions

1. Create a **summary page** for this source (page_type: "summary")
2. Identify **entities** (people, tools, APIs, projects) worth their own page
3. Identify **concepts** (patterns, principles, methodologies) worth their own page
4. For existing wiki pages that should be updated, produce "update" operations
5. Cross-reference with [[slug]] links in page bodies

Return JSON (no markdown fences):

{{
  "pages": [
    {{
      "action": "save",
      "title": "Page Title",
      "body": "Page content with [[cross-references]]",
      "type": "project",
      "tier": "archive",
      "tags": ["tag1", "tag2"],
      "page_type": "summary",
      "description": "One-line description"
    }},
    {{
      "action": "update",
      "slug": "existing-page-slug",
      "body": "Updated content",
      "description": "Updated description"
    }}
  ],
  "log_entry": "Ingested: {title}. Created N pages, updated M pages."
}}

Rules:
- `type` must be one of: "user", "feedback", "project", "reference" (default: "project")
- `page_type` must be one of: "summary", "entity", "concept", "comparison", "log", "synthesis"
- `tier` must be "core" or "archive" (default: "archive")
- Dates: absolute ({today}), never relative
- Each page < 40 lines for core, < 200 lines for archive
- Prefer updating existing pages over creating duplicates
- Summary page gets source_url in metadata
- Only output valid JSON. No explanations before or after.
"""

        # Call LLM with strict JSON system prompt
        llm_response = await self._call_llm(
            prompt, request.request_id,
            system_prompt="You are a wiki maintenance agent. Output only valid JSON. No markdown fences, no explanation.",
        )
        if not llm_response:
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error="LLM returned no response for wiki_ingest",
                payload={},
            )

        # Parse and apply
        parsed = self._parse_json(llm_response)
        pages = parsed.get("pages", [])
        log_entry = parsed.get("log_entry", f"Ingested: {title}")

        _VALID_TYPES = {"user", "feedback", "project", "reference"}
        _VALID_TIERS = {"core", "archive"}
        _VALID_PAGE_TYPES = {"summary", "entity", "concept", "comparison", "log", "synthesis", ""}

        created = 0
        updated = 0
        for page in pages:
            action = page.get("action", "save")
            if action == "save":
                mem_type = page.get("type", "project")
                if mem_type not in _VALID_TYPES:
                    mem_type = "project"
                tier = page.get("tier", "archive")
                if tier not in _VALID_TIERS:
                    tier = "archive"
                page_type = page.get("page_type", "")
                if page_type not in _VALID_PAGE_TYPES:
                    page_type = ""
                self.store.save_memory(
                    title=page.get("title", "Untitled"),
                    body=page.get("body", ""),
                    memory_type=mem_type,
                    tier=tier,
                    tags=page.get("tags", tags),
                    source_url=source_url,
                    source_type=source_type,
                    page_type=page_type,
                    description=page.get("description", ""),
                )
                created += 1
            elif action == "update":
                slug = page.get("slug", "")
                existing = self.store.get_memory(slug)
                if existing:
                    self.store.save_memory(
                        title=existing.metadata.name,
                        body=page.get("body", existing.body),
                        memory_type=existing.metadata.memory_type,
                        tier=existing.metadata.tier,
                        description=page.get("description", existing.metadata.description),
                        slug=slug,
                        tags=existing.metadata.tags,
                        source_url=existing.metadata.source_url,
                        source_type=existing.metadata.source_type,
                        page_type=existing.metadata.page_type,
                        wing=existing.metadata.wing,
                        room=existing.metadata.room,
                    )
                    updated += 1

        self._append_log("ingest", title, log_entry)

        # ── SM2: Store raw verbatim chunks alongside wiki pages ──
        chunks_stored = self._store_raw_chunks(content, title, source_url, source_type)

        return MemoryResponse(
            request_id=request.request_id,
            ok=True,
            payload={
                "pages_created": created,
                "pages_updated": updated,
                "chunks_stored": chunks_stored,
                "summary": log_entry,
            },
        )

    # ── Query ───────────────────────────────────────────────────────────

    async def query(self, request: MemoryRequest) -> MemoryResponse:
        """Answer a question using wiki pages, optionally save as new page."""
        payload = request.payload
        question = str(payload.get("question", ""))
        save_as_page = bool(payload.get("save_as_page", False))
        page_title = str(payload.get("page_title", ""))
        tags = payload.get("tags", [])
        if isinstance(tags, str):
            tags = [t.strip() for t in tags.split(",") if t.strip()]

        if not question.strip():
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error="wiki_query requires 'question' in payload",
                payload={},
            )

        # Search for relevant pages
        recall_result = self.store.recall(question, limit=10)
        if not recall_result.memories:
            return MemoryResponse(
                request_id=request.request_id,
                ok=True,
                payload={
                    "answer": f"No wiki pages found relevant to: {question}",
                    "page_slug": None,
                },
            )

        # Build context from top pages (full bodies, not previews)
        page_context_parts: list[str] = []
        total_chars = 0
        for mem in recall_result.memories[:5]:
            page_text = (
                f"### [{mem.metadata.memory_type}] {mem.metadata.name}\n"
                f"_{mem.metadata.description}_\n"
                f"Tags: {', '.join(mem.metadata.tags) if mem.metadata.tags else 'none'}\n\n"
                f"{mem.body}\n"
            )
            if total_chars + len(page_text) > 30_000:
                break
            page_context_parts.append(page_text)
            total_chars += len(page_text)

        pages_text = "\n---\n".join(page_context_parts)

        prompt = f"""## Relevant Wiki Pages
{pages_text}

## Question
{question}

Synthesize a comprehensive answer using the wiki pages above.
Cite sources with [[slug]] links where relevant.
Be thorough but concise.
"""

        answer = await self._call_llm(
            prompt, request.request_id,
            system_prompt="You are a wiki knowledge assistant. Answer clearly and cite sources with [[slug]] links.",
        )
        if not answer:
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error="LLM returned no response for wiki_query",
                payload={},
            )

        page_slug = None
        if save_as_page and page_title:
            record = self.store.save_memory(
                title=page_title,
                body=answer,
                memory_type="project",
                tier="archive",
                tags=tags + ["wiki-query"],
                source_type="manual",
                page_type="synthesis",
                description=f"Answer to: {question[:80]}",
            )
            page_slug = record.metadata.slug
            self._append_log("query", page_title, f"Q: {question[:100]}")

        return MemoryResponse(
            request_id=request.request_id,
            ok=True,
            payload={
                "answer": answer,
                "page_slug": page_slug,
            },
        )

    # ── Lint ────────────────────────────────────────────────────────────

    async def lint(self, request: MemoryRequest) -> MemoryResponse:
        """Run structural health checks on the wiki."""
        report_parts: list[str] = ["# Wiki Health Report\n"]

        # Orphan pages
        orphans = self.store.list_orphan_memories(min_age_days=30)
        report_parts.append(f"## Orphan Pages ({len(orphans)})")
        if orphans:
            report_parts.append("_Pages with no backlinks and low access, older than 30 days:_")
            for r in orphans[:10]:
                report_parts.append(f"- `{r.metadata.slug}` — {r.metadata.description or r.metadata.name} (accessed {r.metadata.access_count}x)")
        else:
            report_parts.append("_None found._")

        # Broken references
        broken = self.store.list_broken_references()
        report_parts.append(f"\n## Broken References ({len(broken)})")
        if broken:
            report_parts.append("_Pages with [[slug]] links to non-existent pages:_")
            for slug, bad_refs in broken[:10]:
                report_parts.append(f"- `{slug}` references missing: {', '.join(f'[[{r}]]' for r in bad_refs)}")
        else:
            report_parts.append("_None found._")

        # Stale content
        stale = self.store.list_stale_memories(days=60)
        report_parts.append(f"\n## Stale Content ({len(stale)})")
        if stale:
            report_parts.append("_Pages not accessed in 60+ days:_")
            for r in stale[:10]:
                report_parts.append(f"- `{r.metadata.slug}` — {r.freshness}")
        else:
            report_parts.append("_None found._")

        # Missing pages
        missing = self.store.list_missing_pages()
        report_parts.append(f"\n## Missing Pages ({len(missing)})")
        if missing:
            report_parts.append("_Slugs referenced in 2+ pages but lacking their own page:_")
            for slug in missing[:10]:
                report_parts.append(f"- `[[{slug}]]`")
        else:
            report_parts.append("_None found._")

        # Stats
        all_mems = self.store.list_memories("private")
        core_count = len(self.store.list_memories_by_tier("core"))
        archive_count = len(self.store.list_memories_by_tier("archive"))
        report_parts.append(f"\n## Stats")
        report_parts.append(f"- Total pages: {len(all_mems)} (core: {core_count}, archive: {archive_count})")
        tag_counts: dict[str, int] = {}
        for r in all_mems:
            for t in r.metadata.tags:
                tag_counts[t] = tag_counts.get(t, 0) + 1
        if tag_counts:
            top_tags = sorted(tag_counts.items(), key=lambda x: x[1], reverse=True)[:10]
            report_parts.append(f"- Top tags: {', '.join(f'#{t}({c})' for t, c in top_tags)}")

        report = "\n".join(report_parts)
        self._append_log("lint", "Health Check", f"Found {len(orphans)} orphans, {len(broken)} broken refs, {len(stale)} stale, {len(missing)} missing")

        return MemoryResponse(
            request_id=request.request_id,
            ok=True,
            payload={"report": report},
        )

    # ── Helpers ─────────────────────────────────────────────────────────

    def _read_index(self) -> str:
        """Read combined wiki index for LLM context."""
        parts: list[str] = []
        core_idx = self.store.core_dir / "MEMORY.md"
        if core_idx.exists():
            content = core_idx.read_text(encoding="utf-8").strip()
            if content:
                parts.append(f"### Core\n{content}")
        archive_idx = self.store.archive_dir / "ARCHIVE_INDEX.md"
        if archive_idx.exists():
            content = archive_idx.read_text(encoding="utf-8").strip()
            if content:
                # Only first 30 lines of archive index
                lines = content.splitlines()[:30]
                parts.append(f"### Archive ({len(content.splitlines())} total)\n" + "\n".join(lines))
        return "\n\n".join(parts) if parts else "_Empty wiki._"

    async def _call_llm(
        self, prompt: str, request_id: str, system_prompt: str | None = None
    ) -> str | None:
        """Call the LLM and collect the full text response."""
        request = StreamRequest(
            request_id=f"wiki-{request_id}",
            model=DEFAULT_WIKI_MODEL,
            messages=[{"role": "user", "content": prompt}],
            system_prompt=system_prompt or "You are a wiki maintenance agent. Respond concisely.",
            max_output_tokens=8192,
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
        except Exception as e:
            logger.error("Wiki LLM call failed: %s", e, exc_info=True)
            return None
        return "".join(text_parts).strip() or None

    @staticmethod
    def _parse_json(text: str) -> dict[str, Any]:
        """Parse JSON from LLM response, stripping markdown fences."""
        text = text.strip()
        if text.startswith("```"):
            first_nl = text.index("\n") if "\n" in text else 3
            text = text[first_nl + 1:]
            if text.endswith("```"):
                text = text[:-3].strip()
        try:
            return json.loads(text)
        except json.JSONDecodeError:
            start = text.find("{")
            end = text.rfind("}") + 1
            if start >= 0 and end > start:
                try:
                    return json.loads(text[start:end])
                except json.JSONDecodeError:
                    pass
        return {}

    def _store_raw_chunks(
        self, content: str, title: str, source_url: str, source_type: str
    ) -> int:
        """Store raw verbatim chunks of the source content in the vector store."""
        vs = self.store.vector_store
        if vs is None:
            return 0
        try:
            from .memory.chunker import chunk_text
            from .memory.memdir import slugify

            source_slug = f"src-{slugify(title)}"
            chunks = chunk_text(content, source_slug)
            for chunk in chunks:
                vs.index_chunk(
                    chunk_id=chunk["id"],
                    text=chunk["text"],
                    metadata={
                        "source_slug": source_slug,
                        "source_url": source_url,
                        "source_type": source_type,
                        "chunk_index": chunk["chunk_index"],
                    },
                )
            return len(chunks)
        except Exception as e:
            logger.warning("Failed to store raw chunks: %s", e)
            return 0

    def _append_log(self, entry_type: str, title: str, details: str) -> None:
        """Append a timestamped entry to wiki/log.md."""
        self.log_path.parent.mkdir(parents=True, exist_ok=True)
        timestamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M")
        line = f"## [{timestamp}] {entry_type} | {title}\n{details}\n\n"
        with open(self.log_path, "a", encoding="utf-8") as f:
            f.write(line)
