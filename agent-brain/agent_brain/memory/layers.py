"""4-Layer memory stack — structured retrieval with ascending cost/scope.

Inspired by MemPalace's wake-up architecture:
  L0 Identity   (~100 tokens) — always in system prompt
  L1 Essentials  (~800 tokens) — top-scored by importance, always present
  L2 On-Demand  (~2K tokens)  — topic-filtered retrieval on first message
  L3 Deep Search (unlimited)  — full semantic search via tool call

This replaces the ad-hoc M5 (MEMORY.md dump) + M4 (prefetch) approach with
a coherent, debuggable layered model.
"""

from __future__ import annotations

import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from .memdir import MemoryRecord, MemoryStore
    from .vector_store import VectorStore

logger = logging.getLogger(__name__)

# L1 tuning constants
L1_MAX_MEMORIES = 15
L1_MAX_CHARS = 3200
L1_PREVIEW_CHARS = 200

# L2 tuning
L2_MAX_CHARS = 6000

# Importance scoring weights
TYPE_WEIGHTS = {
    "user": 5.0,
    "feedback": 4.0,
    "project": 3.0,
    "reference": 2.0,
}


def compute_importance(record: "MemoryRecord") -> float:
    """Multi-factor importance score normalized to 0-1.

    Factors:
      - type_weight: user=5, feedback=4, project=3, reference=2
      - recency_bonus: +1.0 if <7 days, +0.5 if <30 days
      - access_frequency: min(access_count / 10, 1.0)
      - backlink_weight: min(len(backlinks) / 3, 1.0)
      - pinned_bonus: +2.0 if pinned
    """
    meta = record.metadata

    type_w = TYPE_WEIGHTS.get(meta.memory_type, 2.0)

    # Recency from last_accessed_at
    recency = 0.0
    if meta.last_accessed_at:
        try:
            now = datetime.now(timezone.utc)
            last = datetime.fromisoformat(meta.last_accessed_at.replace("Z", "+00:00"))
            days = (now - last).total_seconds() / 86_400
            if days < 7:
                recency = 1.0
            elif days < 30:
                recency = 0.5
        except ValueError:
            pass

    access_freq = min(meta.access_count / 10.0, 1.0)
    backlink_w = min(len(meta.backlinks) / 3.0, 1.0)
    pinned = 2.0 if meta.pinned else 0.0

    raw = type_w + recency + access_freq + backlink_w + pinned
    return round(raw / 10.0, 3)  # Normalize to 0-1


