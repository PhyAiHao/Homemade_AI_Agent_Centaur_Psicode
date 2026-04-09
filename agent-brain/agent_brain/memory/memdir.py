from __future__ import annotations

import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Literal

from .._compat import Field
from ..types.base import AgentBaseModel

try:
    from jinja2 import Template
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Template = None  # type: ignore[assignment]

MemoryType = Literal["user", "feedback", "project", "reference"]
MemoryScope = Literal["private", "team"]
MemoryTier = Literal["core", "archive"]

ENTRYPOINT_NAME = "MEMORY.md"
ARCHIVE_INDEX_NAME = "ARCHIVE_INDEX.md"
DEFAULT_MEMORY_ROOT = Path.home() / ".agent" / "memory"
MEMORY_TYPES: tuple[MemoryType, ...] = ("user", "feedback", "project", "reference")

# Core tier limits: small enough to fit in system prompt every turn
MAX_CORE_FILES = 10
MAX_CORE_MEMORY_LINES = 50
MAX_CORE_MEMORY_BYTES = 3_000

# Archive tier limits: larger but still bounded per file
MAX_ARCHIVE_MEMORY_LINES = 200
MAX_ARCHIVE_MEMORY_BYTES = 12_000

SYSTEM_PROMPT_TEMPLATE = """# Persistent Memory

You have a file-based memory system rooted at `{{ memory_dir }}`.

## Available scopes
- `private` memories are for user-specific context and private collaboration preferences.
- `team` memories are shared project context. Never store secrets there.

## Memory taxonomy
- `user`: role, preferences, expertise, working style
- `feedback`: how the user wants you to behave or what approaches to avoid/repeat
- `project`: deadlines, initiatives, incidents, team context not derivable from code
- `reference`: pointers to external systems, dashboards, tickets, docs

## What not to store
- Code structure, architecture, or file paths that can be re-read from the repo
- Temporary task state that belongs in a plan or task list instead
- Secrets or credentials, especially in team memory

## Private memory index
{{ private_index }}

{% if include_team %}
## Team memory index
{{ team_index }}
{% endif %}

{% if relevant_memories %}
## Relevant memories for this request
{% for memory in relevant_memories %}
- [{{ memory.metadata.memory_type }}][{{ memory.metadata.scope }}] {{ memory.metadata.name }}: {{ memory.metadata.description or "No description" }}
  Path: {{ memory.path }}
  Freshness: {{ memory.freshness }}
{% endfor %}
{% endif %}
"""


class MemoryMetadata(AgentBaseModel):
    name: str
    description: str = ""
    memory_type: MemoryType
    scope: MemoryScope = "private"
    tier: MemoryTier = "archive"
    created_at: str = ""
    updated_at: str = ""
    last_accessed_at: str = ""
    access_count: int = 0
    pinned: bool = False
    slug: str = ""
    # Wiki fields
    tags: list[str] = Field(default_factory=list)
    source_url: str = ""
    source_type: str = ""  # "transcript" | "web" | "file" | "manual" | "dream"
    references: list[str] = Field(default_factory=list)  # outbound [[slug]] links
    backlinks: list[str] = Field(default_factory=list)   # inbound links from other pages
    page_type: str = ""  # "entity" | "concept" | "summary" | "comparison" | "log" | "synthesis"
    # Spatial organization (wing/room)
    wing: str = ""   # Project, person, or domain (e.g., "auth-system", "frontend")
    room: str = ""   # Aspect within the wing (e.g., "decisions", "bugs", "architecture")
    # Importance scoring
    importance_score: float = 0.0


class MemoryRecord(AgentBaseModel):
    metadata: MemoryMetadata
    body: str
    path: str
    mtime_ms: float = 0.0

    @property
    def freshness(self) -> str:
        days = max(
            0,
            int(
                (
                    datetime.now(timezone.utc).timestamp() * 1000 - self.mtime_ms
                )
                // 86_400_000
            ),
        )
        if days == 0:
            return "today"
        if days == 1:
            return "yesterday"
        return f"{days} days ago"


class MemoryCandidate(AgentBaseModel):
    title: str
    body: str
    memory_type: MemoryType
    scope: MemoryScope = "private"
    description: str = ""
    confidence: float = 0.5


class MemoryRecallResult(AgentBaseModel):
    query: str
    memories: list[MemoryRecord] = Field(default_factory=list)


