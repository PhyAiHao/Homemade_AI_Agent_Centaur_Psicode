from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from agent_brain.skills import SkillExecutor, SkillLoader, SkillService


EXPECTED_BUNDLED_SKILLS = {
    "commit",
    "review",
    "ultraplan",
    "simplify",
    "remember",
    "loop",
    "schedule",
    "debug",
    "batch",
    "claudeApi",
    "verify",
}


class SkillTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.loader = SkillLoader(user_skills_dir=self.temp_dir.name)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_loader_finds_all_bundled_skills(self) -> None:
        skills = self.loader.load_all()
        self.assertTrue(EXPECTED_BUNDLED_SKILLS.issubset(set(skills.keys())))

    def test_executor_expands_all_bundled_skills(self) -> None:
        executor = SkillExecutor(loader=self.loader, project_dir="/tmp/project")
        for skill_name in EXPECTED_BUNDLED_SKILLS:
            content, definition = executor.render(
                skill_name,
                {"user_request": f"Run {skill_name} on this task"},
            )
            self.assertEqual(definition.name, skill_name)
            self.assertIn(f"Run {skill_name} on this task", content)

    def test_user_skill_overrides_bundled_skill(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            user_dir = Path(temp_dir)
            (user_dir / "commit.yaml").write_text(
                "\n".join(
                    [
                        "name: commit",
                        "description: User override",
                        "when_to_use: test override",
                        "template: |",
                        "  Override prompt",
                        "  {{ user_request }}",
                    ]
                ),
                encoding="utf-8",
            )
            loader = SkillLoader(user_skills_dir=user_dir)
            executor = SkillExecutor(loader=loader)
            content, definition = executor.render("commit", {"user_request": "special"})
            self.assertEqual(definition.description, "User override")
            self.assertIn("special", content)

    def test_skill_service_returns_metadata(self) -> None:
        service = SkillService(loader=self.loader, project_dir="/tmp/project")

        class Request:
            request_id = "skill-1"
            skill_name = "verify"
            arguments = {"user_request": "Verify the new memory flow"}

        response = self._run_async(service.handle(Request()))
        self.assertEqual(response.metadata["skill_name"], "verify")
        self.assertIn("Verify the new memory flow", response.content)

    def _run_async(self, awaitable):
        import asyncio

        return asyncio.run(awaitable)
