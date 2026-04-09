from __future__ import annotations

from .base import PromptCommand


INIT_PROMPT = """# Repository Initialization

Goal:
{{ user_request }}

Create or refine minimal onboarding guidance for this repository.

Expected work:
- Identify the key build, test, lint, and development commands
- Summarize the high-level architecture and non-obvious repo workflows
- Draft or improve `CLAUDE.md` guidance without bloating it
- Call out information that still requires user confirmation rather than inventing it

Reference context:

{{ attachment_prompt }}
"""


INIT_COMMAND = PromptCommand(
    name="init",
    description="Initialize or refine repo guidance such as CLAUDE.md.",
    prompt_body=INIT_PROMPT,
    allowed_tools=["Read", "Grep", "Glob", "LS"],
    model="sonnet",
    max_output_tokens=3_000,
    output_expectations="Focus on concise onboarding guidance and explicit unknowns.",
)


def prepare_init(context, *, advisor_service=None):
    return INIT_COMMAND.prepare(context, advisor_service=advisor_service)
