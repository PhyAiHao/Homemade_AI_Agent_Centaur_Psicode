from __future__ import annotations

from typing import Any, Callable

from .types.base import AgentBaseModel


TipPredicate = Callable[[dict[str, Any]], bool]


class TipDefinition(AgentBaseModel):
    tip_id: str
    content: str
    cooldown_sessions: int = 5


class TipsService:
    def __init__(self) -> None:
        self._tips: list[tuple[TipDefinition, TipPredicate]] = [
            (
                TipDefinition(
                    tip_id="memory",
                    content="Use memory features to preserve durable user preferences and project context.",
                    cooldown_sessions=10,
                ),
                lambda context: not context.get("uses_memory", False),
            ),
            (
                TipDefinition(
                    tip_id="init",
                    content="Run the init flow to generate concise repo guidance such as CLAUDE.md.",
                    cooldown_sessions=10,
                ),
                lambda context: not context.get("has_claude_md", False),
            ),
            (
                TipDefinition(
                    tip_id="review",
                    content="Use the review prompt flow before shipping larger changes.",
                    cooldown_sessions=6,
                ),
                lambda context: context.get("recent_edit_count", 0) >= 3,
            ),
        ]

    def suggest(
        self,
        context: dict[str, Any] | None = None,
        *,
        history: dict[str, int] | None = None,
        limit: int = 3,
    ) -> list[TipDefinition]:
        ctx = dict(context or {})
        seen = dict(history or {})
        results: list[TipDefinition] = []
        for tip, predicate in self._tips:
            if seen.get(tip.tip_id, 0) > 0:
                continue
            if not predicate(ctx):
                continue
            results.append(tip)
            if len(results) >= limit:
                break
        return results
