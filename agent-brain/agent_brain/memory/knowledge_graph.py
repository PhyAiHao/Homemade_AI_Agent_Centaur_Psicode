"""Temporal knowledge graph — entity-relationship triples with time validity.

Stores facts like (Max, child_of, Alice, valid_from=2015, valid_to=None) in
SQLite. Enables time-aware queries: "What was true about X before the migration?"

Inspired by MemPalace's temporal KG but adapted for the wiki memory system.
"""

from __future__ import annotations

import hashlib
import json
import logging
import sqlite3
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


class KnowledgeGraph:
    """Temporal entity-relationship store backed by SQLite."""

    def __init__(self, db_path: Path) -> None:
        self.db_path = db_path
        db_path.parent.mkdir(parents=True, exist_ok=True)
        self._init_schema()

    def _conn(self) -> sqlite3.Connection:
        return sqlite3.connect(str(self.db_path))

    def _init_schema(self) -> None:
        conn = self._conn()
        conn.executescript("""
            CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT NOT NULL DEFAULT 'concept',
                properties TEXT DEFAULT '{}',
                created_at TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS triples (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL REFERENCES entities(id),
                predicate TEXT NOT NULL,
                object TEXT NOT NULL REFERENCES entities(id),
                valid_from TEXT,
                valid_to TEXT,
                confidence REAL DEFAULT 1.0,
                source_slug TEXT,
                extracted_at TEXT NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_triples_subject ON triples(subject);
            CREATE INDEX IF NOT EXISTS idx_triples_object ON triples(object);
            CREATE INDEX IF NOT EXISTS idx_triples_predicate ON triples(predicate);
            CREATE INDEX IF NOT EXISTS idx_triples_valid ON triples(valid_from, valid_to);
        """)
        conn.commit()
        conn.close()

    # ── Entity operations ──────────────────────────────────────────────

    def _entity_id(self, name: str) -> str:
        """Normalize name to ID: lowercase, spaces to underscores."""
        return name.strip().lower().replace(" ", "_").replace("-", "_")

    def add_entity(
        self,
        name: str,
        entity_type: str = "concept",
        properties: dict[str, Any] | None = None,
    ) -> str:
        """Create or get entity. Returns entity_id."""
        eid = self._entity_id(name)
        conn = self._conn()
        now = datetime.now(timezone.utc).isoformat()
        props_json = json.dumps(properties or {})
        conn.execute(
            """INSERT INTO entities (id, name, type, properties, created_at)
               VALUES (?, ?, ?, ?, ?)
               ON CONFLICT(id) DO UPDATE SET
                 type = CASE WHEN excluded.type != 'concept' THEN excluded.type ELSE entities.type END,
                 properties = excluded.properties""",
            (eid, name.strip(), entity_type, props_json, now),
        )
        conn.commit()
        conn.close()
        return eid

    def get_entity(self, name: str) -> dict[str, Any] | None:
        """Get entity by name. Returns None if not found."""
        eid = self._entity_id(name)
        conn = self._conn()
        row = conn.execute(
            "SELECT id, name, type, properties, created_at FROM entities WHERE id = ?",
            (eid,),
        ).fetchone()
        conn.close()
        if row is None:
            return None
        return {
            "id": row[0],
            "name": row[1],
            "type": row[2],
            "properties": json.loads(row[3]),
            "created_at": row[4],
        }

    # ── Triple operations ──────────────────────────────────────────────

    def add_triple(
        self,
        subject: str,
        predicate: str,
        obj: str,
        *,
        valid_from: str | None = None,
        valid_to: str | None = None,
        confidence: float = 1.0,
        source_slug: str | None = None,
        subject_type: str = "concept",
        object_type: str = "concept",
    ) -> str:
        """Add a fact triple. Auto-creates entities if needed. Returns triple_id."""
        sub_id = self.add_entity(subject, entity_type=subject_type)
        obj_id = self.add_entity(obj, entity_type=object_type)
        now = datetime.now(timezone.utc).isoformat()

        tid = f"t_{sub_id}_{predicate}_{obj_id}_{hashlib.sha256(now.encode()).hexdigest()[:8]}"

        conn = self._conn()
        conn.execute(
            """INSERT INTO triples (id, subject, predicate, object, valid_from,
               valid_to, confidence, source_slug, extracted_at)
               VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)""",
            (tid, sub_id, predicate, obj_id, valid_from, valid_to,
             confidence, source_slug, now),
        )
        conn.commit()
        conn.close()
        return tid

    def invalidate(
        self,
        subject: str,
        predicate: str,
        obj: str,
        *,
        ended: str | None = None,
    ) -> bool:
        """Mark a fact as no longer valid by setting valid_to."""
        sub_id = self._entity_id(subject)
        obj_id = self._entity_id(obj)
        ended = ended or datetime.now(timezone.utc).strftime("%Y-%m-%d")

        conn = self._conn()
        cursor = conn.execute(
            """UPDATE triples SET valid_to = ?
               WHERE subject = ? AND predicate = ? AND object = ?
               AND valid_to IS NULL""",
            (ended, sub_id, predicate, obj_id),
        )
        conn.commit()
        updated = cursor.rowcount > 0
        conn.close()
        return updated

    # ── Queries ────────────────────────────────────────────────────────

    def query_entity(
        self,
        name: str,
        *,
        as_of: str | None = None,
        direction: str = "both",
    ) -> list[dict[str, Any]]:
        """Get relationships for an entity, optionally at a point in time."""
        eid = self._entity_id(name)
        conn = self._conn()
        results: list[dict[str, Any]] = []

        if direction in ("out", "both"):
            query = """SELECT t.predicate, e.name as obj_name, t.valid_from,
                        t.valid_to, t.confidence, t.source_slug
                      FROM triples t JOIN entities e ON t.object = e.id
                      WHERE t.subject = ?"""
            params: list[Any] = [eid]
            if as_of:
                query += " AND (t.valid_from IS NULL OR t.valid_from <= ?)"
                query += " AND (t.valid_to IS NULL OR t.valid_to >= ?)"
                params.extend([as_of, as_of])
            for row in conn.execute(query, params).fetchall():
                results.append({
                    "direction": "outgoing",
                    "subject": name,
                    "predicate": row[0],
                    "object": row[1],
                    "valid_from": row[2],
                    "valid_to": row[3],
                    "current": row[3] is None,
                    "confidence": row[4],
                    "source_slug": row[5],
                })

        if direction in ("in", "both"):
            query = """SELECT t.predicate, e.name as sub_name, t.valid_from,
                        t.valid_to, t.confidence, t.source_slug
                      FROM triples t JOIN entities e ON t.subject = e.id
                      WHERE t.object = ?"""
            params = [eid]
            if as_of:
                query += " AND (t.valid_from IS NULL OR t.valid_from <= ?)"
                query += " AND (t.valid_to IS NULL OR t.valid_to >= ?)"
                params.extend([as_of, as_of])
            for row in conn.execute(query, params).fetchall():
                results.append({
                    "direction": "incoming",
                    "subject": row[1],
                    "predicate": row[0],
                    "object": name,
                    "valid_from": row[2],
                    "valid_to": row[3],
                    "current": row[3] is None,
                    "confidence": row[4],
                    "source_slug": row[5],
                })

        conn.close()
        return results

    def timeline(self, entity_name: str | None = None) -> list[dict[str, Any]]:
        """Chronological view of facts, optionally filtered to an entity."""
        conn = self._conn()
        if entity_name:
            eid = self._entity_id(entity_name)
            rows = conn.execute(
                """SELECT t.predicate, s.name, o.name, t.valid_from, t.valid_to,
                          t.confidence, t.source_slug
                   FROM triples t
                   JOIN entities s ON t.subject = s.id
                   JOIN entities o ON t.object = o.id
                   WHERE t.subject = ? OR t.object = ?
                   ORDER BY t.valid_from IS NULL, t.valid_from ASC
                   LIMIT 100""",
                (eid, eid),
            ).fetchall()
        else:
            rows = conn.execute(
                """SELECT t.predicate, s.name, o.name, t.valid_from, t.valid_to,
                          t.confidence, t.source_slug
                   FROM triples t
                   JOIN entities s ON t.subject = s.id
                   JOIN entities o ON t.object = o.id
                   ORDER BY t.valid_from IS NULL, t.valid_from ASC
                   LIMIT 100"""
            ).fetchall()
        conn.close()

        return [
            {
                "subject": r[1],
                "predicate": r[0],
                "object": r[2],
                "valid_from": r[3],
                "valid_to": r[4],
                "current": r[4] is None,
                "confidence": r[5],
                "source_slug": r[6],
            }
            for r in rows
        ]

    def stats(self) -> dict[str, Any]:
        """Entity count, triple count, relationship types."""
        conn = self._conn()
        ent_count = conn.execute("SELECT COUNT(*) FROM entities").fetchone()[0]
        tri_count = conn.execute("SELECT COUNT(*) FROM triples").fetchone()[0]
        preds = conn.execute(
            "SELECT DISTINCT predicate FROM triples ORDER BY predicate"
        ).fetchall()
        conn.close()
        return {
            "entities": ent_count,
            "triples": tri_count,
            "relationship_types": [r[0] for r in preds],
        }