class MemoryStack:
    """4-layer memory retrieval with ascending cost/scope."""

    def __init__(
        self,
        store: "MemoryStore",
        vector: "VectorStore | None" = None,
    ) -> None:
        self.store = store
        self.vector = vector

    # ── L0: Identity ───────────────────────────────────────────────────

    def l0_identity(self) -> str:
        """~100 token identity context. Always in system prompt."""
        identity_path = self.store.core_dir / "identity.md"
        if identity_path.exists():
            content = identity_path.read_text(encoding="utf-8").strip()
            # Strip frontmatter if present
            if content.startswith("---"):
                parts = content.split("---", 2)
                if len(parts) >= 3:
                    content = parts[2].strip()
            return content[:500]  # Hard cap for safety
        return ""

    # ── L1: Essentials ─────────────────────────────────────────────────

    def l1_essentials(self, max_chars: int = L1_MAX_CHARS) -> str:
        """Top-scored memories by importance. Always in system prompt."""
        all_memories = self.store.list_memories("private")
        if not all_memories:
            return "_No memories yet._"

        # Score and sort by importance
        scored = [(compute_importance(r), r) for r in all_memories]
        scored.sort(key=lambda x: x[0], reverse=True)

        lines: list[str] = []
        total_chars = 0
        count = 0

        for score, record in scored:
            if count >= L1_MAX_MEMORIES:
                break
            meta = record.metadata
            tags_str = f" #{' #'.join(meta.tags[:3])}" if meta.tags else ""
            entry = (
                f"- [{meta.memory_type}] **{meta.name}**"
                f" — {meta.description or record.body[:80]}"
                f"{tags_str}"
                f" (importance: {score:.2f})"
            )
            if total_chars + len(entry) > max_chars:
                break
            lines.append(entry)
            total_chars += len(entry)
            count += 1

        return "\n".join(lines) if lines else "_No essential memories._"

    # ── L2: On-Demand ──────────────────────────────────────────────────

    def l2_on_demand(
        self,
        topic: str,
        wing: str | None = None,
        max_chars: int = L2_MAX_CHARS,
    ) -> str:
        """Contextually relevant memories for current topic."""
        # Prefer vector search if available
        if self.vector and self.vector.available:
            where = {"wing": wing} if wing else None
            vector_hits = self.vector.search_memories(topic, limit=10, where=where)
            slug_scores = {slug: score for slug, score in vector_hits}
        else:
            slug_scores = {}

        # Also do TF-IDF recall for coverage
        recall_result = self.store.recall(topic, limit=10)
        for r in recall_result.memories:
            if r.metadata.slug not in slug_scores:
                slug_scores[r.metadata.slug] = 0.3  # baseline for TF-IDF hit

        if not slug_scores:
            return ""

        # Fetch full records and sort by score
        parts: list[str] = []
        total_chars = 0
        ranked = sorted(slug_scores.items(), key=lambda x: x[1], reverse=True)

        for slug, score in ranked[:7]:
            record = self.store.get_memory(slug)
            if record is None:
                continue
            meta = record.metadata
            body_preview = record.body[:400]
            if len(record.body) > 400:
                body_preview += "..."
            entry = (
                f"### [{meta.memory_type}] {meta.name}\n"
                f"_{meta.description}_\n"
                f"{body_preview}\n"
            )
            if total_chars + len(entry) > max_chars:
                break
            parts.append(entry)
            total_chars += len(entry)

        return "\n---\n".join(parts)

    # ── L3: Deep Search ────────────────────────────────────────────────

    def l3_deep_search(
        self,
        query: str,
        limit: int = 10,
    ) -> str:
        """Full semantic search across all memory + raw chunks.

        Hybrid: vector similarity + TF-IDF keyword matching.
        """
        results: dict[str, float] = {}

        # Vector search on memories
        if self.vector and self.vector.available:
            for slug, score in self.vector.search_memories(query, limit=limit):
                results[slug] = score * 0.7  # vector weight

            # Also search raw chunks
            chunk_hits = self.vector.search_chunks(query, limit=limit)
            chunk_context: list[str] = []
            for chunk_id, score, doc_text in chunk_hits[:3]:
                chunk_context.append(f"[verbatim] {doc_text[:300]}")

        else:
            chunk_context = []

        # TF-IDF search
        recall_result = self.store.recall(query, limit=limit)
        for r in recall_result.memories:
            slug = r.metadata.slug
            existing = results.get(slug, 0.0)
            results[slug] = existing + 0.3  # TF-IDF boost

        # Format results
        ranked = sorted(results.items(), key=lambda x: x[1], reverse=True)
        parts: list[str] = []

        for slug, score in ranked[:limit]:
            record = self.store.get_memory(slug)
            if record is None:
                continue
            meta = record.metadata
            parts.append(
                f"**{meta.name}** (relevance: {score:.2f})\n"
                f"Type: {meta.memory_type} | Tags: {', '.join(meta.tags[:5])}\n"
                f"{record.body[:500]}\n"
            )

        # Append verbatim chunk hits
        if chunk_context:
            parts.append("\n### Verbatim source fragments\n" + "\n".join(chunk_context))

        return "\n---\n".join(parts) if parts else f"No results for: {query}"

    # ── Wake-up (L0 + L1 + optional L2) ───────────────────────────────

    def wake_up(self, topic: str | None = None) -> str:
        """Session start context. L0 + L1 + optional L2 if topic given."""
        parts: list[str] = []

        identity = self.l0_identity()
        if identity:
            parts.append(f"## Identity\n{identity}")

        essentials = self.l1_essentials()
        parts.append(f"## Essential Memory\n{essentials}")

        if topic:
            on_demand = self.l2_on_demand(topic)
            if on_demand:
                parts.append(f"## Relevant Context\n{on_demand}")

        return "\n\n".join(parts)

    # ── Status ─────────────────────────────────────────────────────────

    def status(self) -> dict:
        """Memory stack stats."""
        all_mems = self.store.list_memories("private")
        core = self.store.list_memories_by_tier("core")
        archive = self.store.list_memories_by_tier("archive")

        # Compute importance distribution
        scores = [compute_importance(r) for r in all_mems]
        avg_importance = sum(scores) / len(scores) if scores else 0.0

        result = {
            "total_memories": len(all_mems),
            "core": len(core),
            "archive": len(archive),
            "avg_importance": round(avg_importance, 3),
            "l1_candidates": min(L1_MAX_MEMORIES, len(all_mems)),
        }

        if self.vector and self.vector.available:
            result.update(self.vector.stats())

        return result
