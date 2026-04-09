from __future__ import annotations

import asyncio
import tempfile
import unittest
from pathlib import Path

from agent_brain.advisor import AdvisorService, PromptTemplateEngine
from agent_brain.attachments import build_memory_attachment, build_note_attachment


class _FakeBackend:
    def __init__(self) -> None:
        self.requests = []

    async def stream_message(self, request):
        self.requests.append(request)
        yield {
            "type": "text_delta",
            "request_id": request.request_id,
            "delta": "ok",
        }


class AdvisorTests(unittest.TestCase):
    def test_prepare_command_renders_system_prompt_and_stream_request(self) -> None:
        service = AdvisorService()
        prepared = service.prepare_command(
            command_name="review",
            command_description="Review code changes.",
            project_dir="/tmp/project",
            user_request="Review the auth diff.",
            prompt_template="review.j2",
            attachments=[build_note_attachment("Diff summary", "Auth middleware changed.")],
            memories=[
                build_memory_attachment(
                    "User preference",
                    "Call out risky regressions first.",
                    memory_type="feedback",
                    scope="private",
                )
            ],
            template_context={
                "review_title": "Code Review",
                "review_mode": "general",
                "review_focus": "Look for regressions.",
                "review_output_format": "Findings first.",
            },
            output_expectations="Lead with findings.",
        )

        request = prepared.to_stream_request("req-1")
        self.assertEqual(prepared.model, "opus")
        self.assertIn("Memory Context", prepared.system_prompt)
        self.assertIn("Review the auth diff.", prepared.user_prompt)
        self.assertEqual(request.metadata["command_name"], "review")

    def test_stream_prepared_uses_backend(self) -> None:
        backend = _FakeBackend()
        service = AdvisorService(backend=backend)
        prepared = service.prepare_command(
            command_name="commit",
            command_description="Create a commit message.",
            project_dir="/tmp/project",
            user_request="Prepare a commit message.",
            prompt_template="commit.j2",
            template_context={
                "commit_style": "Imperative subject.",
                "commit_constraints": "Do not invent facts.",
            },
        )

        events = asyncio.run(self._collect(service.stream_prepared(prepared, request_id="req-2")))
        self.assertEqual(events[0]["delta"], "ok")
        self.assertEqual(backend.requests[0].metadata["command_name"], "commit")

    async def _collect(self, iterator):
        return [event async for event in iterator]


class PromptTemplateEngineTests(unittest.TestCase):
    def test_template_engine_reads_templates_from_custom_dir(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            template_dir = Path(temp_dir)
            (template_dir / "simple.j2").write_text("Hello {{ name }}", encoding="utf-8")
            engine = PromptTemplateEngine(prompts_dir=template_dir)

            rendered = engine.render("simple.j2", {"name": "Centaur"})

            self.assertEqual(rendered, "Hello Centaur")
