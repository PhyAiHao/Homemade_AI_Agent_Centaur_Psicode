from __future__ import annotations

from typing import Any

from .snip import message_to_text


class SessionMemoryCompactor:
    def __init__(self, *, max_chars: int = 1_200) -> None:
        self.max_chars = max_chars

    def compact(self, messages: list[dict[str, Any]]) -> str:
        user_notes = self._collect_recent_excerpts(messages, role="user", limit=3)
        assistant_notes = self._collect_recent_excerpts(
            messages, role="assistant", limit=3
        )

        sections = ["Session memory digest:"]
        if user_notes:
            sections.append("User intent:")
            sections.extend(f"- {note}" for note in user_notes)
        if assistant_notes:
            sections.append("Latest agent work:")
            sections.extend(f"- {note}" for note in assistant_notes)
        digest = "\n".join(sections).strip()
        if len(digest) <= self.max_chars:
            return digest
        return f"{digest[: self.max_chars - 3].rstrip()}..."

    def _collect_recent_excerpts(
        self,
        messages: list[dict[str, Any]],
        *,
        role: str,
        limit: int,
    ) -> list[str]:
        excerpts: list[str] = []
        for message in reversed(messages):
            if message.get("role") != role:
                continue
            excerpt = _shorten(message_to_text(message), 180)
            if excerpt:
                excerpts.append(excerpt)
            if len(excerpts) >= limit:
                break
        excerpts.reverse()
        return excerpts


def _shorten(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return f"{text[: limit - 3].rstrip()}..."
