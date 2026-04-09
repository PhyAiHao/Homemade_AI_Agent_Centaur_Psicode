from __future__ import annotations

from .base import PromptCommand


ULTRAPLAN_PROMPT = """# Ultraplan

Goal:
{{ user_request }}

Produce a staged implementation plan that:
- Breaks the work into phases with clear milestones
- Calls out hidden dependencies and risky assumptions
- Includes validation gates for each stage
- Leaves room for incremental delivery instead of one large leap

Use the repo context below when forming the plan:

{{ attachment_prompt }}
"""


ULTRAPLAN_COMMAND = PromptCommand(
    name="ultraplan",
    description="Create a deep implementation plan before coding.",
    prompt_body=ULTRAPLAN_PROMPT,
    allowed_tools=["Read", "Grep", "Glob"],
    model="opus",
    max_output_tokens=3_500,
    output_expectations="Produce a staged, execution-ready plan with explicit verification points.",
)


def prepare_ultraplan(context, *, advisor_service=None):
    return ULTRAPLAN_COMMAND.prepare(context, advisor_service=advisor_service)
