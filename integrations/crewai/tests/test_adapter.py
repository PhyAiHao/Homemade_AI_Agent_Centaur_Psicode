"""Tests for the Centaur Psicode CrewAI adapter.

Unit tests (no agent-brain required) + integration tests (require agent-brain).
"""

from __future__ import annotations

from unittest.mock import AsyncMock, MagicMock, patch

import pytest


# ── Unit tests (mock IPC) ───────────────────────────────────────────────


class TestBuildPrompt:
    """Test prompt construction from CrewAI tasks."""

    def _make_adapter(self):
        # Import inside test to handle optional crewai dependency
        from ..centaur_adapter import CentaurPsicodeAdapter

        return CentaurPsicodeAdapter(
            role="Developer",
            goal="Write code",
            backstory="Expert coder.",
        )

    def test_basic_task_prompt(self):
        adapter = self._make_adapter()
        task = MagicMock()
        task.description = "Fix the login bug"
        task.expected_output = "Summary of changes"

        prompt = adapter._build_prompt(task)
        assert "Fix the login bug" in prompt
        assert "Summary of changes" in prompt

    def test_prompt_with_context(self):
        adapter = self._make_adapter()
        task = MagicMock()
        task.description = "Review the code"
        task.expected_output = "Review comments"

        prompt = adapter._build_prompt(task, context="Previous task fixed auth.py")
        assert "Review the code" in prompt
        assert "Previous task fixed auth.py" in prompt

    def test_prompt_with_structured_output(self):
        from pydantic import BaseModel

        class ReviewResult(BaseModel):
            approved: bool
            comments: list[str]

        adapter = self._make_adapter()
        task = MagicMock()
        task.description = "Review code"
        task.expected_output = "JSON review"
        task.output_json = ReviewResult
        task.output_pydantic = None
        task.response_model = None

        adapter.configure_structured_output(task)
        prompt = adapter._build_prompt(task)
        assert "JSON" in prompt or "schema" in prompt.lower()


class TestConfigureTools:
    """Test tool description generation."""

    def _make_adapter(self):
        from ..centaur_adapter import CentaurPsicodeAdapter

        return CentaurPsicodeAdapter(
            role="Developer",
            goal="Write code",
            backstory="Expert.",
        )

    def test_no_tools(self):
        adapter = self._make_adapter()
        adapter.configure_tools(None)
        assert adapter._external_tools_desc == ""

    def test_with_tools(self):
        adapter = self._make_adapter()
        tool = MagicMock()
        tool.name = "SearchDocs"
        tool.description = "Search documentation"
        tool.args_schema = None

        adapter.configure_tools([tool])
        assert "SearchDocs" in adapter._external_tools_desc
        assert "Search documentation" in adapter._external_tools_desc


class TestSystemPrompt:
    """Test system prompt construction."""

    def test_includes_role_and_goal(self):
        from ..centaur_adapter import CentaurPsicodeAdapter

        adapter = CentaurPsicodeAdapter(
            role="Security Engineer",
            goal="Find vulnerabilities",
            backstory="OWASP expert.",
        )
        system = adapter._build_system_prompt()
        assert "Security Engineer" in system
        assert "Find vulnerabilities" in system
        assert "OWASP expert" in system

    def test_includes_external_tools(self):
        from ..centaur_adapter import CentaurPsicodeAdapter

        adapter = CentaurPsicodeAdapter(
            role="Dev", goal="Code", backstory="",
        )
        tool = MagicMock()
        tool.name = "Jira"
        tool.description = "Search Jira tickets"
        tool.args_schema = None
        adapter.configure_tools([tool])

        system = adapter._build_system_prompt()
        assert "Jira" in system
