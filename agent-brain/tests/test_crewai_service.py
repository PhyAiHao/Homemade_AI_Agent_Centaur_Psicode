"""Tests for the CrewAI service.

Unit tests that verify config parsing and error handling.
Integration tests (marked) require crewai to be installed.
"""

from __future__ import annotations

import pytest

from agent_brain.crewai_service import CrewAIService
from agent_brain.ipc_types import MemoryRequest


@pytest.fixture
def service():
    return CrewAIService()


def _make_request(payload: dict) -> MemoryRequest:
    return MemoryRequest(
        request_id="test-1",
        action="crewai_run",
        payload=payload,
    )


# ── Error handling tests ────────────────────────────────────────────────


@pytest.mark.asyncio
async def test_missing_crew_config(service):
    """Should fail gracefully when crew_config is missing."""
    req = _make_request({"inputs": {}})
    resp = await service.handle(req)
    assert resp.ok is False
    assert "crew_config" in resp.error


@pytest.mark.asyncio
async def test_empty_agents(service):
    """Should fail when no agents defined."""
    req = _make_request({
        "crew_config": {"agents": [], "tasks": [{"description": "test", "agent": "x"}]},
    })
    resp = await service.handle(req)
    assert resp.ok is False
    assert "agent" in resp.error.lower()


@pytest.mark.asyncio
async def test_empty_tasks(service):
    """Should fail when no tasks defined."""
    req = _make_request({
        "crew_config": {
            "agents": [{"name": "a", "role": "Dev", "goal": "Code"}],
            "tasks": [],
        },
    })
    resp = await service.handle(req)
    assert resp.ok is False
    assert "task" in resp.error.lower()


# ── Config parsing tests ────────────────────────────────────────────────


@pytest.mark.asyncio
@pytest.mark.integration
async def test_valid_sequential_crew(service):
    """Full integration: run a minimal sequential crew."""
    req = _make_request({
        "crew_config": {
            "agents": [
                {"name": "analyst", "role": "Analyst", "goal": "Analyze data", "backstory": "Expert analyst"},
            ],
            "tasks": [
                {"description": "Summarize what 2+2 equals", "expected_output": "A number", "agent": "analyst"},
            ],
            "process": "sequential",
        },
        "inputs": {},
    })
    resp = await service.handle(req)
    # This will fail if crewai is not installed, which is expected
    if "not installed" in (resp.error or ""):
        pytest.skip("crewai not installed")
    assert resp.ok is True
    assert "result" in resp.payload


@pytest.mark.asyncio
@pytest.mark.integration
async def test_context_indices_wiring(service):
    """Test that context_indices correctly wire task dependencies."""
    req = _make_request({
        "crew_config": {
            "agents": [
                {"name": "a", "role": "Writer", "goal": "Write", "backstory": ""},
                {"name": "b", "role": "Editor", "goal": "Edit", "backstory": ""},
            ],
            "tasks": [
                {"description": "Write a haiku", "expected_output": "A haiku", "agent": "a"},
                {"description": "Edit the haiku", "expected_output": "An edited haiku", "agent": "b", "context_indices": [0]},
            ],
            "process": "sequential",
        },
        "inputs": {},
    })
    resp = await service.handle(req)
    if "not installed" in (resp.error or ""):
        pytest.skip("crewai not installed")
    assert resp.ok is True
