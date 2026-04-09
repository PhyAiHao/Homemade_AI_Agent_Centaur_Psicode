"""Tests for the IPC bridge client.

These tests require a running agent-brain process.
Skip with: pytest -m "not integration"
"""

from __future__ import annotations

import pytest
from ..ipc_bridge import CentaurIpcClient
from ..launcher import is_socket_alive

SOCKET = "/tmp/agent-ipc.sock"


def requires_agent_brain():
    """Skip test if agent-brain is not running."""
    if not is_socket_alive(SOCKET):
        pytest.skip("agent-brain not running at " + SOCKET)


# ── Connection tests ────────────────────────────────────────────────────


@pytest.mark.asyncio
@pytest.mark.integration
async def test_connect_and_ping():
    """Connect to agent-brain and send a ping."""
    requires_agent_brain()
    client = CentaurIpcClient(SOCKET)
    await client.connect()
    try:
        resp = await client.ping(timeout=5.0)
        assert resp["type"] == "ipc_pong"
        assert resp["status"] == "ok"
        assert "uptime_ms" in resp
    finally:
        await client.close()


@pytest.mark.asyncio
@pytest.mark.integration
async def test_memory_recall():
    """Search the memory system via IPC."""
    requires_agent_brain()
    client = CentaurIpcClient(SOCKET)
    await client.connect()
    try:
        memories = await client.recall_memories("test query", limit=3)
        assert isinstance(memories, list)
        # May be empty if no memories exist, but should not error
    finally:
        await client.close()


@pytest.mark.asyncio
@pytest.mark.integration
async def test_run_agent_task():
    """Send a simple prompt and get a response."""
    requires_agent_brain()
    client = CentaurIpcClient(SOCKET)
    await client.connect()
    try:
        result = await client.run_agent_task(
            prompt="What is 2 + 2? Answer with just the number.",
            model="claude-sonnet-4-6",
            timeout=30.0,
        )
        assert isinstance(result, str)
        assert len(result) > 0
    finally:
        await client.close()


# ── Unit tests (no agent-brain required) ────────────────────────────────


def test_client_defaults():
    """Test default configuration."""
    client = CentaurIpcClient()
    assert client.socket_path == "/tmp/agent-ipc.sock"
    assert not client.is_connected


def test_client_custom_socket():
    """Test custom socket path."""
    client = CentaurIpcClient("/tmp/custom.sock")
    assert client.socket_path == "/tmp/custom.sock"
