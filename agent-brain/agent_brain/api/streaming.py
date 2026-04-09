from __future__ import annotations

import json
from collections.abc import AsyncIterable, AsyncIterator, Iterable, Iterator, Mapping
from dataclasses import dataclass
from typing import Any

from ..models.catalog import APIProvider, calculate_cost_usd
from ..types.base import AgentBaseModel


class SseEvent(AgentBaseModel):
    event: str | None = None
    data: str = ""
    event_id: str | None = None
    retry: int | None = None


class UsageSnapshot(AgentBaseModel):
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_input_tokens: int = 0
    cache_creation_input_tokens: int = 0
    web_search_requests: int = 0
    web_fetch_requests: int = 0
    cost_usd: float = 0.0


@dataclass
class _SseAccumulator:
    event: str | None = None
    data_lines: list[str] | None = None
    event_id: str | None = None
    retry: int | None = None

    def __post_init__(self) -> None:
        if self.data_lines is None:
            self.data_lines = []

    def build(self) -> SseEvent:
        return SseEvent(
            event=self.event,
            data="\n".join(self.data_lines),
            event_id=self.event_id,
            retry=self.retry,
        )


def parse_sse_events(chunks: Iterable[str | bytes]) -> Iterator[SseEvent]:
    buffer = ""
    current = _SseAccumulator()

    for chunk in chunks:
        piece = chunk.decode("utf-8") if isinstance(chunk, bytes) else chunk
        buffer += piece
        while "\n" in buffer:
            line, buffer = buffer.split("\n", 1)
            line = line.rstrip("\r")
            if line == "":
                if current.data_lines:
                    yield current.build()
                current = _SseAccumulator()
                continue
            _consume_sse_line(current, line)

    if current.data_lines:
        yield current.build()


async def parse_async_sse_events(chunks: AsyncIterable[str | bytes]) -> AsyncIterator[SseEvent]:
    buffer = ""
    current = _SseAccumulator()

    async for chunk in chunks:
        piece = chunk.decode("utf-8") if isinstance(chunk, bytes) else chunk
        buffer += piece
        while "\n" in buffer:
            line, buffer = buffer.split("\n", 1)
            line = line.rstrip("\r")
            if line == "":
                if current.data_lines:
                    yield current.build()
                current = _SseAccumulator()
                continue
            _consume_sse_line(current, line)

    if current.data_lines:
        yield current.build()


def parse_sse_payload(event: SseEvent) -> dict[str, Any] | str:
    data = event.data.strip()
    if not data:
        return ""
    if data == "[DONE]":
        return data
    try:
        return json.loads(data)
    except json.JSONDecodeError:
        return data


