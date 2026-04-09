from __future__ import annotations

from .base import PromptCommand


SECURITY_REVIEW_COMMAND = PromptCommand(
    name="security-review",
    description="Perform a security-focused review of pending changes.",
    prompt_template="review.j2",
    template_context={
        "review_title": "Security Review",
        "review_mode": "security",
        "review_focus": (
            "Focus on high-confidence security findings only: auth bypass, privilege "
            "escalation, secret exposure, injection flaws, unsafe deserialization, "
            "data leakage, and broken trust boundaries."
        ),
        "review_output_format": (
            "Report only concrete medium/high findings with exploit scenario, impact, "
            "and fix recommendation. Do not include speculative or low-signal issues."
        ),
    },
    allowed_tools=["Read", "Grep", "Glob"],
    model="opus",
    max_output_tokens=3_500,
    output_expectations="Minimize false positives and keep the report security-focused.",
    extra_system_instructions=(
        "Treat this as a security audit, not a general style review. Prefer fewer, higher-confidence findings."
    ),
)


def prepare_security_review(context, *, advisor_service=None):
    return SECURITY_REVIEW_COMMAND.prepare(context, advisor_service=advisor_service)
