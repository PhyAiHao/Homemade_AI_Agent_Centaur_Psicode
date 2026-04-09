from __future__ import annotations

import re
from collections.abc import AsyncIterator
from pathlib import Path
from typing import Any

from ._compat import Field
from .api.client import AnthropicBackend, StreamRequest
from .attachments import AttachmentItem, render_attachment_bundle, render_memory_bundle
from .types.base import AgentBaseModel

try:
    from jinja2 import Environment, FileSystemLoader
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Environment = None  # type: ignore[assignment]
    FileSystemLoader = None  # type: ignore[assignment]


DEFAULT_COMMAND_MODELS = {
    "review": "opus",
    "security-review": "opus",
    "ultraplan": "opus",
    "insights": "opus",
    "commit": "sonnet",
    "init": "sonnet",
}


class PreparedCommand(AgentBaseModel):
    name: str
    model: str
    system_prompt: str
    user_prompt: str
    metadata: dict[str, Any] = Field(default_factory=dict)
    attachments: list[AttachmentItem] = Field(default_factory=list)
    allowed_tools: list[str] = Field(default_factory=list)
    max_output_tokens: int | None = None

    def to_stream_request(self, request_id: str) -> StreamRequest:
        metadata = dict(self.metadata)
        metadata.setdefault("command_name", self.name)
        return StreamRequest(
            request_id=request_id,
            model=self.model,
            system_prompt=self.system_prompt,
            messages=[{"role": "user", "content": self.user_prompt}],
            metadata=metadata,
            max_output_tokens=self.max_output_tokens,
        )


class PromptTemplateEngine:
    def __init__(self, *, prompts_dir: str | Path | None = None) -> None:
        self.prompts_dir = Path(
            prompts_dir or (Path(__file__).resolve().parent / "prompts")
        )
        if Environment is not None and FileSystemLoader is not None:
            self._environment = Environment(
                loader=FileSystemLoader(str(self.prompts_dir)),
                autoescape=False,
                trim_blocks=True,
                lstrip_blocks=True,
            )
        else:
            self._environment = None

    def render(self, template_name: str, context: dict[str, Any]) -> str:
        if self._environment is not None:
            return self._environment.get_template(template_name).render(**context).strip()

        template_text = (self.prompts_dir / template_name).read_text(encoding="utf-8")
        return _render_without_jinja(template_text, context).strip()


class AdvisorService:
    def __init__(
        self,
        *,
        backend: AnthropicBackend | None = None,
        template_engine: PromptTemplateEngine | None = None,
    ) -> None:
        self.backend = backend or AnthropicBackend()
        self.template_engine = template_engine or PromptTemplateEngine()

    def choose_model(self, command_name: str, explicit_model: str | None = None) -> str:
        return explicit_model or DEFAULT_COMMAND_MODELS.get(command_name, "sonnet")

    def render_memory_prompt(self, memories: list[AttachmentItem]) -> str:
        if not memories:
            return ""
        return self.template_engine.render(
            "memory.j2",
            {"memory_sections": render_memory_bundle(memories)},
        )

    def render_system_prompt(
        self,
        *,
        command_name: str,
        command_description: str,
        project_dir: str | Path,
        memory_prompt: str = "",
        output_expectations: str = "",
        extra_system_instructions: str = "",
    ) -> str:
        return self.template_engine.render(
            "system.j2",
            {
                "command_name": command_name,
                "command_description": command_description,
                "project_dir": str(Path(project_dir).resolve()),
                "memory_prompt": memory_prompt or "No persistent memory was attached.",
                "output_expectations": output_expectations or "Provide a useful, high-signal answer.",
                "extra_system_instructions": extra_system_instructions or "",
            },
        )

    def prepare_command(
        self,
        *,
        command_name: str,
        command_description: str,
        project_dir: str | Path,
        user_request: str,
        prompt_template: str | None = None,
        prompt_body: str | None = None,
        template_context: dict[str, Any] | None = None,
        attachments: list[AttachmentItem] | None = None,
        memories: list[AttachmentItem] | None = None,
        metadata: dict[str, Any] | None = None,
        allowed_tools: list[str] | None = None,
        model: str | None = None,
        max_output_tokens: int | None = None,
        output_expectations: str = "",
        extra_system_instructions: str = "",
    ) -> PreparedCommand:
        attachment_items = list(attachments or [])
        memory_items = list(memories or [])
        attachment_prompt = render_attachment_bundle(attachment_items)
        memory_prompt = self.render_memory_prompt(memory_items)
        context = {
            "command_name": command_name,
            "command_description": command_description,
            "project_dir": str(Path(project_dir).resolve()),
            "user_request": user_request.strip(),
            "attachment_prompt": attachment_prompt,
            "memory_prompt": memory_prompt or "No persistent memory was attached.",
            "output_expectations": output_expectations or "Provide a useful, high-signal answer.",
        }
        context.update(template_context or {})

        if prompt_template is not None:
            user_prompt = self.template_engine.render(prompt_template, context)
        elif prompt_body is not None:
            user_prompt = _render_without_jinja(prompt_body, context).strip()
        else:
            raise ValueError("prepare_command requires either prompt_template or prompt_body")

        system_prompt = self.render_system_prompt(
            command_name=command_name,
            command_description=command_description,
            project_dir=project_dir,
            memory_prompt=memory_prompt or "",
            output_expectations=output_expectations,
            extra_system_instructions=extra_system_instructions,
        )

        return PreparedCommand(
            name=command_name,
            model=self.choose_model(command_name, model),
            system_prompt=system_prompt,
            user_prompt=user_prompt,
            metadata=dict(metadata or {}),
            attachments=attachment_items + memory_items,
            allowed_tools=list(allowed_tools or []),
            max_output_tokens=max_output_tokens,
        )

    async def stream_prepared(
        self,
        prepared_command: PreparedCommand,
        *,
        request_id: str,
    ) -> AsyncIterator[dict[str, Any]]:
        async for event in self.backend.stream_message(
            prepared_command.to_stream_request(request_id)
        ):
            yield event


def _render_without_jinja(template: str, context: dict[str, Any]) -> str:
    def replace(match: re.Match[str]) -> str:
        key = match.group(1).strip()
        value = context.get(key, "")
        return str(value)

    return re.sub(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}", replace, template)
