from __future__ import annotations

import re
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

try:
    from jinja2 import Template
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Template = None  # type: ignore[assignment]

from ..ipc_types import SkillRequest, SkillResponse
from .loader import SkillDefinition, SkillLoader


class SkillExecutor:
    def __init__(
        self,
        *,
        loader: SkillLoader | None = None,
        project_dir: str | Path | None = None,
    ) -> None:
        self.loader = loader or SkillLoader()
        self.project_dir = str(Path(project_dir or Path.cwd()).resolve())

    def render(
        self,
        skill_name: str,
        arguments: dict[str, Any] | None = None,
    ) -> tuple[str, SkillDefinition]:
        definition = self.loader.get(skill_name)
        if definition is None:
            raise ValueError(f"Unknown skill: {skill_name}")

        context = {
            "skill_name": definition.name,
            "user_request": "",
            "args": "",
            "project_dir": self.project_dir,
            "current_date": datetime.now(timezone.utc).date().isoformat(),
        }
        if arguments:
            context.update(arguments)
            if "args" in arguments and not context["user_request"]:
                context["user_request"] = arguments["args"]
            elif "user_request" in arguments and not context["args"]:
                context["args"] = arguments["user_request"]

        if Template is not None:
            rendered = Template(definition.template).render(**context).strip()
        else:
            rendered = _render_without_jinja(definition.template, context).strip()
        return rendered, definition


class SkillService:
    def __init__(
        self,
        *,
        loader: SkillLoader | None = None,
        project_dir: str | Path | None = None,
    ) -> None:
        self.executor = SkillExecutor(loader=loader, project_dir=project_dir)

    async def handle(self, request: SkillRequest) -> SkillResponse:
        try:
            content, definition = self.executor.render(
                request.skill_name, request.arguments
            )
            return SkillResponse(
                request_id=request.request_id,
                content=content,
                metadata={
                    "skill_name": definition.name,
                    "description": definition.description,
                    "when_to_use": definition.when_to_use,
                    "source": definition.source,
                    "path": definition.path,
                },
            )
        except Exception as error:
            return SkillResponse(
                request_id=request.request_id,
                content="",
                metadata={"error": str(error), "skill_name": request.skill_name},
            )


def _render_without_jinja(template: str, context: dict[str, Any]) -> str:
    def replace(match: re.Match[str]) -> str:
        key = match.group(1).strip()
        value = context.get(key, "")
        return str(value)

    return re.sub(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}", replace, template)