class MemoryStore:
    def __init__(self, root_dir: str | Path | None = None) -> None:
        self.root_dir = Path(root_dir or DEFAULT_MEMORY_ROOT).expanduser()
        self.private_dir = self.root_dir
        self.team_dir = self.root_dir / "team"
        # Core/Archive tier directories (within private scope)
        self.core_dir = self.root_dir / "core"
        self.archive_dir = self.root_dir / "archive"
        self.ensure_structure()
        # TF-IDF search engine — built lazily on first search
        self._search_engine: Any | None = None
        self._search_index_built = False
        # Vector store — built lazily on first use
        self._vector_store: Any | None = None
        self._vector_store_init = False

    def ensure_structure(self) -> None:
        self.private_dir.mkdir(parents=True, exist_ok=True)
        self.team_dir.mkdir(parents=True, exist_ok=True)
        self.core_dir.mkdir(parents=True, exist_ok=True)
        self.archive_dir.mkdir(parents=True, exist_ok=True)
        for scope in ("private", "team"):
            entrypoint = self.entrypoint_path(scope)
            if not entrypoint.exists():
                entrypoint.write_text("", encoding="utf-8")
        # Ensure core and archive indexes exist
        for index_path in [self.core_dir / ENTRYPOINT_NAME, self.archive_dir / ARCHIVE_INDEX_NAME]:
            if not index_path.exists():
                index_path.write_text("", encoding="utf-8")

    def entrypoint_path(self, scope: MemoryScope) -> Path:
        return self.scope_dir(scope) / ENTRYPOINT_NAME

    def scope_dir(self, scope: MemoryScope) -> Path:
        return self.private_dir if scope == "private" else self.team_dir

    def tier_dir(self, tier: MemoryTier) -> Path:
        """Return the directory for a given tier."""
        return self.core_dir if tier == "core" else self.archive_dir

    def list_memories(self, scope: MemoryScope = "private") -> list[MemoryRecord]:
        """List all memories across all tiers for a given scope."""
        directory = self.scope_dir(scope)
        records: list[MemoryRecord] = []
        for file_path in sorted(directory.rglob("*.md")):
            if file_path.name in (ENTRYPOINT_NAME, ARCHIVE_INDEX_NAME):
                continue
            if scope == "private" and self.team_dir in file_path.parents:
                continue
            record = self._load_record(file_path, scope)
            if record is not None:
                records.append(record)
        return sorted(records, key=lambda record: record.mtime_ms, reverse=True)

    def list_memories_by_tier(self, tier: MemoryTier) -> list[MemoryRecord]:
        """List memories in a specific tier (core or archive)."""
        directory = self.tier_dir(tier)
        records: list[MemoryRecord] = []
        for file_path in sorted(directory.glob("*.md")):
            if file_path.name in (ENTRYPOINT_NAME, ARCHIVE_INDEX_NAME):
                continue
            record = self._load_record(file_path, "private")
            if record is not None:
                records.append(record)
        return sorted(records, key=lambda record: record.mtime_ms, reverse=True)

    def get_memory(self, slug: str, scope: MemoryScope = "private") -> MemoryRecord | None:
        """Find a memory by slug, searching both tiers."""
        # Search core first, then archive, then legacy flat directory
        for directory in [self.core_dir, self.archive_dir, self.scope_dir(scope)]:
            path = directory / f"{slug}.md"
            if path.exists():
                record = self._load_record(path, scope)
                if record is not None:
                    return record
        # Fallback: full scan
        for record in self.list_memories(scope):
            if record.metadata.slug == slug or Path(record.path).stem == slug:
                return record
        return None

    def save_memory(
        self,
        *,
        title: str,
        body: str,
        memory_type: MemoryType,
        scope: MemoryScope = "private",
        tier: MemoryTier = "archive",
        description: str = "",
        slug: str | None = None,
        pinned: bool = False,
        tags: list[str] | None = None,
        source_url: str = "",
        source_type: str = "",
        page_type: str = "",
        wing: str = "",
        room: str = "",
    ) -> MemoryRecord:
        if memory_type not in MEMORY_TYPES:
            raise ValueError(f"Unsupported memory type: {memory_type}")
        slug = slug or slugify(title)

        # Secret scanning — redact before writing
        from .team_sync import scan_for_secrets
        combined = "\n".join([title, description, body])
        secret_matches = scan_for_secrets(combined)
        if secret_matches:
            labels = ", ".join(m.label for m in secret_matches)
            import logging
            logging.getLogger(__name__).warning(
                "Secret detected in memory save (slug=%s, tier=%s): %s — stripping",
                slug, tier, labels,
            )
            from .team_sync import SECRET_RULES
            for _rule_id, pattern, _label in SECRET_RULES:
                body = re.sub(pattern, "[REDACTED]", body, flags=re.I | re.S)
                description = re.sub(pattern, "[REDACTED]", description, flags=re.I | re.S)

        # Enforce size limits based on tier
        body = self._enforce_size_limit(body, tier)

        # Auto-tier: user and feedback types go to core by default (always relevant)
        if memory_type in ("user", "feedback") and tier == "archive":
            core_count = len(self.list_memories_by_tier("core"))
            if core_count < MAX_CORE_FILES:
                tier = "core"

        # Enforce core file limit
        if tier == "core":
            core_count = len(self.list_memories_by_tier("core"))
            # Check if this slug already exists in core (update, not new)
            existing_in_core = (self.core_dir / f"{slug}.md").exists()
            if not existing_in_core and core_count >= MAX_CORE_FILES:
                tier = "archive"  # Core is full, save to archive instead

        now = iso_now()
        existing = self.get_memory(slug, scope)

        # Extract [[slug]] references from body for cross-referencing
        found_refs = re.findall(r"\[\[([a-zA-Z0-9_-]+)\]\]", body)

        # For new pages: scan existing memories to find who already references this slug
        initial_backlinks: list[str] = []
        if existing:
            initial_backlinks = existing.metadata.backlinks
        else:
            for other in self.list_memories(scope):
                if slug in other.metadata.references:
                    initial_backlinks.append(other.metadata.slug)

        metadata = MemoryMetadata(
            name=title,
            description=description,
            memory_type=memory_type,
            scope=scope,
            tier=tier,
            created_at=existing.metadata.created_at if existing else now,
            updated_at=now,
            last_accessed_at=now,
            access_count=(existing.metadata.access_count if existing else 0) + 1,
            pinned=pinned or (existing.metadata.pinned if existing else False),
            slug=slug,
            tags=tags or (existing.metadata.tags if existing else []),
            source_url=source_url or (existing.metadata.source_url if existing else ""),
            source_type=source_type or (existing.metadata.source_type if existing else ""),
            page_type=page_type or (existing.metadata.page_type if existing else ""),
            wing=wing or (existing.metadata.wing if existing else ""),
            room=room or (existing.metadata.room if existing else ""),
            references=found_refs,
            backlinks=initial_backlinks,
        )

        # Write to the tier directory
        target_dir = self.tier_dir(tier) if scope == "private" else self.scope_dir(scope)
        path = target_dir / f"{slug}.md"

        # If memory moved tiers, remove from old location
        if existing and Path(existing.path).parent != target_dir:
            Path(existing.path).unlink(missing_ok=True)

        path.write_text(serialize_memory_file(metadata, body), encoding="utf-8")

        # Maintain backlinks on referenced pages
        old_refs = existing.metadata.references if existing else []
        self._update_cross_references(slug, old_refs, found_refs)

        self._rewrite_all_indexes()
        record = self._load_record(path, scope)
        assert record is not None

        # Update search index
        self._index_document_in_search(slug, record)
        # Update vector store
        self._index_in_vector_store(slug, record)
        # Compute and persist importance score
        self._update_importance(record)

        return record

    def delete_memory(self, slug: str, scope: MemoryScope = "private") -> bool:
        record = self.get_memory(slug, scope)
        if record is None:
            return False
        Path(record.path).unlink(missing_ok=True)
        self._rewrite_all_indexes()
        # Remove from search index
        if self._search_engine is not None:
            self._search_engine.remove_document(slug)
        # Remove from vector store
        vs = self.vector_store
        if vs is not None:
            vs.remove_memory(slug)
            vs.remove_chunks_by_source(slug)
        return True

    def promote_to_core(self, slug: str) -> bool:
        """Move a memory from archive to core tier."""
        record = self.get_memory(slug)
        if record is None:
            return False
        core_count = len(self.list_memories_by_tier("core"))
        if core_count >= MAX_CORE_FILES:
            return False  # Core is full
        # Re-save with tier=core (handles file move), preserving all wiki metadata
        self.save_memory(
            title=record.metadata.name,
            body=record.body,
            memory_type=record.metadata.memory_type,
            tier="core",
            description=record.metadata.description,
            slug=slug,
            pinned=record.metadata.pinned,
            tags=record.metadata.tags,
            source_url=record.metadata.source_url,
            source_type=record.metadata.source_type,
            page_type=record.metadata.page_type,
            wing=record.metadata.wing,
            room=record.metadata.room,
        )
        return True

    def demote_to_archive(self, slug: str) -> bool:
        """Move a memory from core to archive tier."""
        record = self.get_memory(slug)
        if record is None:
            return False
        if record.metadata.pinned:
            return False  # Pinned memories stay in core
        self.save_memory(
            title=record.metadata.name,
            body=record.body,
            memory_type=record.metadata.memory_type,
            tier="archive",
            description=record.metadata.description,
            slug=slug,
            tags=record.metadata.tags,
            source_url=record.metadata.source_url,
            source_type=record.metadata.source_type,
            page_type=record.metadata.page_type,
            wing=record.metadata.wing,
            room=record.metadata.room,
        )
        return True

    @staticmethod
    def _enforce_size_limit(body: str, tier: MemoryTier) -> str:
        """Truncate body to fit tier limits."""
        max_lines = MAX_CORE_MEMORY_LINES if tier == "core" else MAX_ARCHIVE_MEMORY_LINES
        max_bytes = MAX_CORE_MEMORY_BYTES if tier == "core" else MAX_ARCHIVE_MEMORY_BYTES
        lines = body.splitlines()
        if len(lines) > max_lines:
            lines = lines[:max_lines]
            lines.append("[truncated — memory exceeded size limit]")
        result = "\n".join(lines)
        if len(result.encode("utf-8")) > max_bytes:
            while len(result.encode("utf-8")) > max_bytes - 50 and lines:
                lines.pop()
            lines.append("[truncated — memory exceeded size limit]")
            result = "\n".join(lines)
        return result

    @property
    def vector_store(self) -> Any | None:
        """Lazily create the vector store. Returns None if chromadb unavailable."""
        if not self._vector_store_init:
            self._vector_store_init = True
            try:
                from .vector_store import VectorStore
                vs = VectorStore(self.root_dir / "vectors")
                if vs.available:
                    self._vector_store = vs
            except Exception:
                pass
        return self._vector_store

    def _index_in_vector_store(self, slug: str, record: "MemoryRecord") -> None:
        """Sync a memory document to the vector store."""
        vs = self.vector_store
        if vs is None:
            return
        text = " ".join([
            record.metadata.name,
            record.metadata.description,
            " ".join(record.metadata.tags),
            record.body,
        ])
        meta: dict[str, Any] = {
            "type": record.metadata.memory_type,
            "tier": record.metadata.tier,
            "page_type": record.metadata.page_type,
        }
        if record.metadata.wing:
            meta["wing"] = record.metadata.wing
        if record.metadata.room:
            meta["room"] = record.metadata.room
        vs.index_memory(slug, text, meta)

    def _ensure_search_index(self) -> None:
        """Lazily build the TF-IDF search index on first use."""
        if self._search_index_built:
            return
        try:
            from .search import WikiSearchEngine
            self._search_engine = WikiSearchEngine()
            for record in self.list_memories("private"):
                text = " ".join([
                    record.metadata.name,
                    record.metadata.description,
                    " ".join(record.metadata.tags),
                    record.metadata.memory_type,
                    record.metadata.page_type,
                    record.body,
                ])
                self._search_engine.index_document(record.metadata.slug, text)
            for record in self.list_memories("team"):
                text = " ".join([record.metadata.name, record.metadata.description, record.body])
                self._search_engine.index_document(f"team:{record.metadata.slug}", text)
            self._search_index_built = True
        except ImportError:
            self._search_engine = None
            self._search_index_built = True

    def _index_document_in_search(self, slug: str, record: MemoryRecord) -> None:
        """Update a single document in the search index after save."""
        if self._search_engine is None:
            return
        text = " ".join([
            record.metadata.name,
            record.metadata.description,
            " ".join(record.metadata.tags),
            record.metadata.memory_type,
            record.metadata.page_type,
            record.body,
        ])
        self._search_engine.index_document(slug, text)

    def recall(
        self,
        query: str,
        *,
        include_team: bool = True,
        limit: int = 5,
    ) -> MemoryRecallResult:
        """Search using hybrid vector + TF-IDF + recency + importance scoring."""
        self._ensure_search_index()

        candidates = self.list_memories("private")
        if include_team:
            candidates.extend(self.list_memories("team"))

        # Build slug -> record lookup
        slug_map: dict[str, MemoryRecord] = {}
        for r in candidates:
            key = f"team:{r.metadata.slug}" if r.metadata.scope == "team" else r.metadata.slug
            slug_map[key] = r

        # ── Vector search (semantic, 0.7 weight) ──
        vector_scores: dict[str, float] = {}
        vs = self.vector_store
        if vs is not None:
            for slug, sim in vs.search_memories(query, limit=limit * 3):
                vector_scores[slug] = sim * 0.7

        # ── TF-IDF search (keyword, 0.3 weight) ──
        tfidf_scores: dict[str, float] = {}
        if self._search_engine is not None and self._search_engine.document_count > 0:
            for slug, score in self._search_engine.search(query, limit=limit * 3):
                tfidf_scores[slug] = score
        else:
            for r in candidates:
                tfidf_scores[r.metadata.slug] = self._score_memory(r, query)

        # Normalize TF-IDF scores to 0-1 range
        max_tfidf = max(tfidf_scores.values()) if tfidf_scores else 1.0
        if max_tfidf > 0:
            tfidf_scores = {k: v / max_tfidf for k, v in tfidf_scores.items()}

        # ── Combined scoring ──
        def combined_score(record: MemoryRecord) -> float:
            slug = record.metadata.slug
            if record.metadata.scope == "team":
                slug = f"team:{slug}"
            vec = vector_scores.get(slug, 0.0)
            tfidf = tfidf_scores.get(slug, 0.0) * 0.3
            recency = 0.1 if record.freshness in ("today", "yesterday") else 0.0
            importance = record.metadata.importance_score * 0.15
            return vec + tfidf + recency + importance

        ranked = sorted(candidates, key=combined_score, reverse=True)
        selected = [r for r in ranked if combined_score(r) > 0][:limit]

        # Track access for aging/eviction
        now = iso_now()
        for record in selected:
            self._touch_access(record, now)

        return MemoryRecallResult(query=query, memories=selected)

    def _touch_access(self, record: MemoryRecord, now: str) -> None:
        """Update last_accessed_at and access_count without rewriting the full file."""
        path = Path(record.path)
        if not path.exists():
            return
        try:
            raw = path.read_text(encoding="utf-8")
            metadata_map, body = parse_memory_file(raw)
            count = int(metadata_map.get("access_count", "0") or "0") + 1
            metadata_map["last_accessed_at"] = now
            metadata_map["access_count"] = str(count)
            # Rebuild frontmatter
            fm_lines = ["---"]
            for key, val in metadata_map.items():
                fm_lines.append(f"{key}: {val}")
            fm_lines.append("---")
            fm_lines.append("")
            path.write_text("\n".join(fm_lines) + body, encoding="utf-8")
        except Exception:
            pass  # Non-critical — don't fail recall because of access tracking

    def render_system_prompt(
        self,
        *,
        query: str = "",
        include_team: bool = True,
        relevant_limit: int = 5,
    ) -> str:
        relevant = self.recall(query, include_team=include_team, limit=relevant_limit)
        context = {
            "memory_dir": str(self.root_dir),
            "private_index": self.entrypoint_path("private").read_text(encoding="utf-8").strip() or "_No private memories yet._",
            "team_index": self.entrypoint_path("team").read_text(encoding="utf-8").strip() or "_No team memories yet._",
            "include_team": include_team,
            "relevant_memories": relevant.memories,
        }
        if Template is not None:
            return Template(SYSTEM_PROMPT_TEMPLATE).render(**context).strip()
        text = SYSTEM_PROMPT_TEMPLATE
        text = text.replace("{{ memory_dir }}", context["memory_dir"])
        text = text.replace("{{ private_index }}", context["private_index"])
        text = text.replace("{{ team_index }}", context["team_index"])
        if not include_team:
            text = re.sub(r"\n\{% if include_team %}.*?\{% endif %}", "", text, flags=re.S)
        if relevant.memories:
            lines = []
            for memory in relevant.memories:
                lines.append(
                    f"- [{memory.metadata.memory_type}][{memory.metadata.scope}] {memory.metadata.name}: "
                    f"{memory.metadata.description or 'No description'}\n"
                    f"  Path: {memory.path}\n"
                    f"  Freshness: {memory.freshness}"
                )
            text = text.replace(
                "{% if relevant_memories %}\n## Relevant memories for this request\n{% for memory in relevant_memories %}\n- [{{ memory.metadata.memory_type }}][{{ memory.metadata.scope }}] {{ memory.metadata.name }}: {{ memory.metadata.description or \"No description\" }}\n  Path: {{ memory.path }}\n  Freshness: {{ memory.freshness }}\n{% endfor %}\n{% endif %}",
                "## Relevant memories for this request\n" + "\n".join(lines),
            )
        else:
            text = text.replace(
                "{% if relevant_memories %}\n## Relevant memories for this request\n{% for memory in relevant_memories %}\n- [{{ memory.metadata.memory_type }}][{{ memory.metadata.scope }}] {{ memory.metadata.name }}: {{ memory.metadata.description or \"No description\" }}\n  Path: {{ memory.path }}\n  Freshness: {{ memory.freshness }}\n{% endfor %}\n{% endif %}",
                "",
            )
        text = text.replace("{% if include_team %}", "").replace("{% endif %}", "")
        text = text.replace("{% for memory in relevant_memories %}", "").replace("{% endfor %}", "")
        return text.strip()

    def _update_importance(self, record: MemoryRecord) -> None:
        """Compute and persist the importance score for a memory."""
        try:
            from .layers import compute_importance
            score = compute_importance(record)
            if abs(score - record.metadata.importance_score) < 0.01:
                return  # No meaningful change
            path = Path(record.path)
            if not path.exists():
                return
            raw = path.read_text(encoding="utf-8")
            metadata_map, body = parse_memory_file(raw)
            metadata_map["importance_score"] = str(score)
            fm_lines = ["---"] + [f"{k}: {v}" for k, v in metadata_map.items()] + ["---", ""]
            path.write_text("\n".join(fm_lines) + body, encoding="utf-8")
        except Exception:
            pass  # Non-critical

    # ── Wing/Room queries ──────────────────────────────────────────────

    def list_wings(self) -> list[dict]:
        """All wings with memory counts."""
        from .graph import MemoryGraph
        return MemoryGraph(self).list_wings()

    def list_rooms(self, wing: str) -> list[dict]:
        """All rooms in a wing."""
        from .graph import MemoryGraph
        return MemoryGraph(self).list_rooms(wing)

    # ── Wiki graph queries ───────────────────────────────────────────────

    def list_by_tag(self, tag: str) -> list[MemoryRecord]:
        """Return all memories that have the given tag."""
        tag_lower = tag.lower()
        return [
            r for r in self.list_memories("private")
            if tag_lower in [t.lower() for t in r.metadata.tags]
        ]

    def list_by_page_type(self, page_type: str) -> list[MemoryRecord]:
        """Return all memories of a given page type (entity, concept, etc.)."""
        return [
            r for r in self.list_memories("private")
            if r.metadata.page_type == page_type
        ]

    def list_orphan_memories(self, min_age_days: int = 30) -> list[MemoryRecord]:
        """Memories with no backlinks AND low access count AND older than min_age_days."""
        now_ms = datetime.now(timezone.utc).timestamp() * 1000
        threshold_ms = min_age_days * 86_400_000
        return [
            r for r in self.list_memories("private")
            if not r.metadata.backlinks
            and r.metadata.access_count < 2
            and (now_ms - r.mtime_ms) > threshold_ms
        ]

    def list_broken_references(self) -> list[tuple[str, list[str]]]:
        """Return (slug, [broken_ref_slugs]) for pages with [[slug]] links to non-existent pages."""
        all_slugs = {r.metadata.slug for r in self.list_memories("private")}
        broken: list[tuple[str, list[str]]] = []
        for record in self.list_memories("private"):
            bad = [ref for ref in record.metadata.references if ref not in all_slugs]
            if bad:
                broken.append((record.metadata.slug, bad))
        return broken

    def list_stale_memories(self, days: int = 60) -> list[MemoryRecord]:
        """Memories not accessed in `days` days (based on last_accessed_at metadata)."""
        now = datetime.now(timezone.utc)
        threshold_secs = days * 86_400
        results = []
        for r in self.list_memories("private"):
            accessed_at = r.metadata.last_accessed_at
            if accessed_at:
                try:
                    last = datetime.fromisoformat(accessed_at.replace("Z", "+00:00"))
                    if (now - last).total_seconds() > threshold_secs:
                        results.append(r)
                except ValueError:
                    # Unparseable date — fall back to mtime
                    now_ms = now.timestamp() * 1000
                    if (now_ms - r.mtime_ms) > threshold_secs * 1000:
                        results.append(r)
            else:
                # No last_accessed_at recorded — use mtime as proxy
                now_ms = now.timestamp() * 1000
                if (now_ms - r.mtime_ms) > threshold_secs * 1000:
                    results.append(r)
        return results

    def list_missing_pages(self) -> list[str]:
        """Slugs referenced via [[slug]] in 2+ pages but lacking their own page."""
        from collections import Counter
        all_slugs = {r.metadata.slug for r in self.list_memories("private")}
        ref_counts: Counter[str] = Counter()
        for record in self.list_memories("private"):
            for ref in record.metadata.references:
                if ref not in all_slugs:
                    ref_counts[ref] += 1
        return [slug for slug, count in ref_counts.items() if count >= 2]

    # ── Backlink maintenance ────────────────────────────────────────────

    def _update_cross_references(
        self, source_slug: str, old_refs: list[str], new_refs: list[str]
    ) -> None:
        """Update backlinks on referenced pages when a page's references change."""
        added = set(new_refs) - set(old_refs)
        removed = set(old_refs) - set(new_refs)

        for ref_slug in added:
            self._add_backlink(ref_slug, source_slug)
        for ref_slug in removed:
            self._remove_backlink(ref_slug, source_slug)

    def _add_backlink(self, target_slug: str, source_slug: str) -> None:
        """Add source_slug to target's backlinks list."""
        record = self.get_memory(target_slug)
        if record is None:
            return
        if source_slug in record.metadata.backlinks:
            return
        path = Path(record.path)
        try:
            raw = path.read_text(encoding="utf-8")
            metadata_map, body = parse_memory_file(raw)
            existing_bl = metadata_map.get("backlinks", "")
            bl_list = [b.strip() for b in existing_bl.split(",") if b.strip()]
            if source_slug not in bl_list:
                bl_list.append(source_slug)
            metadata_map["backlinks"] = ", ".join(bl_list)
            fm = ["---"] + [f"{k}: {v}" for k, v in metadata_map.items()] + ["---", ""]
            path.write_text("\n".join(fm) + body, encoding="utf-8")
        except Exception:
            pass  # Non-critical

    def _remove_backlink(self, target_slug: str, source_slug: str) -> None:
        """Remove source_slug from target's backlinks list."""
        record = self.get_memory(target_slug)
        if record is None:
            return
        path = Path(record.path)
        try:
            raw = path.read_text(encoding="utf-8")
            metadata_map, body = parse_memory_file(raw)
            existing_bl = metadata_map.get("backlinks", "")
            bl_list = [b.strip() for b in existing_bl.split(",") if b.strip()]
            if source_slug in bl_list:
                bl_list.remove(source_slug)
            metadata_map["backlinks"] = ", ".join(bl_list) if bl_list else ""
            fm = ["---"] + [f"{k}: {v}" for k, v in metadata_map.items()] + ["---", ""]
            path.write_text("\n".join(fm) + body, encoding="utf-8")
        except Exception:
            pass  # Non-critical

    def _rewrite_entrypoint(self, scope: MemoryScope) -> None:
        """Legacy entrypoint rewriter — delegates to _rewrite_all_indexes."""
        self._rewrite_all_indexes()

    @staticmethod
    def _format_index_entry(record: MemoryRecord) -> str:
        """Format a single index entry with tags and page_type."""
        desc = record.metadata.description or summarize_text(record.body, 80)
        suffix_parts: list[str] = []
        if record.metadata.page_type:
            suffix_parts.append(record.metadata.page_type)
        if record.metadata.source_type:
            suffix_parts.append(record.metadata.source_type)
        tags_str = " ".join(f"#{t}" for t in record.metadata.tags[:4])
        entry = f"- [{record.metadata.name}]({Path(record.path).name}) — {desc}"
        if tags_str:
            entry += f" {tags_str}"
        if suffix_parts:
            entry += f" ({', '.join(suffix_parts)})"
        return entry

    def _rewrite_all_indexes(self) -> None:
        """Rewrite all index files: core/MEMORY.md, archive/ARCHIVE_INDEX.md, and legacy."""
        # Core index (this is what goes into the system prompt — keep it small)
        core_records = self.list_memories_by_tier("core")
        core_lines = [self._format_index_entry(r) for r in sorted(core_records, key=lambda m: m.metadata.name.lower())]
        (self.core_dir / ENTRYPOINT_NAME).write_text(
            "\n".join(core_lines) + ("\n" if core_lines else ""), encoding="utf-8"
        )

        # Archive index
        archive_records = self.list_memories_by_tier("archive")
        archive_lines = [self._format_index_entry(r) for r in sorted(archive_records, key=lambda m: m.metadata.name.lower())]
        (self.archive_dir / ARCHIVE_INDEX_NAME).write_text(
            "\n".join(archive_lines) + ("\n" if archive_lines else ""), encoding="utf-8"
        )

        # Legacy top-level MEMORY.md (combined, for backwards compat)
        all_lines = []
        if core_lines:
            all_lines.append("## Core Memories")
            all_lines.extend(core_lines)
        if archive_lines:
            all_lines.append("")
            all_lines.append(f"## Archive ({len(archive_lines)} memories — use MemoryRecall tool to search)")
            # Only show first 5 archive entries in the combined index
            all_lines.extend(archive_lines[:5])
            if len(archive_lines) > 5:
                all_lines.append(f"  ... and {len(archive_lines) - 5} more (searchable via MemoryRecall)")
        self.entrypoint_path("private").write_text(
            "\n".join(all_lines) + ("\n" if all_lines else ""), encoding="utf-8"
        )

        # Team index (unchanged logic)
        team_records = self.list_memories("team")
        team_lines = []
        for record in sorted(team_records, key=lambda m: m.metadata.name.lower()):
            desc = record.metadata.description or summarize_text(record.body, 80)
            team_lines.append(f"- [{record.metadata.name}]({Path(record.path).name}) — {desc}")
        self.entrypoint_path("team").write_text(
            "\n".join(team_lines) + ("\n" if team_lines else ""), encoding="utf-8"
        )

    def _load_record(self, path: Path, scope: MemoryScope) -> MemoryRecord | None:
        raw = path.read_text(encoding="utf-8")
        metadata_map, body = parse_memory_file(raw)
        memory_type = str(metadata_map.get("type") or "").strip().lower()
        if memory_type not in MEMORY_TYPES:
            return None
        stat = path.stat()

        # Infer tier from directory
        tier: MemoryTier = "archive"
        if self.core_dir in path.parents or path.parent == self.core_dir:
            tier = "core"

        # Parse list fields (comma-separated strings in frontmatter)
        def _parse_list(val: str) -> list[str]:
            if not val:
                return []
            return [item.strip() for item in val.split(",") if item.strip()]

        metadata = MemoryMetadata(
            name=str(metadata_map.get("name") or path.stem.replace("-", " ").title()),
            description=str(metadata_map.get("description") or ""),
            memory_type=memory_type,  # type: ignore[arg-type]
            scope=scope,
            tier=tier,
            created_at=str(metadata_map.get("created_at") or ""),
            updated_at=str(metadata_map.get("updated_at") or ""),
            last_accessed_at=str(metadata_map.get("last_accessed_at") or ""),
            access_count=int(metadata_map.get("access_count", "0") or "0"),
            pinned=str(metadata_map.get("pinned", "")).lower() in ("true", "1", "yes"),
            slug=str(metadata_map.get("slug") or path.stem),
            tags=_parse_list(metadata_map.get("tags", "")),
            source_url=str(metadata_map.get("source_url") or ""),
            source_type=str(metadata_map.get("source_type") or ""),
            references=_parse_list(metadata_map.get("references", "")),
            backlinks=_parse_list(metadata_map.get("backlinks", "")),
            page_type=str(metadata_map.get("page_type") or ""),
            wing=str(metadata_map.get("wing") or ""),
            room=str(metadata_map.get("room") or ""),
            importance_score=float(metadata_map.get("importance_score", "0") or "0"),
        )
        return MemoryRecord(
            metadata=metadata,
            body=body.strip(),
            path=str(path),
            mtime_ms=stat.st_mtime * 1000,
        )

    def _score_memory(self, record: MemoryRecord, query: str) -> float:
        if not query.strip():
            return 0.0
        query_terms = tokenize(query)
        if not query_terms:
            return 0.0
        haystack = " ".join(
            [
                record.metadata.name,
                record.metadata.description,
                record.metadata.memory_type,
                record.body,
            ]
        ).lower()
        overlap = sum(2 if term in record.metadata.name.lower() else 1 for term in query_terms if term in haystack)
        recency_bonus = 0.25 if record.freshness in {"today", "yesterday"} else 0.0
        return overlap + recency_bonus


