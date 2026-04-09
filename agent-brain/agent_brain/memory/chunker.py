"""Verbatim text chunker for the memory system.

Splits source text into overlapping chunks for embedding-based retrieval.
MemPalace's 96.6% LongMemEval score comes from storing raw verbatim chunks
rather than LLM-generated summaries. This module enables the same approach.

Chunks are stored in the VectorStore (ChromaDB) only — not as markdown files.
"""

from __future__ import annotations

import hashlib


CHUNK_SIZE = 800       # Characters per chunk
CHUNK_OVERLAP = 100    # Overlap between consecutive chunks
MIN_CHUNK_SIZE = 50    # Skip tiny fragments


def chunk_text(
    text: str,
    source_slug: str,
    *,
    chunk_size: int = CHUNK_SIZE,
    overlap: int = CHUNK_OVERLAP,
) -> list[dict]:
    """Split text into overlapping chunks with metadata.

    Returns list of dicts:
        {"id": "chunk_{slug}_{i}", "text": "...",
         "source_slug": slug, "chunk_index": i}
    """
    if not text or not text.strip():
        return []

    # Split on paragraph boundaries first for cleaner breaks
    paragraphs = text.split("\n\n")
    chunks: list[dict] = []
    current = ""
    chunk_index = 0

    for para in paragraphs:
        para = para.strip()
        if not para:
            continue

        if len(current) + len(para) + 2 <= chunk_size:
            current = (current + "\n\n" + para).strip() if current else para
        else:
            # Current chunk is full — flush it
            if len(current) >= MIN_CHUNK_SIZE:
                chunk_id = _make_chunk_id(source_slug, chunk_index)
                chunks.append({
                    "id": chunk_id,
                    "text": current,
                    "source_slug": source_slug,
                    "chunk_index": chunk_index,
                })
                chunk_index += 1

            # Start new chunk with overlap from end of previous
            if overlap > 0 and current:
                tail = current[-overlap:]
                current = tail + "\n\n" + para
            else:
                current = para

            # If this single paragraph exceeds chunk_size, split it further
            while len(current) > chunk_size:
                segment = current[:chunk_size]
                # Try to break at a line boundary
                last_nl = segment.rfind("\n")
                if last_nl > chunk_size // 2:
                    segment = segment[:last_nl]

                if len(segment) >= MIN_CHUNK_SIZE:
                    chunk_id = _make_chunk_id(source_slug, chunk_index)
                    chunks.append({
                        "id": chunk_id,
                        "text": segment,
                        "source_slug": source_slug,
                        "chunk_index": chunk_index,
                    })
                    chunk_index += 1

                current = current[len(segment):].strip()

    # Flush final chunk
    if len(current) >= MIN_CHUNK_SIZE:
        chunk_id = _make_chunk_id(source_slug, chunk_index)
        chunks.append({
            "id": chunk_id,
            "text": current,
            "source_slug": source_slug,
            "chunk_index": chunk_index,
        })

    return chunks


def _make_chunk_id(source_slug: str, index: int) -> str:
    """Generate a deterministic chunk ID."""
    raw = f"{source_slug}:{index}"
    h = hashlib.sha256(raw.encode()).hexdigest()[:12]
    return f"chunk_{source_slug}_{h}"