class AnthropicStreamNormalizer:
    def __init__(
        self,
        *,
        request_id: str,
        model: str,
        provider: APIProvider = "first_party",
        fast_mode: bool = False,
    ) -> None:
        self.request_id = request_id
        self.model = model
        self.provider = provider
        self.fast_mode = fast_mode
        self.usage = UsageSnapshot()
        self._content_blocks: dict[int, dict[str, Any]] = {}
        self._stop_reason: str | None = None
        self._saw_message_start = False
        self._done_emitted = False

    async def normalize(
        self, raw_events: AsyncIterable[Mapping[str, Any] | Any]
    ) -> AsyncIterator[dict[str, Any]]:
        async for raw_event in raw_events:
            event = _coerce_event(raw_event)
            async for item in self._consume_event(event):
                yield item

        if self._saw_message_start and not self._done_emitted:
            yield self._build_done_event()

    async def _consume_event(self, event: Mapping[str, Any]) -> AsyncIterator[dict[str, Any]]:
        event_type = str(event.get("type", ""))
        if event_type == "message_start":
            self._saw_message_start = True
            self._merge_usage(event.get("message", {}).get("usage"))
            return

        if event_type == "content_block_start":
            index = int(event.get("index", 0))
            content_block = _coerce_event(event.get("content_block", {}))
            content_type = content_block.get("type")
            if content_type == "text":
                self._content_blocks[index] = {"type": "text", "text": ""}
            elif content_type in {"tool_use", "server_tool_use"}:
                self._content_blocks[index] = {
                    "type": content_type,
                    "id": content_block.get("id", ""),
                    "name": content_block.get("name", ""),
                    "input_json": "",
                }
            else:
                self._content_blocks[index] = dict(content_block)
            return

        if event_type == "content_block_delta":
            index = int(event.get("index", 0))
            delta = _coerce_event(event.get("delta", {}))
            block = self._content_blocks.get(index)
            if block is None:
                return
            delta_type = delta.get("type")
            if delta_type == "text_delta" and block.get("type") == "text":
                text = str(delta.get("text", ""))
                block["text"] += text
                yield {
                    "type": "text_delta",
                    "request_id": self.request_id,
                    "delta": text,
                }
                return
            if delta_type == "input_json_delta" and block.get("type") in {"tool_use", "server_tool_use"}:
                block["input_json"] += str(delta.get("partial_json", ""))
                return
            if delta_type == "thinking_delta":
                return
            if delta_type == "signature_delta":
                return
            return

        if event_type == "content_block_stop":
            index = int(event.get("index", 0))
            block = self._content_blocks.get(index)
            if block is None:
                return
            if block.get("type") in {"tool_use", "server_tool_use"}:
                yield {
                    "type": "tool_use",
                    "request_id": self.request_id,
                    "tool_call_id": block.get("id", ""),
                    "name": block.get("name", ""),
                    "input": _parse_partial_json(block.get("input_json", "")),
                    "server_tool_use": block.get("type") == "server_tool_use",
                }
            return

        if event_type == "message_delta":
            self._merge_usage(event.get("usage"))
            delta = _coerce_event(event.get("delta", {}))
            stop_reason = delta.get("stop_reason")
            if isinstance(stop_reason, str):
                self._stop_reason = stop_reason
            return

        if event_type == "message_stop":
            if not self._done_emitted:
                self._done_emitted = True
                yield self._build_done_event()
            return

    def _merge_usage(self, payload: Any) -> None:
        if not isinstance(payload, Mapping):
            return

        self.usage.input_tokens = _coalesce_int(
            payload.get("input_tokens"), self.usage.input_tokens
        )
        self.usage.output_tokens = _coalesce_int(
            payload.get("output_tokens"), self.usage.output_tokens
        )
        self.usage.cache_read_input_tokens = _coalesce_int(
            payload.get("cache_read_input_tokens"),
            self.usage.cache_read_input_tokens,
        )
        self.usage.cache_creation_input_tokens = _coalesce_int(
            payload.get("cache_creation_input_tokens"),
            self.usage.cache_creation_input_tokens,
        )

        server_tool_use = payload.get("server_tool_use")
        if isinstance(server_tool_use, Mapping):
            self.usage.web_search_requests = _coalesce_int(
                server_tool_use.get("web_search_requests"),
                self.usage.web_search_requests,
            )
            self.usage.web_fetch_requests = _coalesce_int(
                server_tool_use.get("web_fetch_requests"),
                self.usage.web_fetch_requests,
            )

    def _build_done_event(self) -> dict[str, Any]:
        usage_dict = self.usage.model_dump()
        usage_dict["cost_usd"] = calculate_cost_usd(
            self.model,
            usage_dict,
            provider=self.provider,
            fast_mode=self.fast_mode,
        )
        return {
            "type": "message_done",
            "request_id": self.request_id,
            "usage": usage_dict,
            "stop_reason": self._stop_reason,
            # 8a: Preserve raw stop_reason for debugging
            "raw_stop_reason": self._stop_reason,
            # 8c: Provider metadata for debugging auto-routing
            "provider_metadata": {
                "provider": self.provider,
                "resolved_model": self.model,
            },
        }


class OpenAIStreamNormalizer:
    """Normalize OpenAI ChatCompletionChunk objects into IPC events."""

    def __init__(self, *, request_id: str, model: str) -> None:
        self.request_id = request_id
        self.model = model
        self.usage = UsageSnapshot()
        self._tool_calls: dict[int, dict[str, Any]] = {}
        self._stop_reason: str | None = None
        self._raw_stop_reason: str | None = None

    async def normalize(self, stream: AsyncIterable[Any]) -> AsyncIterator[dict[str, Any]]:
        async for chunk in stream:
            for choice in (chunk.choices or []):
                delta = choice.delta
                # Check both content and reasoning fields.
                # Gemma4 and other reasoning models put text in delta.reasoning
                # instead of delta.content.
                text = None
                if delta:
                    text = delta.content or getattr(delta, "reasoning", None)
                if text:
                    yield {
                        "type": "text_delta",
                        "request_id": self.request_id,
                        "delta": text,
                    }
                if delta and delta.tool_calls:
                    for tc in delta.tool_calls:
                        idx = tc.index
                        if idx not in self._tool_calls:
                            self._tool_calls[idx] = {"id": "", "name": "", "arguments": ""}
                        if tc.id:
                            self._tool_calls[idx]["id"] = tc.id
                        if tc.function:
                            if tc.function.name:
                                self._tool_calls[idx]["name"] = tc.function.name
                            if tc.function.arguments:
                                self._tool_calls[idx]["arguments"] += tc.function.arguments
                if choice.finish_reason:
                    self._raw_stop_reason = choice.finish_reason
                    self._stop_reason = {
                        "stop": "end_turn",
                        "tool_calls": "tool_use",
                        "length": "max_tokens",
                    }.get(choice.finish_reason, choice.finish_reason)
            if chunk.usage:
                self.usage.input_tokens = chunk.usage.prompt_tokens or 0
                self.usage.output_tokens = chunk.usage.completion_tokens or 0

        # Emit accumulated tool calls
        for tc in self._tool_calls.values():
            yield {
                "type": "tool_use",
                "request_id": self.request_id,
                "tool_call_id": tc["id"],
                "name": tc["name"],
                "input": _parse_partial_json(tc["arguments"]),
                "server_tool_use": False,
            }

        # 8b: Preserve provider-specific usage details (e.g., completion_tokens_details)
        usage_dict = self.usage.model_dump()
        yield {
            "type": "message_done",
            "request_id": self.request_id,
            "usage": usage_dict,
            "stop_reason": self._stop_reason,
            "raw_stop_reason": self._raw_stop_reason,
            "provider_metadata": {
                "provider": "openai",
                "resolved_model": self.model,
            },
        }


