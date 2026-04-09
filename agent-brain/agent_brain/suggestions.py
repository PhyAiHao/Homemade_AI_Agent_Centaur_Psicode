from __future__ import annotations

from pathlib import Path
from typing import Any

from ._compat import Field
from .commands import COMMAND_REGISTRY
from .types.base import AgentBaseModel


class PromptSuggestion(AgentBaseModel):
    command_name: str
    prompt: str
    rationale: str


class PromptSuggestionService:
    def suggest(
        self,
        messages: list[dict[str, Any]],
        *,
        project_dir: str | Path | None = None,
        limit: int = 3,
    ) -> list[PromptSuggestion]:
        last_user_text = _last_user_text(messages).lower()
        suggestions: list[PromptSuggestion] = []

        if _contains_any(last_user_text, ["review", "diff", "regression", "pr"]):
            suggestions.append(
                self._make(
                    "review",
                    "Review the current change set for bugs and missing tests.",
                    "The recent request sounds review-oriented.",
                )
            )

        if _contains_any(last_user_text, ["security", "auth", "xss", "sql", "vuln"]):
            suggestions.append(
                self._make(
                    "security-review",
                    "Audit the current changes for high-confidence security issues.",
                    "The recent request mentions security-sensitive concerns.",
                )
            )

        if _contains_any(last_user_text, ["plan", "approach", "roadmap", "architecture"]):
            suggestions.append(
                self._make(
                    "ultraplan",
                    "Break this task into phased execution steps with validation gates.",
                    "The recent request sounds like planning work.",
                )
            )

        if _contains_any(last_user_text, ["commit", "message", "ship", "finalize"]):
            suggestions.append(
                self._make(
                    "commit",
                    "Generate a focused commit message from the current diff.",
                    "The recent request sounds like commit preparation.",
                )
            )

        if project_dir is not None and not Path(project_dir).joinpath("CLAUDE.md").exists():
            suggestions.append(
                self._make(
                    "init",
                    "Scan the repo and draft minimal CLAUDE.md guidance.",
                    "This repo does not appear to have a CLAUDE.md yet.",
                )
            )

        if _contains_any(last_user_text, ["insight", "understand", "codebase", "hotspot"]):
            suggestions.append(
                self._make(
                    "insights",
                    "Summarize architecture, hotspots, and follow-up questions for this repo.",
                    "The recent request sounds like codebase discovery.",
                )
            )

        if not suggestions:
            suggestions = [
                self._make(
                    "ultraplan",
                    "Plan the next implementation slice before editing code.",
                    "Planning is a safe default when the next step is ambiguous.",
                ),
                self._make(
                    "review",
                    "Review the current changes for correctness and missing tests.",
                    "A review pass helps surface obvious issues early.",
                ),
                self._make(
                    "insights",
                    "Summarize the main architecture and the next area worth reading.",
                    "An insights pass helps orient the next action.",
                ),
            ]

        deduped: list[PromptSuggestion] = []
        seen: set[str] = set()
        for suggestion in suggestions:
            if suggestion.command_name in seen:
                continue
            seen.add(suggestion.command_name)
            deduped.append(suggestion)
            if len(deduped) >= limit:
                break
        return deduped

    def _make(self, command_name: str, prompt: str, rationale: str) -> PromptSuggestion:
        if command_name not in COMMAND_REGISTRY:
            raise KeyError(f"Unknown command: {command_name}")
        return PromptSuggestion(
            command_name=command_name,
            prompt=prompt,
            rationale=rationale,
        )


def _last_user_text(messages: list[dict[str, Any]]) -> str:
    for message in reversed(messages):
        if message.get("role") == "user":
            content = message.get("content", "")
            if isinstance(content, str):
                return content
            return str(content)
    return ""


def _contains_any(text: str, fragments: list[str]) -> bool:
    return any(fragment in text for fragment in fragments)
