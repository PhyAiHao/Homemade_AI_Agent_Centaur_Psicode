from __future__ import annotations

from collections.abc import Awaitable, Callable, Iterable
from typing import Any

from .._compat import Field
from ..types.base import AgentBaseModel

DatadogTransport = Callable[[list[dict[str, Any]]], Awaitable[None]]


class DatadogLogRecord(AgentBaseModel):
    ddsource: str = "python"
    ddtags: str = ""
    message: str
    service: str = "centaur-agent-brain"
    hostname: str = "centaur-agent-brain"
    payload: dict[str, Any] = Field(default_factory=dict)


class InMemoryDatadogTransport:
    def __init__(self) -> None:
        self.batches: list[list[dict[str, Any]]] = []

    async def __call__(self, records: list[dict[str, Any]]) -> None:
        self.batches.append(records)


class DatadogAnalyticsSink:
    def __init__(
        self,
        *,
        sender: DatadogTransport | None = None,
        allowed_events: Iterable[str] | None = None,
    ) -> None:
        self.sender = sender or _noop_sender
        self.allowed_events = set(allowed_events or [])
        self._batch: list[DatadogLogRecord] = []

    def log_event(self, event_name: str, metadata: dict[str, Any]) -> None:
        if self.allowed_events and event_name not in self.allowed_events:
            return
        tags = [f"event:{event_name}"]
        tags.extend(
            f"{key}:{value}"
            for key, value in metadata.items()
            if isinstance(value, (str, int, float, bool)) and value not in {"", None}
        )
        self._batch.append(
            DatadogLogRecord(
                message=event_name,
                ddtags=",".join(tags),
                payload={key: value for key, value in metadata.items() if value is not None},
            )
        )

    async def flush(self) -> None:
        if not self._batch:
            return
        records = [item.model_dump() for item in self._batch]
        self._batch.clear()
        await self.sender(records)


async def _noop_sender(_records: list[dict[str, Any]]) -> None:
    return
