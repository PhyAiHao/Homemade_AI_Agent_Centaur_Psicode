from __future__ import annotations

import asyncio
import tempfile
import unittest
from pathlib import Path

from agent_brain.voice import (
    VoiceModeClient,
    VoiceService,
    get_voice_keyterms,
    split_identifier,
)


class VoiceKeytermTests(unittest.TestCase):
    def test_split_identifier_and_keyterm_collection(self) -> None:
        parts = split_identifier("feat/voiceSystem_auth-flow")
        keyterms = get_voice_keyterms(
            project_dir="/tmp/claude-code-main",
            branch_name="feat/voiceSystem_auth-flow",
            recent_files=["src/services/voiceStreamSTT.ts", "docs/AuthGuide.md"],
            extra_terms=["Anthropic"],
        )

        self.assertIn("voice", [part.lower() for part in parts])
        self.assertIn("auth", [term.lower() for term in keyterms])
        self.assertIn("Anthropic", keyterms)


class VoiceClientTests(unittest.TestCase):
    def test_capture_text_audio_and_transcribe(self) -> None:
        client = VoiceModeClient()
        captured = client.capture_text_audio("review the latest auth diff")
        result = client.transcribe_audio_bytes(captured.audio_bytes, language="en")

        self.assertEqual(result.text, "review the latest auth diff")
        self.assertEqual(result.source, "embedded_hint")

    def test_streaming_session_combines_audio_chunks(self) -> None:
        client = VoiceModeClient()
        captured = client.capture_text_audio("summarize the failing test output")
        session = client.start_session(language="en", keyterms=["test", "output"])
        midpoint = len(captured.audio_bytes) // 2
        session.send_audio(captured.audio_bytes[:midpoint])
        session.send_audio(captured.audio_bytes[midpoint:])

        result = session.finalize()

        self.assertEqual(result.text, "summarize the failing test output")
        self.assertIn("test", result.keyterms)

    def test_transcribe_file_round_trip(self) -> None:
        client = VoiceModeClient()
        captured = client.capture_text_audio("check the websocket transcript flow")
        with tempfile.TemporaryDirectory() as temp_dir:
            wav_path = Path(temp_dir) / "voice.wav"
            wav_path.write_bytes(captured.audio_bytes)

            result = client.transcribe_file(wav_path)

        self.assertEqual(result.text, "check the websocket transcript flow")


class VoiceServiceTests(unittest.TestCase):
    def test_voice_service_handles_audio_b64_request(self) -> None:
        client = VoiceModeClient()
        captured = client.capture_text_audio("verify the compact summary")
        service = VoiceService(client=client)

        class Request:
            request_id = "voice-1"
            language = "en"
            audio_b64 = captured.audio_b64
            audio_path = None
            keyterms = []
            recent_files = ["src/services/compact/compact.ts"]
            project_dir = "/tmp/project"
            branch_name = "feat/voice-mode"
            transcript_hint = None

        response = asyncio.run(service.handle(Request()))
        self.assertEqual(response.text, "verify the compact summary")
        self.assertEqual(response.metadata["source"], "embedded_hint")