class GeminiStreamNormalizer:
    """Normalize Google Gemini streaming chunks into IPC events."""

    def __init__(self, *, request_id: str, model: str) -> None:
        self.request_id = request_id
        self.model = model
        self.usage = UsageSnapshot()
        self._tool_calls: list[dict[str, Any]] = []
        self._stop_reason: str | None = None

    async def normalize(self, stream: AsyncIterable[Any]) -> AsyncIterator[dict[str, Any]]:
        import uuid as _uuid

        async for chunk in stream:
            candidates = getattr(chunk, "candidates", None) or []
            for candidate in candidates:
                content = getattr(candidate, "content", None)
                if content:
                    for part in getattr(content, "parts", []):
                        text = getattr(part, "text", None)
                        if text:
                            yield {
                                "type": "text_delta",
                                "request_id": self.request_id,
                                "delta": text,
                            }
                        fc = getattr(part, "function_call", None)
                        if fc:
                            self._tool_calls.append({
                                "id": str(_uuid.uuid4())[:8],
                                "name": getattr(fc, "name", ""),
                                "args": dict(getattr(fc, "args", {})),
                            })
                finish_reason = getattr(candidate, "finish_reason", None)
                if finish_reason:
                    fr_str = str(finish_reason)
                    if "STOP" in fr_str:
                        self._stop_reason = "end_turn"
                    elif "MAX_TOKENS" in fr_str:
                        self._stop_reason = "max_tokens"

            usage_meta = getattr(chunk, "usage_metadata", None)
            if usage_meta:
                self.usage.input_tokens = getattr(usage_meta, "prompt_token_count", 0) or 0
                self.usage.output_tokens = getattr(usage_meta, "candidates_token_count", 0) or 0

        # Emit tool calls
        for tc in self._tool_calls:
            if not self._stop_reason:
                self._stop_reason = "tool_use"
            yield {
                "type": "tool_use",
                "request_id": self.request_id,
                "tool_call_id": tc["id"],
                "name": tc["name"],
                "input": tc["args"],
                "server_tool_use": False,
            }

        yield {
            "type": "message_done",
            "request_id": self.request_id,
            "usage": self.usage.model_dump(),
            "stop_reason": self._stop_reason,
            "raw_stop_reason": self._stop_reason,  # Gemini stop_reason is already raw
            "provider_metadata": {
                "provider": "gemini",
                "resolved_model": self.model,
            },
        }


def _consume_sse_line(accumulator: _SseAccumulator, line: str) -> None:
    if line.startswith(":"):
        return
    field, _, value = line.partition(":")
    value = value.lstrip(" ")
    if field == "event":
        accumulator.event = value
    elif field == "data":
        accumulator.data_lines.append(value)
    elif field == "id":
        accumulator.event_id = value
    elif field == "retry":
        try:
            accumulator.retry = int(value)
        except ValueError:
            accumulator.retry = None


def _coerce_event(value: Any) -> dict[str, Any]:
    if isinstance(value, Mapping):
        return {str(key): _coerce_nested(item) for key, item in value.items()}
    if hasattr(value, "model_dump") and callable(value.model_dump):
        dumped = value.model_dump()
        if isinstance(dumped, Mapping):
            return {str(key): _coerce_nested(item) for key, item in dumped.items()}
    if hasattr(value, "dict") and callable(value.dict):
        dumped = value.dict()
        if isinstance(dumped, Mapping):
            return {str(key): _coerce_nested(item) for key, item in dumped.items()}
    if hasattr(value, "__dict__"):
        return {
            str(key): _coerce_nested(item)
            for key, item in vars(value).items()
            if not key.startswith("_")
        }
    return {}


def _coerce_nested(value: Any) -> Any:
    if isinstance(value, Mapping):
        return {str(key): _coerce_nested(item) for key, item in value.items()}
    if isinstance(value, list):
        return [_coerce_nested(item) for item in value]
    if hasattr(value, "model_dump") and callable(value.model_dump):
        return _coerce_nested(value.model_dump())
    if hasattr(value, "__dict__") and not isinstance(value, type):
        return _coerce_nested(vars(value))
    return value


def _parse_partial_json(raw_json: str) -> Any:
    raw_json = raw_json.strip()
    if not raw_json:
        return {}
    try:
        return json.loads(raw_json)
    except json.JSONDecodeError:
        return {"raw_json": raw_json}


def _coalesce_int(value: Any, current: int) -> int:
    if value is None:
        return current
    try:
        return int(value)
    except (TypeError, ValueError):
        return current
