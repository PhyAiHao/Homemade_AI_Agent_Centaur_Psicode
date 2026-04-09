from __future__ import annotations

from .base import PromptCommand


INSIGHTS_PROMPT = """# Codebase Insights

Question:
{{ user_request }}

Analyze the attached repository context and produce:
- Major architectural themes
- High-churn or high-risk areas worth watching
- Notable workflows or commands that shape daily development
- Concrete follow-up questions or next investigations

Context:

{{ attachment_prompt }}
"""


INSIGHTS_COMMAND = PromptCommand(
    name="insights",
    description="Generate high-level codebase insights and hotspots.",
    prompt_body=INSIGHTS_PROMPT,
    allowed_tools=["Read", "Grep", "Glob", "LS"],
    model="opus",
    max_output_tokens=3_000,
    output_expectations="Return architectural insights, hotspots, and actionable next questions.",
)


def prepare_insights(context, *, advisor_service=None):
    return INSIGHTS_COMMAND.prepare(context, advisor_service=advisor_service)
