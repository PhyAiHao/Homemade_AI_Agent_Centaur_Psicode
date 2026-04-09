"""ChromaDB-backed semantic vector search for the memory system.

Provides embedding-based retrieval that understands synonyms and paraphrases,
unlike TF-IDF which only matches exact keywords. ChromaDB uses the
all-MiniLM-L6-v2 model (384 dims) by default — runs locally, no API calls.

Falls back gracefully if chromadb is not installed.
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)

# Lazy-loaded to avoid import cost when vector search is unused
_chromadb = None
_chromadb_available: bool | None = None


def _ensure_chromadb() -> bool:
    """Check if chromadb is available. Caches the result."""
    global _chromadb, _chromadb_available
    if _chromadb_available is None:
        try:
            import chromadb

            _chromadb = chromadb
            _chromadb_available = True
        except ImportError:
            _chromadb_available = False
    return _chromadb_available


class VectorStore:
    """ChromaDB-backed semantic search for memory pages and raw chunks."""

    COLLECTION_MEMORIES = "agent_memories"
    COLLECTION_CHUNKS = "agent_chunks"

    def __init__(self, persist_dir: Path) -> None:
        self._persist_dir = persist_dir
        self._client: Any | None = None
        self._mem_collection: Any | None = None
        self._chunk_collection: Any | None = None
        self._available = _ensure_chromadb()

    @property
    def available(self) -> bool:
        return self._available

    def _ensure_client(self) -> bool:
        """Lazily create the ChromaDB client and collections."""
        if not self._available:
            return False
        if self._client is not None:
            return True
        try:
            self._persist_dir.mkdir(parents=True, exist_ok=True)
            self._client = _chromadb.PersistentClient(
                path=str(self._persist_dir)
            )
            self._mem_collection = self._client.get_or_create_collection(
                name=self.COLLECTION_MEMORIES,
                metadata={"hnsw:space": "cosine"},
            )
            self._chunk_collection = self._client.get_or_create_collection(
                name=self.COLLECTION_CHUNKS,
                metadata={"hnsw:space": "cosine"},
            )
            return True
        except Exception as e:
            logger.warning("ChromaDB init failed: %s", e)
            self._available = False
            return False

    # ── Memory documents ───────────────────────────────────────────────

    def index_memory(
        self,
        slug: str,
        text: str,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Upsert a memory document into the vector store."""
        if not self._ensure_client():
            return
        meta = metadata or {}
        # ChromaDB metadata values must be str, int, float, or bool
        safe_meta = {
            k: v
            for k, v in meta.items()
            if isinstance(v, (str, int, float, bool))
        }
        try:
            self._mem_collection.upsert(
                ids=[slug],
                documents=[text],
                metadatas=[safe_meta],
            )
        except Exception as e:
            logger.warning("VectorStore.index_memory failed for %s: %s", slug, e)

    def remove_memory(self, slug: str) -> None:
        """Remove a memory from the vector store."""
        if not self._ensure_client():
            return
        try:
            self._mem_collection.delete(ids=[slug])
        except Exception as e:
            logger.debug("VectorStore.remove_memory: %s", e)

    def search_memories(
        self,
        query: str,
        limit: int = 10,
        where: dict[str, Any] | None = None,
    ) -> list[tuple[str, float]]:
        """Semantic search on memory documents.

        Returns (slug, similarity) pairs where similarity is 1 - cosine_distance.
        """
        if not self._ensure_client():
            return []
        try:
            kwargs: dict[str, Any] = {
                "query_texts": [query],
                "n_results": min(limit, self._mem_collection.count() or 1),
                "include": ["distances"],
            }
            if where:
                kwargs["where"] = where
            results = self._mem_collection.query(**kwargs)
            ids = results.get("ids", [[]])[0]
            distances = results.get("distances", [[]])[0]
            return [
                (slug, round(1.0 - dist, 4))
                for slug, dist in zip(ids, distances)
                if dist < 1.0  # Filter out totally irrelevant
            ]
        except Exception as e:
            logger.warning("VectorStore.search_memories failed: %s", e)
            return []

    # ── Raw chunks ─────────────────────────────────────────────────────

    def index_chunk(
        self,
        chunk_id: str,
        text: str,
        metadata: dict[str, Any] | None = None,
    ) -> None:
        """Upsert a raw text chunk into the chunk collection."""
        if not self._ensure_client():
            return
        meta = metadata or {}
        safe_meta = {
            k: v
            for k, v in meta.items()
            if isinstance(v, (str, int, float, bool))
        }
        try:
            self._chunk_collection.upsert(
                ids=[chunk_id],
                documents=[text],
                metadatas=[safe_meta],
            )
        except Exception as e:
            logger.warning("VectorStore.index_chunk failed for %s: %s", chunk_id, e)

    def remove_chunks_by_source(self, source_slug: str) -> None:
        """Remove all chunks associated with a source slug."""
        if not self._ensure_client():
            return
        try:
            self._chunk_collection.delete(
                where={"source_slug": source_slug}
            )
        except Exception as e:
            logger.debug("VectorStore.remove_chunks_by_source: %s", e)

    def search_chunks(
        self,
        query: str,
        limit: int = 10,
        where: dict[str, Any] | None = None,
    ) -> list[tuple[str, float, str]]:
        """Semantic search on raw text chunks.

        Returns (chunk_id, similarity, document_text) triples.
        """
        if not self._ensure_client():
            return []
        try:
            count = self._chunk_collection.count()
            if count == 0:
                return []
            kwargs: dict[str, Any] = {
                "query_texts": [query],
                "n_results": min(limit, count),
                "include": ["distances", "documents"],
            }
            if where:
                kwargs["where"] = where
            results = self._chunk_collection.query(**kwargs)
            ids = results.get("ids", [[]])[0]
            distances = results.get("distances", [[]])[0]
            documents = results.get("documents", [[]])[0]
            return [
                (cid, round(1.0 - dist, 4), doc)
                for cid, dist, doc in zip(ids, distances, documents)
                if dist < 1.0
            ]
        except Exception as e:
            logger.warning("VectorStore.search_chunks failed: %s", e)
            return []

    # ── Duplicate detection ────────────────────────────────────────────

    def check_duplicate(
        self, text: str, threshold: float = 0.9
    ) -> list[dict[str, Any]]:
        """Check if similar content already exists.

        Returns matches with similarity >= threshold.
        """
        if not self._ensure_client():
            return []
        try:
            count = self._mem_collection.count()
            if count == 0:
                return []
            results = self._mem_collection.query(
                query_texts=[text],
                n_results=min(5, count),
                include=["distances", "metadatas"],
            )
            ids = results.get("ids", [[]])[0]
            distances = results.get("distances", [[]])[0]
            metadatas = results.get("metadatas", [[]])[0]
            matches = []
            for slug, dist, meta in zip(ids, distances, metadatas):
                similarity = 1.0 - dist
                if similarity >= threshold:
                    matches.append(
                        {"slug": slug, "similarity": round(similarity, 4), "metadata": meta}
                    )
            return matches
        except Exception as e:
            logger.warning("VectorStore.check_duplicate failed: %s", e)
            return []

    # ── Stats ──────────────────────────────────────────────────────────

    def stats(self) -> dict[str, int]:
        """Return counts of indexed documents."""
        if not self._ensure_client():
            return {"memories": 0, "chunks": 0, "available": False}
        return {
            "memories": self._mem_collection.count(),
            "chunks": self._chunk_collection.count(),
            "available": True,
        }
