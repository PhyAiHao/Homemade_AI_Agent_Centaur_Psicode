from __future__ import annotations

import json
import math
from dataclasses import dataclass
from typing import Any


@dataclass
class SnipResult:
    kept_messages: list[dict[str, Any]]
    snipped_messages: list[dict[str, Any]]
    kept_tokens: int
    freed_tokens: int


def message_to_text(message: dict[str, Any]) -> str:
    role = str(message.get("role", "unknown"))
    content = message.get("content", "")
    text = _content_to_text(content)
    return f"{role}: {text}".strip()


def estimate_message_tokens(message: dict[str, Any]) -> int:
    text = message_to_text(message)
    return max(1, math.ceil(len(text) / 4) + 4)


def estimate_messages_tokens(messages: list[dict[str, Any]]) -> int:
    return sum(estimate_message_tokens(message) for message in messages)


class HistorySnipper:
    def __init__(
        self,
        *,
        min_recent_messages: int = 12,
        summary_reserve_tokens: int = 1_200,
    ) -> None:
        self.min_recent_messages = min_recent_messages
        self.summary_reserve_tokens = summary_reserve_tokens

    def snip(
        self,
        messages: list[dict[str, Any]],
        token_budget: int,
    ) -> SnipResult:
        if not messages:
            return SnipResult([], [], 0, 0)

        leading_system_count = _leading_system_count(messages)
        kept_indices = set(range(leading_system_count))
        kept_tail: list[int] = []
        tail_tokens = 0
        target_budget = max(0, token_budget - self.summary_reserve_tokens)

        for index in range(len(messages) - 1, leading_system_count - 1, -1):
            message_tokens = estimate_message_tokens(messages[index])
            if (
                len(kept_tail) < self.min_recent_messages
                or tail_tokens + message_tokens <= target_budget
            ):
                kept_tail.append(index)
                tail_tokens += message_tokens

        kept_indices.update(kept_tail)
        kept_messages = [message for i, message in enumerate(messages) if i in kept_indices]
        snipped_messages = [
            message for i, message in enumerate(messages) if i not in kept_indices
        ]
        kept_tokens = estimate_messages_tokens(kept_messages)
        freed_tokens = estimate_messages_tokens(snipped_messages)
        return SnipResult(
            kept_messages=kept_messages,
            snipped_messages=snipped_messages,
            kept_tokens=kept_tokens,
            freed_tokens=freed_tokens,
        )


def _leading_system_count(messages: list[dict[str, Any]]) -> int:
    count = 0
    for message in messages:
        if message.get("role") != "system":
            break
        count += 1
    return count


def _content_to_text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts = [_content_to_text(item) for item in content]
        return "\n".join(part for part in parts if part).strip()
    if isinstance(content, dict):
        content_type = content.get("type")
        if content_type == "text":
            return str(content.get("text", ""))
        if content_type == "thinking":
            return str(content.get("thinking", ""))
        if content_type == "tool_use":
            name = content.get("name", "tool")
            tool_input = json.dumps(content.get("input", {}), sort_keys=True)
            return f"tool_use {name} {tool_input}"
        if content_type == "tool_result":
            return _content_to_text(content.get("content", ""))
        if "text" in content:
            return str(content["text"])
        return json.dumps(content, sort_keys=True)
    if content is None:
        return ""
    return str(content)
