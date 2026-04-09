from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from agent_brain.suggestions import PromptSuggestionService


class SuggestionTests(unittest.TestCase):
    def test_suggest_review_for_review_like_request(self) -> None:
        service = PromptSuggestionService()
        suggestions = service.suggest(
            [{"role": "user", "content": "Can you review this diff for regressions?"}]
        )

        self.assertEqual(suggestions[0].command_name, "review")

    def test_suggest_init_when_claude_md_is_missing(self) -> None:
        service = PromptSuggestionService()
        with tempfile.TemporaryDirectory() as temp_dir:
            suggestions = service.suggest(
                [{"role": "user", "content": "Help me get started in this repo."}],
                project_dir=Path(temp_dir),
            )

            self.assertIn("init", [suggestion.command_name for suggestion in suggestions])
