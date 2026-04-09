from __future__ import annotations

import unittest

from agent_brain.api.client import AnthropicBackend, StreamRequest


class _FakeStream:
    def __init__(self, events):
        self._events = list(events)

    def __aiter__(self):
        self._iter = iter(self._events)
        return self

    async def __anext__(self):
        try:
            return next(self._iter)
        except StopIteration as error:
            raise StopAsyncIteration from error


class _FakeStreamContextManager:
    """Mimics the async context manager returned by client.beta.messages.stream()."""
    def __init__(self, events):
        self._stream = _FakeStream(events)

    async def __aenter__(self):
        return self._stream

    async def __aexit__(self, *args):
        pass


class _FakeMessages:
    def __init__(self, events):
        self.events = events
        self.calls = []

    def stream(self, **kwargs):
        self.calls.append(kwargs)
        return _FakeStreamContextManager(self.events)

    async def create(self, **kwargs):
        self.calls.append(kwargs)
        return _FakeStream(self.events)


class _FakeBeta:
    def __init__(self, events):
        self.messages = _FakeMessages(events)


class _FakeClient:
    def __init__(self, events):
        self.beta = _FakeBeta(events)


class ClientTests(unittest.IsolatedAsyncioTestCase):
    async def test_backend_streams_normalized_events(self) -> None:
        events = [
            {"type": "message_start", "message": {"usage": {"input_tokens": 4}}},
            {
                "type": "content_block_start",
                "index": 0,
                "content_block": {"type": "text"},
            },
            {
                "type": "content_block_delta",
                "index": 0,
                "delta": {"type": "text_delta", "text": "Hi"},
            },
            {
                "type": "message_delta",
                "usage": {"output_tokens": 2},
                "delta": {"stop_reason": "end_turn"},
            },
            {"type": "message_stop"},
        ]
        fake_client = _FakeClient(events)
        backend = AnthropicBackend(client=fake_client)
        request = StreamRequest(
            request_id="request-1",
            model="sonnet",
            messages=[{"role": "user", "content": "hello"}],
        )

        output = [item async for item in backend.stream_message(request)]

        self.assertEqual(fake_client.beta.messages.calls[0]["model"], "claude-sonnet-4-6")
        # stream=True is no longer a kwarg — we call .stream() method instead
        self.assertNotIn("stream", fake_client.beta.messages.calls[0])
        self.assertEqual(output[0]["type"], "text_delta")
        self.assertEqual(output[-1]["type"], "message_done")
