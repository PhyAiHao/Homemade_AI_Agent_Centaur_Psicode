from __future__ import annotations

import hashlib
import json
import os
from collections.abc import AsyncIterable, Awaitable, Callable, Iterable
from pathlib import Path
from typing import Any, TypeVar

T = TypeVar("T")


def should_use_vcr(env: dict[str, str] | None = None) -> bool:
    values = env or os.environ
    if values.get("NODE_ENV") == "test":
        return True
    if values.get("FORCE_VCR") in {"1", "true", "TRUE"}:
        return True
    if values.get("VCR_RECORD") in {"1", "true", "TRUE"}:
        return True
    return False


class VCRRecorder:
    def __init__(
        self,
        *,
        root_dir: str | Path,
        enabled: bool = True,
    ) -> None:
        self.root_dir = Path(root_dir)
        self.root_dir.mkdir(parents=True, exist_ok=True)
        self.enabled = enabled

    @classmethod
    def from_environment(
        cls,
        *,
        root_dir: str | Path | None = None,
        env: dict[str, str] | None = None,
    ) -> "VCRRecorder":
        values = env or os.environ
        resolved_root = root_dir or values.get("CLAUDE_CODE_TEST_FIXTURES_ROOT") or "."
        return cls(root_dir=resolved_root, enabled=should_use_vcr(values))

    def make_key(self, payload: Any, *, prefix: str = "") -> str:
        body = json.dumps(payload, sort_keys=True, separators=(",", ":"))
        return f"{prefix}{body}" if prefix else body

    def fixture_path(self, key: str) -> Path:
        digest = hashlib.sha1(key.encode("utf-8")).hexdigest()[:12]
        return self.root_dir / f"{digest}.json"

    def replay(self, key: str) -> Any | None:
        path = self.fixture_path(key)
        if not path.exists():
            return None
        return json.loads(path.read_text(encoding="utf-8"))

    def record(self, key: str, payload: Any) -> Path:
        path = self.fixture_path(key)
        path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")
        return path

    def with_fixture(self, key: str, builder: Callable[[], T]) -> T:
        if self.enabled:
            cached = self.replay(key)
            if cached is not None:
                return cached
        result = builder()
        if self.enabled:
            self.record(key, result)
        return result

    async def with_fixture_async(self, key: str, builder: Callable[[], Awaitable[T]]) -> T:
        if self.enabled:
            cached = self.replay(key)
            if cached is not None:
                return cached
        result = await builder()
        if self.enabled:
            self.record(key, result)
        return result

    def replay_stream(self, key: str) -> list[Any] | None:
        cached = self.replay(key)
        if cached is None:
            return None
        if isinstance(cached, list):
            return cached
        return [cached]

    def record_stream(self, key: str, payload: Iterable[Any]) -> Path:
        return self.record(key, list(payload))

    async def with_stream_fixture(
        self,
        key: str,
        builder: Callable[[], AsyncIterable[T]],
    ) -> list[T]:
        if self.enabled:
            cached = self.replay_stream(key)
            if cached is not None:
                return cached

        collected: list[T] = []
        async for item in builder():
            collected.append(item)

        if self.enabled:
            self.record_stream(key, collected)
        return collected
