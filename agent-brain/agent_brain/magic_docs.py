from __future__ import annotations

import re
from pathlib import Path

from .types.base import AgentBaseModel


MAGIC_DOC_HEADER_PATTERN = re.compile(r"^#\s*MAGIC\s+DOC:\s*(.+)$", re.MULTILINE)
ITALICS_LINE_PATTERN = re.compile(r"^[_*](.+?)[_*]\s*$")


class MagicDocHeader(AgentBaseModel):
    title: str
    instructions: str = ""


class MagicDocsService:
    def __init__(self) -> None:
        self._tracked_docs: dict[str, MagicDocHeader] = {}

    def detect(self, content: str) -> MagicDocHeader | None:
        match = MAGIC_DOC_HEADER_PATTERN.search(content)
        if not match:
            return None

        title = match.group(1).strip()
        instructions = ""
        after_header = content[match.end() :].splitlines()
        for line in after_header:
            stripped = line.strip()
            if not stripped:
                continue
            italics = ITALICS_LINE_PATTERN.match(stripped)
            if italics:
                instructions = italics.group(1).strip()
            break
        return MagicDocHeader(title=title, instructions=instructions)

    def register_if_magic_doc(self, path: str | Path, content: str) -> bool:
        detected = self.detect(content)
        if detected is None:
            return False
        self._tracked_docs[str(Path(path))] = detected
        return True

    def tracked_docs(self) -> dict[str, MagicDocHeader]:
        return dict(self._tracked_docs)

    def build_update_prompt(
        self,
        *,
        path: str | Path,
        content: str,
        latest_summary: str = "",
    ) -> str:
        detected = self.detect(content)
        if detected is None:
            raise ValueError("File does not contain a MAGIC DOC header")

        summary = latest_summary.strip() or "No fresh conversation summary was supplied."
        instructions = detected.instructions or "No extra author instructions were embedded."
        return "\n".join(
            [
                "# Magic Docs Update",
                f"Path: {Path(path)}",
                f"Title: {detected.title}",
                f"Embedded instructions: {instructions}",
                "",
                "Current document:",
                "```markdown",
                content.strip(),
                "```",
                "",
                "Recent conversation summary:",
                "```text",
                summary,
                "```",
                "",
                "Update the document so it stays accurate, concise, and durable.",
            ]
        ).strip()
