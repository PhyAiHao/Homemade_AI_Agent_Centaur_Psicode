from __future__ import annotations

from datetime import datetime, timedelta, timezone
import tempfile
import unittest

from agent_brain.auto_dream import AutoDreamService
from agent_brain.magic_docs import MagicDocsService
from agent_brain.rate_limits import RateLimitState, get_rate_limit_message, is_rate_limit_error_message
from agent_brain.tips import TipsService
from agent_brain.vcr import VCRRecorder


class MagicDocsTests(unittest.TestCase):
    def test_detects_magic_doc_and_builds_prompt(self) -> None:
        service = MagicDocsService()
        content = "# MAGIC DOC: Auth Flow\n\n_Update when auth behavior changes._\n\nBody"

        detected = service.detect(content)
        prompt = service.build_update_prompt(
            path="docs/auth.md",
            content=content,
            latest_summary="Auth middleware changed in the latest patch.",
        )

        self.assertIsNotNone(detected)
        self.assertEqual(detected.title, "Auth Flow")
        self.assertIn("Auth middleware changed", prompt)


class AutoDreamTests(unittest.TestCase):
    def test_evaluates_time_and_session_gates(self) -> None:
        service = AutoDreamService(min_hours=24, min_sessions=2)
        now = datetime.now(timezone.utc)
        last = now - timedelta(hours=30)
        decision = service.evaluate(
            last_consolidated_at=last,
            session_timestamps=[now - timedelta(hours=4), now - timedelta(hours=2)],
            now=now,
        )

        self.assertTrue(decision.should_run)
        self.assertEqual(decision.reason, "ready")

    def test_builds_consolidation_prompt(self) -> None:
        service = AutoDreamService()
        prompt = service.build_consolidation_prompt(
            memory_root="/tmp/memory",
            session_summaries=["Session A touched auth", "Session B touched billing"],
        )

        self.assertIn("AutoDream Consolidation", prompt)
        self.assertIn("Session A touched auth", prompt)


class TipsTests(unittest.TestCase):
    def test_suggests_relevant_tips(self) -> None:
        service = TipsService()
        tips = service.suggest({"uses_memory": False, "has_claude_md": False})

        tip_ids = [tip.tip_id for tip in tips]
        self.assertIn("memory", tip_ids)
        self.assertIn("init", tip_ids)


class VCRTests(unittest.TestCase):
    def test_records_and_replays_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            recorder = VCRRecorder(root_dir=temp_dir)
            payload = recorder.with_fixture("auth-review", lambda: {"status": "ok"})

            self.assertEqual(payload["status"], "ok")
            replayed = recorder.replay("auth-review")
            self.assertEqual(replayed["status"], "ok")


class AsyncVCRTests(unittest.IsolatedAsyncioTestCase):
    async def test_records_and_replays_stream_fixture(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            recorder = VCRRecorder(root_dir=temp_dir)

            async def stream_fixture():
                yield {"event": "start"}
                yield {"event": "done"}

            first = await recorder.with_stream_fixture("stream-review", stream_fixture)
            second = await recorder.with_stream_fixture("stream-review", stream_fixture)

            self.assertEqual(first[0]["event"], "start")
            self.assertEqual(second[-1]["event"], "done")


class RateLimitTests(unittest.TestCase):
    def test_formats_warning_and_detects_prefix(self) -> None:
        message = get_rate_limit_message(
            RateLimitState(
                status="allowed_warning",
                rate_limit_type="seven_day",
                utilization=0.82,
                resets_at="tomorrow",
            )
        )

        self.assertIsNotNone(message)
        self.assertEqual(message.severity, "warning")
        self.assertTrue(is_rate_limit_error_message("You've used 82% of your weekly limit"))
