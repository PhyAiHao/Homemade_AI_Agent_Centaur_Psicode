from __future__ import annotations

from .base import PromptCommand


COMMIT_COMMAND = PromptCommand(
    name="commit",
    description="Generate a focused commit message from the current diff.",
    prompt_template="commit.j2",
    template_context={
        "commit_style": (
            "Use a short imperative subject line. Add a body only when extra context "
            "helps explain why the change exists."
        ),
        "commit_constraints": (
            "Do not invent files, tests, or rationale that are not present in the attachments. "
            "Prefer one coherent commit boundary."
        ),
    },
    allowed_tools=["Bash(git status:*)", "Bash(git diff:*)", "Bash(git commit:*)"],
    model="sonnet",
    max_output_tokens=1_500,
    output_expectations="Produce a commit message that is accurate, concise, and ready to use.",
)


def prepare_commit(context, *, advisor_service=None):
    return COMMIT_COMMAND.prepare(context, advisor_service=advisor_service)
