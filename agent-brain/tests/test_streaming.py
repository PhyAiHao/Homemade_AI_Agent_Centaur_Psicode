from __future__ import annotations

import asyncio
import unittest

from agent_brain.api.streaming import AnthropicStreamNormalizer, parse_sse_events


class StreamingTests(unittest.IsolatedAsyncioTestCase):
    async def test_normalizer_emits_text_delta_tool_use_and_done(self) -> None:
        async def raw_events():
            yield {
                "type": "message_start",
                "message": {"usage": {"input_tokens": 11}},
            }
            yield {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text"},
            }
            yield {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hello"},
            }
            yield {
                "type": "content_block_start",
                "index": 1,
                "content_block": {
                    "type": "tool_use",
                    "id": "tool-1",
                    "name": "Bash",
                },
            }
            yield {
                "type": "content_block_delta",
                "index": 1,
                "delta": {"type": "input_json_delta", "partial_json": '{"command":"ls"}'},
            }
            yield {"type": "content_block_stop", "index": 1}
            yield {
                "type": "message_delta",
                "usage": {"output_tokens": 7},
                "delta": {"stop_reason": "end_turn"},
            }
            yield {"type": "message_stop"}

        normalizer = AnthropicStreamNormalizer(
            request_id="req-1",
            model="claude-sonnet-4-6",
        )
        events = [event async for event in normalizer.normalize(raw_events())]

        self.assertEqual(events[0]["type"], "text_delta")
        self.assertEqual(events[0]["delta"], "Hello")
        self.assertEqual(events[1]["type"], "tool_use")
        self.assertEqual(events[1]["tool_call_id"], "tool-1")
        self.assertEqual(events[1]["input"], {"command": "ls"})
        self.assertEqual(events[-1]["type"], "message_done")
        self.assertEqual(events[-1]["usage"]["input_tokens"], 11)
        self.assertEqual(events[-1]["usage"]["output_tokens"], 7)
        self.assertEqual(events[-1]["stop_reason"], "end_turn")

    def test_parse_sse_events(self) -> None:
        chunks = [
            "event: message\n",
            'data: {"type":"message_start"}\n',
            "\n",
            "data: [DONE]\n",
            "\n",
        ]
        events = list(parse_sse_events(chunks))

        self.assertEqual(len(events), 2)
        self.assertEqual(events[0].event, "message")
        self.assertIn("message_start", events[0].data)
        self.assertEqual(events[1].data, "[DONE]")
