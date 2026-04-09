from __future__ import annotations

from typing import Any

from .._compat import Field
from ..advisor import AdvisorService, PreparedCommand
from ..attachments import AttachmentItem
from ..types.base import AgentBaseModel


class CommandContext(AgentBaseModel):
    project_dir: str
    user_request: str = ""
    attachments: list[AttachmentItem] = Field(default_factory=list)
    memories: list[AttachmentItem] = Field(default_factory=list)
    metadata: dict[str, Any] = Field(default_factory=dict)


class PromptCommand:
    def __init__(
        self,
        *,
        name: str,
        description: str,
        prompt_template: str | None = None,
        prompt_body: str | None = None,
        template_context: dict[str, Any] | None = None,
        allowed_tools: list[str] | None = None,
        model: str | None = None,
        max_output_tokens: int | None = None,
        output_expectations: str = "",
        extra_system_instructions: str = "",
    ) -> None:
        self.name = name
        self.description = description
        self.prompt_template = prompt_template
        self.prompt_body = prompt_body
        self.template_context = dict(template_context or {})
        self.allowed_tools = list(allowed_tools or [])
        self.model = model
        self.max_output_tokens = max_output_tokens
        self.output_expectations = output_expectations
        self.extra_system_instructions = extra_system_instructions

    def prepare(
        self,
        context: CommandContext,
        *,
        advisor_service: AdvisorService | None = None,
    ) -> PreparedCommand:
        service = advisor_service or AdvisorService()
        template_context = dict(self.template_context)
        template_context.update(context.metadata)
        return service.prepare_command(
            command_name=self.name,
            command_description=self.description,
            project_dir=context.project_dir,
            user_request=context.user_request or self.description,
            prompt_template=self.prompt_template,
            prompt_body=self.prompt_body,
            template_context=template_context,
            attachments=context.attachments,
            memories=context.memories,
            metadata={"command_name": self.name, **context.metadata},
            allowed_tools=self.allowed_tools,
            model=self.model,
            max_output_tokens=self.max_output_tokens,
            output_expectations=self.output_expectations,
            extra_system_instructions=self.extra_system_instructions,
        )
