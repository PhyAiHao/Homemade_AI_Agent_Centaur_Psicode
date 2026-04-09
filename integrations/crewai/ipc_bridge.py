"""Lightweight IPC client for Centaur Psicode's agent-brain.

Speaks the same protocol as agent-core/src/ipc.rs:
  - Transport: Unix domain socket
  - Framing: 4-byte big-endian length prefix + msgpack payload
  - Messages: typed dicts with "type" field

This client is independent of the agent-core Rust binary — it talks
directly to the Python agent-brain process over the IPC socket.
"""

from __future__ import annotations

import asyncio
import struct
from collections.abc import AsyncIterator
from typing import Any
from uuid import uuid4

try:
    import msgpack
except ImportError:
    raise ImportError("msgpack is required: pip install msgpack")


class CentaurIpcClient:
    """Async IPC client for Centaur Psicode's agent-brain."""

    def __init__(self, socket_path: str = "/tmp/agent-ipc.sock"):
        self.socket_path = socket_path
        self._reader: asyncio.StreamReader | None = None
        self._writer: asyncio.StreamWriter | None = None

    # ── Connection lifecycle ────────────────────────────────────────────

    async def connect(self) -> None:
        """Connect to the agent-brain IPC socket."""
        self._reader, self._writer = await asyncio.open_unix_connection(
            self.socket_path
        )

    async def close(self) -> None:
        """Close the connection."""
        if self._writer:
            self._writer.close()
            await self._writer.wait_closed()
            self._writer = None
            self._reader = None

    @property
    def is_connected(self) -> bool:
        return self._writer is not None and not self._writer.is_closing()

    # ── Wire protocol ───────────────────────────────────────────────────

    async def send_message(self, msg: dict[str, Any]) -> None:
        """Send a msgpack-framed message."""
        assert self._writer is not None, "Not connected"
        payload = msgpack.packb(msg, use_bin_type=True)
        length = struct.pack(">I", len(payload))
        self._writer.write(length + payload)
        await self._writer.drain()

    async def recv_message(self) -> dict[str, Any]:
        """Receive a single msgpack-framed message."""
        assert self._reader is not None, "Not connected"
        length_bytes = await self._reader.readexactly(4)
        length = struct.unpack(">I", length_bytes)[0]
        if length == 0 or length > 128 * 1024 * 1024:
            raise ValueError(f"IPC message length out of bounds: {length}")
        payload = await self._reader.readexactly(length)
        return msgpack.unpackb(payload, raw=False)

    # ── Request/response patterns ───────────────────────────────────────

    async def request(self, msg: dict[str, Any], timeout: float = 60.0) -> dict[str, Any]:
        """Send a message and wait for a single response."""
        await self.send_message(msg)
        return await asyncio.wait_for(self.recv_message(), timeout=timeout)

    async def stream_request(
        self, msg: dict[str, Any], timeout: float = 300.0
    ) -> AsyncIterator[dict[str, Any]]:
        """Send a request on a DEDICATED connection and yield streaming responses.

        Opens a new connection for this request (streaming needs its own socket
        because multiple events flow back). Closes when message_done is received.
        """
        reader, writer = await asyncio.open_unix_connection(self.socket_path)
        try:
            payload = msgpack.packb(msg, use_bin_type=True)
            writer.write(struct.pack(">I", len(payload)) + payload)
            await writer.drain()

            while True:
                length_bytes = await asyncio.wait_for(
                    reader.readexactly(4), timeout=timeout
                )
                length = struct.unpack(">I", length_bytes)[0]
                resp_payload = await reader.readexactly(length)
                resp = msgpack.unpackb(resp_payload, raw=False)
                yield resp
                if resp.get("type") == "message_done":
                    break
        finally:
            writer.close()
            await writer.wait_closed()

    # ── Convenience methods ─────────────────────────────────────────────

    async def ping(self, timeout: float = 5.0) -> dict[str, Any]:
        """Check if agent-brain is alive. Returns pong with uptime."""
        return await self.request(
            {"type": "ipc_ping", "request_id": str(uuid4())},
            timeout=timeout,
        )

    async def run_agent_task(
        self,
        prompt: str,
        model: str = "claude-sonnet-4-6",
        system_prompt: str = "",
        provider: str = "first_party",
        api_key: str | None = None,
        max_output_tokens: int = 16384,
        timeout: float = 300.0,
    ) -> str:
        """Send a full prompt to agent-brain and collect the streaming response.

        This bypasses the Rust query loop — it sends directly to the Python
        brain which calls the LLM API and streams back text deltas.
        """
        request = {
            "type": "api_request",
            "request_id": str(uuid4()),
            "model": model,
            "messages": [{"role": "user", "content": prompt}],
            "tools": [],
            "system_prompt": system_prompt or None,
            "max_output_tokens": max_output_tokens,
            "metadata": {},
            "tool_choice": None,
            "thinking": None,
            "betas": [],
            "provider": provider,
            "api_key": api_key,
            "base_url": None,
            "fast_mode": False,
        }

        text_parts: list[str] = []
        async for event in self.stream_request(request, timeout=timeout):
            event_type = event.get("type", "")
            if event_type == "text_delta":
                text_parts.append(event.get("delta", ""))
            elif event_type == "message_done":
                break

        return "".join(text_parts)

    async def recall_memories(
        self, query: str, limit: int = 5, timeout: float = 30.0
    ) -> list[dict[str, Any]]:
        """Search the agent's memory system for relevant context."""
        resp = await self.request(
            {
                "type": "memory_request",
                "request_id": str(uuid4()),
                "action": "recall",
                "payload": {"query": query, "limit": limit},
            },
            timeout=timeout,
        )
        if resp.get("ok"):
            return resp.get("payload", {}).get("memories", [])
        return []
