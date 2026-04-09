from __future__ import annotations

import unittest

from agent_brain.attachments import build_memory_attachment, build_note_attachment
from agent_brain.commands import (
    CommandContext,
    prepare_commit,
    prepare_init,
    prepare_insights,
    prepare_review,
    prepare_security_review,
    prepare_ultraplan,
)


class CommandTests(unittest.TestCase):
    def setUp(self) -> None:
        self.context = CommandContext(
            project_dir="/tmp/project",
            user_request="Investigate the latest auth changes.",
            attachments=[build_note_attachment("Diff", "Auth middleware and session checks changed.")],
            memories=[
                build_memory_attachment(
                    "User preference",
                    "Keep review output terse.",
                    memory_type="feedback",
                    scope="private",
                )
            ],
        )

    def test_review_command_builds_review_prompt(self) -> None:
        prepared = prepare_review(self.context)
        self.assertEqual(prepared.name, "review")
        self.assertIn("Find correctness bugs", prepared.user_prompt)
        self.assertIn("Auth middleware", prepared.user_prompt)

    def test_security_review_command_changes_focus(self) -> None:
        prepared = prepare_security_review(self.context)
        self.assertEqual(prepared.name, "security-review")
        self.assertIn("high-confidence security findings", prepared.user_prompt)
        self.assertIn("security audit", prepared.system_prompt)

    def test_commit_command_uses_commit_template(self) -> None:
        prepared = prepare_commit(self.context)
        self.assertEqual(prepared.name, "commit")
        self.assertIn("Commit Message Generation", prepared.user_prompt)
        self.assertIn("git commit", " ".join(prepared.allowed_tools))

    def test_planning_and_repo_commands_prepare_prompts(self) -> None:
        ultraplan = prepare_ultraplan(self.context)
        init = prepare_init(self.context)
        insights = prepare_insights(self.context)

        self.assertIn("staged implementation plan", ultraplan.user_prompt)
        self.assertIn("CLAUDE.md", init.user_prompt)
        self.assertIn("architectural themes", insights.user_prompt)
