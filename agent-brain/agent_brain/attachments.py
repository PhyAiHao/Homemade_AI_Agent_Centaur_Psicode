from __future__ import annotations

from pathlib import Path
from typing import Any, Literal

from ._compat import Field
from .types.base import AgentBaseModel

AttachmentKind = Literal["file", "memory", "note"]
TRUNCATION_MARKER = "\n[truncated attachment content]"


class AttachmentItem(AgentBaseModel):
    kind: AttachmentKind
    label: str
    content: str
    path: str | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)


def build_file_attachment(
    path: str | Path,
    *,
    label: str | None = None,
    max_chars: int = 4_000,
) -> AttachmentItem:
    file_path = Path(path).expanduser().resolve()
    raw_content = file_path.read_text(encoding="utf-8", errors="replace")
    content, truncated = _truncate_text(raw_content, max_chars)
    return AttachmentItem(
        kind="file",
        label=label or file_path.name,
        path=str(file_path),
        content=content,
        metadata={
            "truncated": truncated,
            "char_count": len(raw_content),
            "suffix": file_path.suffix,
        },
    )


def build_memory_attachment(
    title: str,
    body: str,
    *,
    memory_type: str = "project",
    scope: str = "private",
    description: str = "",
) -> AttachmentItem:
    return AttachmentItem(
        kind="memory",
        label=title,
        content=body.strip(),
        metadata={
            "memory_type": memory_type,
            "scope": scope,
            "description": description.strip(),
        },
    )


def build_note_attachment(
    label: str,
    content: str,
    *,
    metadata: dict[str, Any] | None = None,
) -> AttachmentItem:
    return AttachmentItem(
        kind="note",
        label=label,
        content=content.strip(),
        metadata=dict(metadata or {}),
    )


def render_attachment_bundle(items: list[AttachmentItem]) -> str:
    if not items:
        return "No attachments were provided."
    return "\n\n".join(render_attachment(item) for item in items)


def render_memory_bundle(items: list[AttachmentItem]) -> str:
    memory_items = [item for item in items if item.kind == "memory"]
    if not memory_items:
        return "No persistent memory was provided."

    lines: list[str] = []
    for item in memory_items:
        memory_type = item.metadata.get("memory_type", "memory")
        scope = item.metadata.get("scope", "private")
        description = str(item.metadata.get("description", "")).strip()
        header = f"- {item.label} [{memory_type}/{scope}]"
        lines.append(header)
        if description:
            lines.append(f"  Why it matters: {description}")
        lines.append(f"  Content: {item.content}")
    return "\n".join(lines)


def render_attachment(item: AttachmentItem) -> str:
    if item.kind == "file":
        return _render_file_attachment(item)
    if item.kind == "memory":
        return _render_memory_attachment(item)
    return _render_note_attachment(item)


def _render_file_attachment(item: AttachmentItem) -> str:
    path_line = f"Path: {item.path}" if item.path else "Path: (inline)"
    return "\n".join(
        [
            f"## File Attachment: {item.label}",
            path_line,
            "```text",
            item.content,
            "```",
        ]
    ).strip()


def _render_memory_attachment(item: AttachmentItem) -> str:
    memory_type = item.metadata.get("memory_type", "memory")
    scope = item.metadata.get("scope", "private")
    description = str(item.metadata.get("description", "")).strip()
    lines = [
        f"## Memory Attachment: {item.label}",
        f"Type: {memory_type}",
        f"Scope: {scope}",
    ]
    if description:
        lines.append(f"Description: {description}")
    lines.extend(["```text", item.content, "```"])
    return "\n".join(lines).strip()


def _render_note_attachment(item: AttachmentItem) -> str:
    return "\n".join(
        [
            f"## Note Attachment: {item.label}",
            "```text",
            item.content,
            "```",
        ]
    ).strip()


def _truncate_text(text: str, max_chars: int) -> tuple[str, bool]:
    if len(text) <= max_chars:
        return text, False

    visible_chars = max(80, max_chars - len(TRUNCATION_MARKER))
    return f"{text[:visible_chars].rstrip()}{TRUNCATION_MARKER}", True
