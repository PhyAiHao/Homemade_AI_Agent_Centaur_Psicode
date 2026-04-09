from __future__ import annotations

import re
from datetime import datetime, timezone
from typing import Any

from ._compat import Field
from .types.base import AgentBaseModel

FILE_PATTERN = re.compile(r"(?<!\w)([A-Za-z0-9_./-]+\.[A-Za-z0-9_]+)")
FUNCTION_PATTERN = re.compile(r"\b([A-Za-z_][A-Za-z0-9_]*)\(")
ING_PATTERN = re.compile(r"\b([A-Za-z]{4,18}ing)\b", re.IGNORECASE)

KEYWORD_ACTIONS = (
    ("fix", "Fixing"),
    ("patch", "Patching"),
    ("test", "Running"),
    ("review", "Reviewing"),
    ("inspect", "Inspecting"),
    ("read", "Reading"),
    ("wire", "Wiring"),
    ("implement", "Implementing"),
    ("add", "Adding"),
    ("update", "Updating"),
    ("refactor", "Refactoring"),
    ("debug", "Debugging"),
    ("plan", "Planning"),
    ("summar", "Summarizing"),
)

IGNORED_ING_WORDS = {
    "string",
    "thing",
    "during",
    "warning",
    "according",
}


class AgentProgressSummary(AgentBaseModel):
    headline: str
    detail: str
    focus: str | None = None
    message_count: int = 0
    updated_at: str


class CapabilityCategory(AgentBaseModel):
    name: str
    items: list[str] = Field(default_factory=list)


class AgentCapabilitySummary(AgentBaseModel):
    headline: str
    categories: list[CapabilityCategory] = Field(default_factory=list)
    suggested_commands: list[str] = Field(default_factory=list)
    constraints: list[str] = Field(default_factory=list)


class AgentSummaryService:
    def __init__(self, *, min_messages: int = 3) -> None:
        self.min_messages = min_messages

    def build_progress_prompt(self, previous_summary: str | None = None) -> str:
        previous_line = (
            f'\nPrevious summary: "{previous_summary}"\nSay something new if progress moved.\n'
            if previous_summary
            else ""
        )
        return (
            "Describe the agent's most recent action in 3-5 words using present tense (-ing). "
            "Mention a file or function when possible. Do not use tools."
            f"{previous_line}"
            '\nGood: "Updating ipc_server.py"'
            '\nGood: "Running summary tests"'
            '\nGood: "Reviewing chrome setup"'
        )

    def summarize_progress(
        self,
        messages: list[dict[str, Any]],
        *,
        previous_summary: str | None = None,
    ) -> AgentProgressSummary:
        texts = self._collect_texts(messages)
        focus = self._select_focus(texts)
        action = self._select_action(texts, previous_summary=previous_summary)
        detail = self._build_detail(texts, focus)

        if len(messages) < self.min_messages and not texts:
            headline = "Gathering context"
        else:
            headline = action if focus is None else f"{action} {focus}"

        return AgentProgressSummary(
            headline=headline,
            detail=detail,
            focus=focus,
            message_count=len(messages),
            updated_at=datetime.now(timezone.utc).isoformat(),
        )

    def summarize_capabilities(
        self,
        *,
        commands: list[str] | None = None,
        tools: list[str] | None = None,
        skills: list[str] | None = None,
        plugins: list[str] | None = None,
        modes: list[str] | None = None,
        constraints: list[str] | None = None,
    ) -> AgentCapabilitySummary:
        command_items = sorted(set(commands or []))
        tool_items = sorted(set(tools or []))
        skill_items = sorted(set(skills or []))
        plugin_items = sorted(set(plugins or []))
        mode_items = sorted(set(modes or []))

        categories: list[CapabilityCategory] = []
        if command_items:
            categories.append(
                CapabilityCategory(name="commands", items=command_items[:8])
            )
        if tool_items:
            categories.append(CapabilityCategory(name="tools", items=tool_items[:8]))
        if skill_items:
            categories.append(
                CapabilityCategory(name="skills", items=skill_items[:8])
            )
        if plugin_items:
            categories.append(
                CapabilityCategory(name="plugins", items=plugin_items[:8])
            )
        if mode_items:
            categories.append(CapabilityCategory(name="modes", items=mode_items[:8]))

        summary_bits = [
            f"{len(command_items)} commands",
            f"{len(tool_items)} tools",
            f"{len(skill_items)} skills",
        ]
        if plugin_items:
            summary_bits.append(f"{len(plugin_items)} plugins")

        suggested_commands = [
            name
            for name in ("review", "commit", "ultraplan", "init", "insights")
            if name in command_items
        ]

        return AgentCapabilitySummary(
            headline=", ".join(summary_bits) if categories else "No capabilities registered",
            categories=categories,
            suggested_commands=suggested_commands,
            constraints=list(constraints or []),
        )

    def _collect_texts(self, messages: list[dict[str, Any]]) -> list[str]:
        texts: list[str] = []
        for message in messages:
            self._append_texts(message.get("content"), texts)
            for key in ("text", "summary", "error"):
                value = message.get(key)
                if isinstance(value, str) and value.strip():
                    texts.append(value.strip())
        return texts

    def _append_texts(self, value: Any, output: list[str]) -> None:
        if isinstance(value, str):
            stripped = value.strip()
            if stripped:
                output.append(stripped)
            return
        if isinstance(value, list):
            for item in value:
                self._append_texts(item, output)
            return
        if not isinstance(value, dict):
            return

        block_type = value.get("type")
        if block_type == "text" and isinstance(value.get("text"), str):
            output.append(str(value["text"]).strip())
        elif block_type == "tool_use":
            tool_name = str(value.get("name", "tool")).strip()
            output.append(f"Using {tool_name}")
            self._append_texts(value.get("input"), output)
        elif block_type == "tool_result":
            self._append_texts(value.get("content"), output)

        for key in ("content", "message", "detail", "path", "file", "function"):
            nested = value.get(key)
            if isinstance(nested, str):
                stripped = nested.strip()
                if stripped:
                    output.append(stripped)
            elif isinstance(nested, (dict, list)):
                self._append_texts(nested, output)

    def _select_focus(self, texts: list[str]) -> str | None:
        for text in reversed(texts):
            file_match = FILE_PATTERN.search(text)
            if file_match:
                return file_match.group(1).split("/")[-1]
        for text in reversed(texts):
            function_match = FUNCTION_PATTERN.search(text)
            if function_match:
                return f"{function_match.group(1)}()"
        return None

    def _select_action(
        self,
        texts: list[str],
        *,
        previous_summary: str | None,
    ) -> str:
        previous = (previous_summary or "").strip().lower()
        for text in reversed(texts):
            for match in ING_PATTERN.finditer(text):
                word = match.group(1).lower()
                if word in IGNORED_ING_WORDS:
                    continue
                candidate = word.capitalize()
                if previous and candidate.lower() in previous:
                    continue
                return candidate

        flattened = " ".join(texts).lower()
        for keyword, action in KEYWORD_ACTIONS:
            if keyword in flattened and (not previous or action.lower() not in previous):
                return action
        return "Reviewing"

    def _build_detail(self, texts: list[str], focus: str | None) -> str:
        if not texts:
            return "The agent is still gathering context."

        latest = self._collapse_whitespace(texts[-1])
        if focus and focus not in latest:
            return f"Latest visible work mentions {focus}. {latest[:140]}".strip()
        return latest[:180]

    def _collapse_whitespace(self, text: str) -> str:
        return re.sub(r"\s+", " ", text).strip()