def parse_memory_file(raw: str) -> tuple[dict[str, str], str]:
    stripped = raw.strip()
    if not stripped.startswith("---"):
        return {}, raw
    lines = raw.splitlines()
    metadata: dict[str, str] = {}
    end_index = None
    for index in range(1, len(lines)):
        line = lines[index]
        if line.strip() == "---":
            end_index = index
            break
        if ":" in line:
            key, value = line.split(":", 1)
            metadata[key.strip()] = value.strip()
    if end_index is None:
        return {}, raw
    body = "\n".join(lines[end_index + 1 :]).lstrip("\n")
    return metadata, body


def serialize_memory_file(metadata: MemoryMetadata, body: str) -> str:
    frontmatter_lines = [
        "---",
        f"name: {metadata.name}",
        f"description: {metadata.description}",
        f"type: {metadata.memory_type}",
        f"scope: {metadata.scope}",
        f"tier: {metadata.tier}",
        f"created_at: {metadata.created_at}",
        f"updated_at: {metadata.updated_at}",
        f"last_accessed_at: {metadata.last_accessed_at}",
        f"access_count: {metadata.access_count}",
        f"pinned: {metadata.pinned}",
        f"slug: {metadata.slug}",
    ]
    # Wiki fields — only write if non-empty to keep files clean
    if metadata.tags:
        frontmatter_lines.append(f"tags: {', '.join(metadata.tags)}")
    if metadata.source_url:
        frontmatter_lines.append(f"source_url: {metadata.source_url}")
    if metadata.source_type:
        frontmatter_lines.append(f"source_type: {metadata.source_type}")
    if metadata.references:
        frontmatter_lines.append(f"references: {', '.join(metadata.references)}")
    if metadata.backlinks:
        frontmatter_lines.append(f"backlinks: {', '.join(metadata.backlinks)}")
    if metadata.page_type:
        frontmatter_lines.append(f"page_type: {metadata.page_type}")
    if metadata.wing:
        frontmatter_lines.append(f"wing: {metadata.wing}")
    if metadata.room:
        frontmatter_lines.append(f"room: {metadata.room}")
    if metadata.importance_score > 0:
        frontmatter_lines.append(f"importance_score: {metadata.importance_score}")
    frontmatter_lines.extend(["---", ""])
    return "\n".join(frontmatter_lines) + body.strip() + "\n"


def slugify(text: str) -> str:
    slug = re.sub(r"[^a-zA-Z0-9]+", "-", text.strip().lower()).strip("-")
    return slug or "memory"


def tokenize(text: str) -> list[str]:
    return [token for token in re.findall(r"[a-zA-Z0-9_./:-]+", text.lower()) if len(token) > 2]


def iso_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat()


def summarize_text(text: str, max_length: int = 120) -> str:
    collapsed = " ".join(text.strip().split())
    if len(collapsed) <= max_length:
        return collapsed
    return collapsed[: max_length - 3].rstrip() + "..."
