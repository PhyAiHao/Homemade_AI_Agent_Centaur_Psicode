from __future__ import annotations

import random
from collections.abc import Awaitable, Callable
from typing import Any

from .growthbook import GrowthBookExposure, GrowthBookFeatureStore
from .metadata import EventMetadataBuilder

FirstPartyTransport = Callable[[list[dict[str, Any]]], Awaitable[None]]


def should_sample_event(
    event_name: str,
    config: dict[str, dict[str, float]] | None,
    *,
    random_value: float | None = None,
) -> float | None:
    if not config:
        return None
    event_config = config.get(event_name)
    if not event_config:
        return None
    sample_rate = event_config.get("sample_rate")
    if not isinstance(sample_rate, (int, float)):
        return None
    if sample_rate <= 0:
        return 0.0
    if sample_rate >= 1:
        return None
    roll = random.random() if random_value is None else random_value
    return float(sample_rate) if roll < sample_rate else 0.0


class InMemoryFirstPartyTransport:
    def __init__(self) -> None:
        self.batches: list[list[dict[str, Any]]] = []

    async def __call__(self, records: list[dict[str, Any]]) -> None:
        self.batches.append(records)


class FirstPartyEventLogger:
    def __init__(
        self,
        *,
        metadata_builder: EventMetadataBuilder | None = None,
        growthbook: GrowthBookFeatureStore | None = None,
        sender: FirstPartyTransport | None = None,
    ) -> None:
        self.metadata_builder = metadata_builder or EventMetadataBuilder()
        self.growthbook = growthbook or GrowthBookFeatureStore()
        self.sender = sender or _noop_sender
        self._batch: list[dict[str, Any]] = []

    def log_event(self, event_name: str, metadata: dict[str, Any] | None = None) -> bool:
        sample = should_sample_event(
            event_name,
            self.growthbook.get_dynamic_config("tengu_event_sampling_config", {}),
        )
        if sample == 0:
            return False
        payload = dict(metadata or {})
        if sample is not None:
            payload["sample_rate"] = sample
        event = self.metadata_builder.build_internal_event(
            event_name,
            metadata=payload,
            model=str(payload.get("model", "")),
            session_id=str(payload.get("session_id", "")),
        )
        self._batch.append(event.model_dump())
        return True

    def log_growthbook_experiment(self, exposure: GrowthBookExposure) -> None:
        event = self.metadata_builder.build_growthbook_experiment_event(exposure)
        self._batch.append(event.model_dump())

    async def flush(self) -> None:
        if not self._batch:
            return
        records = list(self._batch)
        self._batch.clear()
        await self.sender(records)


async def _noop_sender(_records: list[dict[str, Any]]) -> None:
    return
