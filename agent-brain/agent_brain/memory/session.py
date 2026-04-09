from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from ..types.base import AgentBaseModel
from .extract import normalize_message_content

DEFAULT_SESSION_MEMORY_TEMPLATE = """
# Session Title
_A short and distinctive title for the current session_

# Current State
_What is actively being worked on right now?_

# Task specification
_What did the user ask to build or explain?_

# Files and Functions
_Important files, modules, and functions discussed in the session_

# Workflow
_Important commands, workflows, or operational steps_

# Errors & Corrections
_Failures, corrections, and paths to avoid repeating_

# Learnings
_Reusable insights from the session_

# Key results
_Important outputs or conclusions reached so far_

# Worklog
_Terse chronological notes about what happened_
""".strip()


class SessionMemoryConfig(AgentBaseModel):
    minimum_message_tokens_to_init: int = 10_000
    minimum_tokens_between_update: int = 5_000
    tool_calls_between_updates: int = 3


class SessionMemoryState(AgentBaseModel):
    last_summarized_message_id: str | None = None
    extraction_started: bool = False
    tokens_at_last_extraction: int = 0
    initialized: bool = False


class SessionMemoryManager:
    def __init__(
        self,
        *,
        root_dir: str | Path,
        session_id: str = "default",
        config: SessionMemoryConfig | None = None,
    ) -> None:
        self.root_dir = Path(root_dir).expanduser()
        self.session_id = session_id
        self.config = config or SessionMemoryConfig()
        self.state = SessionMemoryState()
        self.path = self.root_dir / "sessions" / session_id / "SESSION_MEMORY.md"
        self.path.parent.mkdir(parents=True, exist_ok=True)
        if not self.path.exists():
            self.path.write_text(DEFAULT_SESSION_MEMORY_TEMPLATE + "\n", encoding="utf-8")

    def should_extract(
        self,
        *,
        current_token_count: int,
        tool_calls_since_update: int,
        last_assistant_turn_has_tool_calls: bool,
    ) -> bool:
        if not self.state.initialized:
            if current_token_count < self.config.minimum_message_tokens_to_init:
                return False
            self.state.initialized = True

        tokens_since_last = current_token_count - self.state.tokens_at_last_extraction
        has_token_threshold = tokens_since_last >= self.config.minimum_tokens_between_update
        has_tool_threshold = tool_calls_since_update >= self.config.tool_calls_between_updates
        should_extract = (has_token_threshold and has_tool_threshold) or (
            has_token_threshold and not last_assistant_turn_has_tool_calls
        )
        return should_extract

    def update(
        self,
        messages: list[dict[str, Any]],
        *,
        current_token_count: int | None = None,
        last_message_id: str | None = None,
    ) -> str:
        summary = self._build_sections(messages)
        rendered = render_session_template(summary)
        self.path.write_text(rendered, encoding="utf-8")
        self.state.tokens_at_last_extraction = current_token_count or self.state.tokens_at_last_extraction
        self.state.last_summarized_message_id = last_message_id
        self.state.extraction_started = False
        return rendered

    def load(self) -> str:
        return self.path.read_text(encoding="utf-8")

    def is_empty(self) -> bool:
        return self.load().strip() == DEFAULT_SESSION_MEMORY_TEMPLATE.strip()

    def _build_sections(self, messages: list[dict[str, Any]]) -> dict[str, str]:
        user_messages = [normalize_message_content(message) for message in messages if str(message.get("role", "")).lower() == "user"]
        assistant_messages = [normalize_message_content(message) for message in messages if str(message.get("role", "")).lower() == "assistant"]
        combined = [text for text in user_messages + assistant_messages if text.strip()]

        session_title = summarize_line(user_messages[0] if user_messages else "Untitled session", 10)
        current_state = summarize_line(assistant_messages[-1] if assistant_messages else (user_messages[-1] if user_messages else ""), 28)
        task_spec = summarize_paragraph(" ".join(user_messages[:3]), 300)
        files_and_functions = summarize_list(find_paths_and_symbols(combined))
        workflow = summarize_list(find_commands(combined))
        errors = summarize_list(find_error_lines(combined))
        learnings = summarize_list(find_learning_lines(combined))
        key_results = summarize_paragraph(assistant_messages[-1] if assistant_messages else "", 400)
        worklog = "\n".join(
            f"- {summarize_line(text, 18)}"
            for text in combined[-8:]
        )

        return {
            "Session Title": session_title or "_No title yet_",
            "Current State": current_state or "_No current state yet_",
            "Task specification": task_spec,
            "Files and Functions": files_and_functions,
            "Workflow": workflow,
            "Errors & Corrections": errors,
            "Learnings": learnings,
            "Key results": key_results,
            "Worklog": worklog,
        }


def render_session_template(section_values: dict[str, str]) -> str:
    output_lines: list[str] = []
    current_header = ""
    for line in DEFAULT_SESSION_MEMORY_TEMPLATE.splitlines():
        output_lines.append(line)
        if line.startswith("# "):
            current_header = line[2:]
            continue
        if line.startswith("_") and line.endswith("_") and current_header:
            content = section_values.get(current_header, "").strip()
            if content:
                output_lines.append("")
                output_lines.append(content)
                output_lines.append("")
    return "\n".join(output_lines).rstrip() + "\n"


def summarize_line(text: str, max_words: int) -> str:
    words = text.strip().split()
    if len(words) <= max_words:
        return " ".join(words)
    return " ".join(words[:max_words]).rstrip(".,;:") + "..."


def summarize_paragraph(text: str, max_chars: int) -> str:
    collapsed = " ".join(text.strip().split())
    if len(collapsed) <= max_chars:
        return collapsed
    return collapsed[: max_chars - 3].rstrip() + "..."


def summarize_list(items: list[str]) -> str:
    seen: list[str] = []
    for item in items:
        if item not in seen:
            seen.append(item)
    return "\n".join(f"- {item}" for item in seen[:8])


def find_paths_and_symbols(chunks: list[str]) -> list[str]:
    matches: list[str] = []
    path_pattern = re.compile(r"(?:[A-Za-z0-9_.-]+/)+[A-Za-z0-9_.-]+")
    symbol_pattern = re.compile(r"\b[A-Za-z_][A-Za-z0-9_]*\(\)")
    for chunk in chunks:
        matches.extend(path_pattern.findall(chunk))
        matches.extend(symbol_pattern.findall(chunk))
    return [summarize_line(match, 12) for match in matches[:20]]


def find_commands(chunks: list[str]) -> list[str]:
    commands: list[str] = []
    command_pattern = re.compile(r"\b(?:pnpm|npm|bun|python3?|pytest|vitest|cargo|git|rg|tsc)\b[^\n`]*")
    for chunk in chunks:
        commands.extend(command_pattern.findall(chunk))
    return [summarize_line(command, 16) for command in commands[:20]]


def find_error_lines(chunks: list[str]) -> list[str]:
    return [
        summarize_line(chunk, 18)
        for chunk in chunks
        if any(keyword in chunk.lower() for keyword in ("error", "failed", "fix", "broken", "correction"))
    ][:12]


def find_learning_lines(chunks: list[str]) -> list[str]:
    return [
        summarize_line(chunk, 18)
        for chunk in chunks
        if any(keyword in chunk.lower() for keyword in ("prefer", "learned", "remember", "avoid", "keep", "works well"))
    ][:12]
