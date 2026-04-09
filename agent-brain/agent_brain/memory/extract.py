from __future__ import annotations

import re
from typing import Any

from .._compat import Field
from ..types.base import AgentBaseModel
from .memdir import MemoryCandidate, MemoryStore, MemoryType, summarize_text

try:
    from jinja2 import Template
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Template = None  # type: ignore[assignment]

EXTRACTION_PROMPT = """You are extracting durable memories from the recent conversation.

Only keep information that will matter in future conversations.

Existing memories:
{{ existing_manifest }}

Recent messages:
{{ messages_text }}
"""


class ExtractionResult(AgentBaseModel):
    candidates: list[MemoryCandidate] = Field(default_factory=list)
    prompt: str = ""


class MemoryExtractor:
    def __init__(self, llm_callback=None) -> None:
        self.llm_callback = llm_callback

    def extract(
        self,
        messages: list[dict[str, Any]],
        *,
        store: MemoryStore | None = None,
        include_team: bool = True,
    ) -> ExtractionResult:
        existing_manifest = ""
        if store is not None:
            existing_records = store.list_memories("private")
            if include_team:
                existing_records += store.list_memories("team")
            existing_manifest = "\n".join(
                f"- [{record.metadata.memory_type}] {record.metadata.name}: {record.metadata.description}"
                for record in existing_records
            )
        prompt = self.build_prompt(messages, existing_manifest)
        if self.llm_callback is not None:
            candidates = self.llm_callback(messages=messages, prompt=prompt)
        else:
            candidates = self._heuristic_extract(messages, include_team=include_team)
        return ExtractionResult(candidates=candidates, prompt=prompt)

    def apply(
        self,
        messages: list[dict[str, Any]],
        *,
        store: MemoryStore,
        include_team: bool = True,
    ) -> list[MemoryCandidate]:
        result = self.extract(messages, store=store, include_team=include_team)
        for candidate in result.candidates:
            store.save_memory(
                title=candidate.title,
                body=candidate.body,
                memory_type=candidate.memory_type,
                scope=candidate.scope,
                description=candidate.description,
            )
        return result.candidates

    def build_prompt(self, messages: list[dict[str, Any]], existing_manifest: str) -> str:
        messages_text = "\n".join(
            f"{str(message.get('role', 'unknown')).upper()}: {normalize_message_content(message)}"
            for message in messages[-20:]
        )
        context = {
            "existing_manifest": existing_manifest or "_No memories yet._",
            "messages_text": messages_text,
        }
        if Template is not None:
            return Template(EXTRACTION_PROMPT).render(**context).strip()
        return (
            EXTRACTION_PROMPT.replace("{{ existing_manifest }}", context["existing_manifest"])
            .replace("{{ messages_text }}", context["messages_text"])
            .strip()
        )

    def _heuristic_extract(
        self,
        messages: list[dict[str, Any]],
        *,
        include_team: bool,
    ) -> list[MemoryCandidate]:
        candidates: list[MemoryCandidate] = []
        for message in messages[-20:]:
            role = str(message.get("role", "")).lower()
            if role != "user":
                continue
            content = normalize_message_content(message)
            if not content:
                continue
            candidate = self._candidate_from_text(content, include_team=include_team)
            if candidate is not None and not _is_duplicate_candidate(candidates, candidate):
                candidates.append(candidate)
        return candidates[:5]

    def _candidate_from_text(
        self, text: str, *, include_team: bool
    ) -> MemoryCandidate | None:
        lowered = text.lower()
        if "forget" in lowered:
            return None

        if any(keyword in lowered for keyword in ("linear", "slack", "grafana", "dashboard", "wiki", "runbook", "http://", "https://")):
            return MemoryCandidate(
                title=title_from_text(text, "Reference"),
                body=text.strip(),
                description=summarize_text(text),
                memory_type="reference",
                scope="team" if include_team else "private",
                confidence=0.7,
            )

        if any(keyword in lowered for keyword in ("don't", "do not", "stop ", "avoid ", "keep doing", "prefer ", "please always")):
            return MemoryCandidate(
                title=title_from_text(text, "Feedback"),
                body=text.strip(),
                description=summarize_text(text),
                memory_type="feedback",
                scope="private",
                confidence=0.8,
            )

        if re.search(r"\b(i am|i'm|i’ve been|i have been)\b", lowered):
            return MemoryCandidate(
                title=title_from_text(text, "User"),
                body=text.strip(),
                description=summarize_text(text),
                memory_type="user",
                scope="private",
                confidence=0.75,
            )

        if any(
            keyword in lowered
            for keyword in (
                "deadline",
                "release",
                "incident",
                "migration",
                "freeze",
                "rollout",
                "we're",
                "we are",
                "by ",
                "before ",
                "after ",
            )
        ):
            return MemoryCandidate(
                title=title_from_text(text, "Project"),
                body=text.strip(),
                description=summarize_text(text),
                memory_type="project",
                scope="team" if include_team else "private",
                confidence=0.65,
            )

        if "remember" in lowered:
            memory_type: MemoryType = "project"
            if "i am" in lowered or "i'm" in lowered:
                memory_type = "user"
            return MemoryCandidate(
                title=title_from_text(text, "Memory"),
                body=text.strip(),
                description=summarize_text(text),
                memory_type=memory_type,
                scope="private",
                confidence=0.6,
            )

        return None


def normalize_message_content(message: dict[str, Any]) -> str:
    content = message.get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        chunks: list[str] = []
        for block in content:
            if isinstance(block, dict):
                if "text" in block and isinstance(block["text"], str):
                    chunks.append(block["text"])
                elif block.get("type") == "tool_result":
                    chunks.append(str(block.get("content", "")))
            else:
                chunks.append(str(block))
        return "\n".join(chunks)
    return str(content)


def title_from_text(text: str, prefix: str) -> str:
    cleaned = summarize_text(text, 48).rstrip(".")
    return f"{prefix}: {cleaned}"


def _is_duplicate_candidate(
    candidates: list[MemoryCandidate], candidate: MemoryCandidate
) -> bool:
    normalized = candidate.body.strip().lower()
    return any(existing.body.strip().lower() == normalized for existing in candidates)
