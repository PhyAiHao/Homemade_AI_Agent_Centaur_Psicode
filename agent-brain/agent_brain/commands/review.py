from __future__ import annotations

from .base import PromptCommand


REVIEW_COMMAND = PromptCommand(
    name="review",
    description="Review code changes for bugs, regressions, and missing tests.",
    prompt_template="review.j2",
    template_context={
        "review_title": "Code Review",
        "review_mode": "general",
        "review_focus": (
            "Find correctness bugs, behavioral regressions, unsafe assumptions, "
            "missing tests, and rollout risks."
        ),
        "review_output_format": (
            "List findings first, ordered by severity. Cite files and line numbers "
            "when the attachments make that possible. If you find no issues, say so explicitly."
        ),
    },
    allowed_tools=["Read", "Grep", "Glob"],
    model="opus",
    max_output_tokens=3_000,
    output_expectations="Lead with concrete findings and keep any summary brief.",
)


def prepare_review(context, *, advisor_service=None):
    return REVIEW_COMMAND.prepare(context, advisor_service=advisor_service)
