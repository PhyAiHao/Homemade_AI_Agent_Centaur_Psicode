"""TF-IDF search engine for the wiki/memory system.

Zero external dependencies — uses pure Python math for TF-IDF scoring.
Replaces the naive keyword overlap scoring in MemoryStore._score_memory().

Usage:
    engine = WikiSearchEngine()
    engine.index_document("auth-migration", "auth migration OAuth2 tokens ...")
    results = engine.search("authentication tokens", limit=5)
    # -> [("auth-migration", 3.42), ...]
"""

from __future__ import annotations

import math
import re
from collections import Counter


# Stopwords — common English words that add noise to search
_STOPWORDS = frozenset({
    "a", "an", "the", "and", "or", "but", "in", "on", "at", "to", "for",
    "of", "with", "by", "from", "is", "it", "as", "be", "was", "are",
    "been", "has", "have", "had", "do", "does", "did", "will", "would",
    "could", "should", "may", "might", "can", "this", "that", "these",
    "those", "not", "no", "so", "if", "then", "than", "when", "where",
    "what", "which", "who", "how", "all", "each", "every", "both",
    "few", "more", "most", "other", "some", "such", "only", "own",
    "same", "too", "very", "just", "about", "also", "into", "over",
    "after", "before", "between", "under", "again", "further", "once",
})


def _tokenize(text: str) -> list[str]:
    """Lowercase, split on non-alphanumeric, remove stopwords, min length 2."""
    words = re.findall(r"[a-zA-Z0-9_]+", text.lower())
    return [w for w in words if len(w) >= 2 and w not in _STOPWORDS]


class WikiSearchEngine:
    """In-process TF-IDF search over memory/wiki files. Zero external deps."""

    def __init__(self) -> None:
        self._docs: dict[str, list[str]] = {}  # slug -> tokenized terms
        self._doc_term_counts: dict[str, Counter[str]] = {}  # slug -> term frequencies
        self._idf: dict[str, float] = {}  # term -> inverse document frequency
        self._dirty = True

    @property
    def document_count(self) -> int:
        return len(self._docs)

    def index_document(self, slug: str, text: str) -> None:
        """Add or update a document in the index."""
        tokens = _tokenize(text)
        self._docs[slug] = tokens
        self._doc_term_counts[slug] = Counter(tokens)
        self._dirty = True

    def remove_document(self, slug: str) -> None:
        """Remove a document from the index."""
        self._docs.pop(slug, None)
        self._doc_term_counts.pop(slug, None)
        self._dirty = True

    def search(self, query: str, limit: int = 10) -> list[tuple[str, float]]:
        """Return (slug, score) pairs ranked by TF-IDF relevance."""
        if self._dirty:
            self._recompute_idf()

        query_terms = _tokenize(query)
        if not query_terms:
            return []

        scores: dict[str, float] = {}
        for slug, term_counts in self._doc_term_counts.items():
            score = 0.0
            doc_len = len(self._docs[slug]) or 1
            for term in query_terms:
                tf = term_counts.get(term, 0) / doc_len  # normalized TF
                idf = self._idf.get(term, 0.0)
                score += tf * idf
            if score > 0:
                scores[slug] = score

        ranked = sorted(scores.items(), key=lambda x: x[1], reverse=True)
        return ranked[:limit]

    def _recompute_idf(self) -> None:
        """Recompute inverse document frequency for all terms."""
        n = len(self._docs)
        if n == 0:
            self._idf = {}
            self._dirty = False
            return

        # Count how many documents contain each term
        doc_freq: Counter[str] = Counter()
        for term_counts in self._doc_term_counts.values():
            for term in term_counts:
                doc_freq[term] += 1

        # IDF = log(N / df) + 1  (smoothed to avoid zero for common terms)
        self._idf = {
            term: math.log(n / df) + 1.0
            for term, df in doc_freq.items()
        }
        self._dirty = False
