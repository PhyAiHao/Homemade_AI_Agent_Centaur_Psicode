from __future__ import annotations

import unittest

from agent_brain.summary import AgentSummaryService


class AgentSummaryTests(unittest.TestCase):
    def test_progress_summary_prefers_recent_file_focus(self) -> None:
        service = AgentSummaryService()
        summary = service.summarize_progress(
            [
                {"role": "user", "content": "Please update the chrome bridge."},
                {
                    "role": "assistant",
                    "content": "Inspecting agent-integrations/chrome/setup.ts and wiring manifest helpers.",
                },
                {
                    "role": "assistant",
                    "content": [
                        {
                            "type": "text",
                            "text": "Updating setup.ts to install the native host manifest.",
                        }
                    ],
                },
            ]
        )

        self.assertIn("Updating", summary.headline)
        self.assertIn("setup.ts", summary.headline)
        self.assertEqual(summary.focus, "setup.ts")
        self.assertIn("native host manifest", summary.detail)

    def test_capability_summary_groups_surface(self) -> None:
        service = AgentSummaryService()
        summary = service.summarize_capabilities(
            commands=["review", "commit", "init"],
            tools=["WebFetch", "LSPTool"],
            skills=["review", "remember"],
            plugins=["diagnostics"],
            modes=["default", "plan"],
            constraints=["Browser automation is disabled until the extension is installed."],
        )

        self.assertIn("3 commands", summary.headline)
        self.assertEqual(summary.suggested_commands, ["review", "commit", "init"])
        category_names = [category.name for category in summary.categories]
        self.assertIn("tools", category_names)
        self.assertIn("plugins", category_names)
        self.assertEqual(len(summary.constraints), 1)
