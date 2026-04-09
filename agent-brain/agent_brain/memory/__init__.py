from __future__ import annotations

from pathlib import Path
from typing import Any

from ..ipc_types import MemoryRequest, MemoryResponse
from .extract import ExtractionResult, MemoryExtractor
from .memdir import MemoryCandidate, MemoryRecord, MemoryStore
from .session import SessionMemoryManager
from .team_sync import TeamMemorySyncManager


class MemoryService:
    def __init__(self, root_dir: str | Path) -> None:
        self.store = MemoryStore(root_dir)
        self.extractor = MemoryExtractor()
        self.session_manager = SessionMemoryManager(root_dir=root_dir)
        self.team_sync = TeamMemorySyncManager(self.store.team_dir)

    async def handle(self, request: MemoryRequest) -> MemoryResponse:
        action = request.action
        payload = request.payload

        try:
            if action == "list":
                include_team = bool(payload.get("include_team", False))
                items = [record.model_dump() for record in self.store.list_memories("private")]
                if include_team:
                    items.extend(record.model_dump() for record in self.store.list_memories("team"))
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"items": items})

            if action == "save":
                record = self.store.save_memory(
                    title=str(payload["title"]),
                    body=str(payload["body"]),
                    memory_type=str(payload["memory_type"]),
                    scope=str(payload.get("scope", "private")),
                    description=str(payload.get("description", "")),
                    slug=payload.get("slug"),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"record": record.model_dump()})

            if action == "delete":
                deleted = self.store.delete_memory(
                    slug=str(payload["slug"]),
                    scope=str(payload.get("scope", "private")),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"deleted": deleted})

            if action == "recall":
                result = self.store.recall(
                    str(payload.get("query", "")),
                    include_team=bool(payload.get("include_team", True)),
                    limit=int(payload.get("limit", 5)),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload=result.model_dump())

            if action == "render_prompt":
                prompt = self.store.render_system_prompt(
                    query=str(payload.get("query", "")),
                    include_team=bool(payload.get("include_team", True)),
                    relevant_limit=int(payload.get("limit", 5)),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"prompt": prompt})

            if action == "extract":
                messages = payload.get("messages", [])
                result = self.extractor.extract(
                    messages,
                    store=self.store,
                    include_team=bool(payload.get("include_team", True)),
                )
                if payload.get("apply", False):
                    self.extractor.apply(
                        messages,
                        store=self.store,
                        include_team=bool(payload.get("include_team", True)),
                    )
                return MemoryResponse(request_id=request.request_id, ok=True, payload=result.model_dump())

            if action == "session_update":
                content = self.session_manager.update(
                    payload.get("messages", []),
                    current_token_count=int(payload.get("current_token_count", 0) or 0),
                    last_message_id=payload.get("last_message_id"),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"content": content, "path": str(self.session_manager.path)})

            if action == "session_get":
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=True,
                    payload={"content": self.session_manager.load(), "path": str(self.session_manager.path)},
                )

            if action == "team_export":
                snapshot = self.team_sync.export_snapshot(
                    organization_id=str(payload.get("organization_id", "org")),
                    repo=str(payload.get("repo", "repo")),
                    version=int(payload.get("version", 1)),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload={"snapshot": snapshot.model_dump()})

            if action == "team_import":
                snapshot_payload = payload.get("snapshot", {})
                result = self.team_sync.import_snapshot(
                    self._snapshot_from_payload(snapshot_payload),
                    merge=bool(payload.get("merge", True)),
                )
                return MemoryResponse(request_id=request.request_id, ok=True, payload=result.model_dump())

            if action == "dream_consolidate":
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=False,
                    error="dream_consolidate must be routed through DreamConsolidationService, not MemoryService",
                    payload={},
                )

            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=f"Unsupported memory action: {action}",
                payload={},
            )
        except Exception as error:
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=str(error),
                payload={},
            )

    def _snapshot_from_payload(self, payload: dict[str, Any]):
        from .team_sync import TeamMemoryContent, TeamMemorySnapshot

        content_payload = payload.get("content", {})
        return TeamMemorySnapshot(
            organization_id=str(payload.get("organization_id", "")),
            repo=str(payload.get("repo", "")),
            version=int(payload.get("version", 1)),
            last_modified=str(payload.get("last_modified", "")),
            checksum=str(payload.get("checksum", "")),
            content=TeamMemoryContent(
                entries=dict(content_payload.get("entries", {})),
                entry_checksums=dict(content_payload.get("entry_checksums", {})),
            ),
        )


__all__ = [
    "ExtractionResult",
    "MemoryCandidate",
    "MemoryRecord",
    "MemoryExtractor",
    "MemoryService",
    "MemoryStore",
    "SessionMemoryManager",
    "TeamMemorySyncManager",
]
