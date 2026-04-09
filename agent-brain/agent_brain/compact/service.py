from __future__ import annotations

from pathlib import Path
from typing import Any

try:
    from jinja2 import Environment, FileSystemLoader
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Environment = None  # type: ignore[assignment]
    FileSystemLoader = None  # type: ignore[assignment]

from ..ipc_types import CompactRequest, CompactResponse
from .auto import AutoCompactPolicy
from .micro import MicroCompactor
from .session_memory_compact import SessionMemoryCompactor
from .snip import HistorySnipper, estimate_messages_tokens, message_to_text


class CompactService:
    def __init__(
        self,
        *,
        prompts_dir: str | Path | None = None,
        auto_policy: AutoCompactPolicy | None = None,
        micro_compactor: MicroCompactor | None = None,
        snipper: HistorySnipper | None = None,
        session_memory_compactor: SessionMemoryCompactor | None = None,
    ) -> None:
        self.prompts_dir = Path(
            prompts_dir or (Path(__file__).resolve().parent.parent / "prompts")
        )
        self.auto_policy = auto_policy or AutoCompactPolicy()
        self.micro_compactor = micro_compactor or MicroCompactor()
        self.snipper = snipper or HistorySnipper()
        self.session_memory_compactor = (
            session_memory_compactor or SessionMemoryCompactor()
        )

    async def handle(self, request: CompactRequest) -> CompactResponse:
        summary, compacted_messages = self.compact(
            request.messages, token_budget=request.token_budget
        )
        return CompactResponse(
            request_id=request.request_id,
            summary=summary,
            messages=compacted_messages,
        )

    def compact(
        self,
        messages: list[dict[str, Any]],
        *,
        token_budget: int | None,
    ) -> tuple[str, list[dict[str, Any]]]:
        decision = self.auto_policy.evaluate(messages, token_budget)
        micro_messages, micro_stats = self.micro_compactor.compact_messages(messages)
        snip_result = self.snipper.snip(micro_messages, decision.effective_budget)
        kept_messages = snip_result.kept_messages
        summary = self._render_summary(
            original_messages=messages,
            kept_messages=kept_messages,
            snipped_messages=snip_result.snipped_messages,
            requested_budget=token_budget or 24_000,
            effective_budget=decision.effective_budget,
            estimated_tokens_before=decision.estimated_tokens,
            estimated_tokens_after=estimate_messages_tokens(kept_messages),
            tool_results_microcompacted=micro_stats.summarized_tool_results,
            chars_saved=micro_stats.chars_saved,
            session_memory=self.session_memory_compactor.compact(messages),
        )
        return summary, kept_messages

    def _render_summary(
        self,
        *,
        original_messages: list[dict[str, Any]],
        kept_messages: list[dict[str, Any]],
        snipped_messages: list[dict[str, Any]],
        requested_budget: int,
        effective_budget: int,
        estimated_tokens_before: int,
        estimated_tokens_after: int,
        tool_results_microcompacted: int,
        chars_saved: int,
        session_memory: str,
    ) -> str:
        primary_request = _extract_primary_request(original_messages)
        highlights = _collect_highlights(snipped_messages or original_messages)
        context = {
            "requested_budget": requested_budget,
            "effective_budget": effective_budget,
            "estimated_tokens_before": estimated_tokens_before,
            "estimated_tokens_after": estimated_tokens_after,
            "messages_total": len(original_messages),
            "messages_kept": len(kept_messages),
            "messages_removed": len(snipped_messages),
            "tool_results_microcompacted": tool_results_microcompacted,
            "chars_saved": chars_saved,
            "primary_request": primary_request,
            "highlights": highlights,
            "session_memory": session_memory,
        }

        if Environment is not None and FileSystemLoader is not None:
            environment = Environment(
                loader=FileSystemLoader(str(self.prompts_dir)),
                autoescape=False,
                trim_blocks=True,
                lstrip_blocks=True,
            )
            template = environment.get_template("compact.j2")
            return template.render(**context).strip()

        highlights_text = "\n".join(f"- {item}" for item in highlights) or "- No earlier highlights."
        return "\n".join(
            [
                "# Compact Summary",
                f"Primary request: {primary_request}",
                f"Budget: {estimated_tokens_before} -> {estimated_tokens_after} tokens within target {effective_budget}/{requested_budget}.",
                f"Messages removed: {len(snipped_messages)} of {len(original_messages)}.",
                f"Tool results micro-compacted: {tool_results_microcompacted} (saved {chars_saved} chars).",
                "",
                "Highlights:",
                highlights_text,
                "",
                session_memory,
            ]
        ).strip()


def _extract_primary_request(messages: list[dict[str, Any]]) -> str:
    for message in messages:
        if message.get("role") == "user":
            return _shorten(message_to_text(message), 220)
    return "No user request captured."


def _collect_highlights(messages: list[dict[str, Any]], limit: int = 8) -> list[str]:
    highlights: list[str] = []
    for message in messages:
        if message.get("role") not in {"user", "assistant"}:
            continue
        excerpt = _shorten(message_to_text(message), 200)
        if excerpt:
            highlights.append(excerpt)
        if len(highlights) >= limit:
            break
    if not highlights:
        highlights.append("Earlier messages were retained without needing a separate summary.")
    return highlights


def _shorten(text: str, limit: int) -> str:
    if len(text) <= limit:
        return text
    return f"{text[: limit - 3].rstrip()}..."
