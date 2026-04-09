from __future__ import annotations

import asyncio
import tempfile
import unittest
from pathlib import Path

from agent_brain.api.client import StreamRequest
from agent_brain.ipc_server import IpcServer
from agent_brain.ipc_wire import read_frame, write_frame
from agent_brain.skills import SkillLoader, SkillService
from agent_brain.voice import VoiceModeClient


class _FakeBackend:
    def __init__(self):
        self.requests = []

    async def stream_message(self, request: StreamRequest):
        self.requests.append(request)
        yield {
            "type": "text_delta",
            "request_id": request.request_id,
            "delta": "Hello from IPC",
        }
        yield {
            "type": "tool_use",
            "request_id": request.request_id,
            "tool_call_id": "tool-1",
            "name": "Bash",
            "input": {"command": "ls"},
        }
        yield {
            "type": "message_done",
            "request_id": request.request_id,
            "usage": {"input_tokens": 10, "output_tokens": 4},
            "stop_reason": "end_turn",
        }


class IpcServerTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        socket_path = Path(self.temp_dir.name) / "agent-ipc.sock"
        self.backend = _FakeBackend()
        styles_dir = Path(self.temp_dir.name) / "output-styles"
        styles_dir.mkdir(parents=True, exist_ok=True)
        (styles_dir / "focus.md").write_text(
            "\n".join(
                [
                    "---",
                    "name: focus",
                    "description: Focus style",
                    "---",
                    "Focus on the most important technical details.",
                ]
            ),
            encoding="utf-8",
        )
        self.skill_service = SkillService(
            loader=SkillLoader(user_skills_dir=Path(self.temp_dir.name) / "user-skills"),
            project_dir=self.temp_dir.name,
        )
        self.server = await IpcServer(
            socket_path=socket_path,
            backend=self.backend,
            plugin_root_dir=Path(self.temp_dir.name) / "plugins",
            output_styles_dir=styles_dir,
            skill_handler=self.skill_service.handle,
        ).start()
        self.socket_path = socket_path

    async def asyncTearDown(self) -> None:
        await self.server.close()
        self.temp_dir.cleanup()

    async def test_api_request_streams_events(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        await write_frame(
            writer,
            {
                "type": "api_request",
                "request_id": "req-1",
                "messages": [{"role": "user", "content": "hi"}],
                "model": "sonnet",
            },
        )

        first = await read_frame(reader)
        second = await read_frame(reader)
        third = await read_frame(reader)

        self.assertEqual(first["type"], "text_delta")
        self.assertEqual(first["delta"], "Hello from IPC")
        self.assertEqual(second["type"], "tool_use")
        self.assertEqual(second["input"], {"command": "ls"})
        self.assertEqual(third["type"], "message_done")
        self.assertEqual(third["stop_reason"], "end_turn")
        self.assertEqual(self.backend.requests[0].model, "sonnet")

        writer.close()
        await writer.wait_closed()

    async def test_memory_request_uses_real_memory_service(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        await write_frame(
            writer,
            {
                "type": "memory_request",
                "request_id": "mem-1",
                "action": "save",
                "payload": {
                    "title": "Feedback: keep replies terse",
                    "body": "The user prefers terse close-out messages.",
                    "memory_type": "feedback",
                    "scope": "private",
                    "description": "User prefers terse close-out messages.",
                },
            },
        )

        save_response = await read_frame(reader)
        self.assertEqual(save_response["type"], "memory_response")
        self.assertTrue(save_response["ok"])

        await write_frame(
            writer,
            {
                "type": "memory_request",
                "request_id": "mem-2",
                "action": "list",
                "payload": {"include_team": False},
            },
        )
        list_response = await read_frame(reader)
        self.assertEqual(list_response["type"], "memory_response")
        self.assertTrue(list_response["ok"])
        self.assertEqual(len(list_response["payload"]["items"]), 1)

        writer.close()
        await writer.wait_closed()

    async def test_cost_request_returns_usage_report(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        await write_frame(
            writer,
            {
                "type": "api_request",
                "request_id": "req-cost",
                "messages": [{"role": "user", "content": "hi"}],
                "model": "sonnet",
            },
        )

        await read_frame(reader)
        await read_frame(reader)
        await read_frame(reader)

        await write_frame(
            writer,
            {
                "type": "cost_request",
                "request_id": "cost-1",
            },
        )
        response = await read_frame(reader)
        self.assertEqual(response["type"], "cost_response")
        self.assertEqual(response["usage"]["request_count"], 1)
        self.assertEqual(response["usage"]["successful_requests"], 1)
        self.assertEqual(response["usage"]["total_input_tokens"], 10)
        self.assertIn("tengu_api_success", response["diagnostics"]["event_counts"])

        await write_frame(
            writer,
            {
                "type": "cost_request",
                "request_id": "cost-2",
                "reset": True,
            },
        )
        reset_response = await read_frame(reader)
        self.assertEqual(reset_response["usage"]["request_count"], 1)

        await write_frame(
            writer,
            {
                "type": "cost_request",
                "request_id": "cost-3",
            },
        )
        cleared_response = await read_frame(reader)
        self.assertEqual(cleared_response["usage"]["request_count"], 0)

        writer.close()
        await writer.wait_closed()

    async def test_compact_request_uses_real_compact_service(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        messages = []
        for index in range(100):
            role = "user" if index % 2 == 0 else "assistant"
            messages.append(
                {
                    "role": role,
                    "content": (
                        f"Message {index}: "
                        "Carry forward the latest implementation details. " * 5
                    ),
                }
            )

        await write_frame(
            writer,
            {
                "type": "compact_request",
                "request_id": "compact-1",
                "messages": messages,
                "token_budget": 2_500,
            },
        )

        response = await read_frame(reader)
        self.assertEqual(response["type"], "compact_response")
        self.assertIn("Primary request:", response["summary"])
        self.assertLess(len(response["messages"]), len(messages))

        writer.close()
        await writer.wait_closed()

    async def test_skill_request_uses_real_skill_service(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        await write_frame(
            writer,
            {
                "type": "skill_request",
                "request_id": "skill-1",
                "skill_name": "verify",
                "arguments": {"user_request": "Verify the new skill IPC flow"},
            },
        )

        response = await read_frame(reader)
        self.assertEqual(response["type"], "skill_response")
        self.assertEqual(response["metadata"]["skill_name"], "verify")
        self.assertIn("Verify the new skill IPC flow", response["content"])

        writer.close()
        await writer.wait_closed()

    async def test_output_style_request_uses_real_output_style_service(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        await write_frame(
            writer,
            {
                "type": "output_style_request",
                "request_id": "style-1",
                "style_name": "focus",
            },
        )

        response = await read_frame(reader)
        self.assertEqual(response["type"], "output_style_response")
        self.assertEqual(response["style"]["name"], "focus")
        self.assertIn("important technical details", response["style"]["prompt"])

        writer.close()
        await writer.wait_closed()

    async def test_voice_start_uses_real_voice_service(self) -> None:
        reader, writer = await asyncio.open_unix_connection(str(self.socket_path))
        captured = VoiceModeClient().capture_text_audio("transcribe the auth failure")
        await write_frame(
            writer,
            {
                "type": "voice_start",
                "request_id": "voice-1",
                "language": "en",
                "audio_b64": captured.audio_b64,
                "recent_files": ["src/services/voice.ts"],
                "project_dir": self.temp_dir.name,
                "branch_name": "feat/voice-system",
            },
        )

        response = await read_frame(reader)
        self.assertEqual(response["type"], "voice_transcript")
        self.assertEqual(response["text"], "transcribe the auth failure")
        self.assertEqual(response["metadata"]["source"], "embedded_hint")

        writer.close()
        await writer.wait_closed()
