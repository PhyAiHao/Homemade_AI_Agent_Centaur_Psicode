from __future__ import annotations

import asyncio
import tempfile
import unittest
from pathlib import Path

from agent_brain.memory import MemoryService, MemoryStore
from agent_brain.memory.extract import MemoryExtractor
from agent_brain.memory.session import SessionMemoryManager
from agent_brain.memory.team_sync import TeamMemoryContent, TeamMemorySnapshot, scan_for_secrets


class MemoryStoreTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.store = MemoryStore(self.temp_dir.name)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_save_list_recall_and_prompt(self) -> None:
        self.store.save_memory(
            title="User: Backend expert new to React",
            body="The user has deep Go experience and is new to the React side of the codebase.",
            memory_type="user",
            scope="private",
            description="Backend-heavy user; frontend explanations should map to backend analogies.",
        )
        self.store.save_memory(
            title="Project: Mobile release freeze",
            body="The repo enters a merge freeze on 2026-03-05 for the mobile release cut.",
            memory_type="project",
            scope="team",
            description="Freeze begins 2026-03-05 for mobile release.",
        )

        private_memories = self.store.list_memories("private")
        team_memories = self.store.list_memories("team")
        recall = self.store.recall("React explanations and release freeze", include_team=True)
        prompt = self.store.render_system_prompt(query="React explanations", include_team=True)

        self.assertEqual(len(private_memories), 1)
        self.assertEqual(len(team_memories), 1)
        self.assertGreaterEqual(len(recall.memories), 1)
        self.assertIn("Persistent Memory", prompt)
        self.assertIn("Backend expert", prompt)


class ExtractionTests(unittest.TestCase):
    def test_extractor_detects_feedback_and_reference(self) -> None:
        extractor = MemoryExtractor()
        messages = [
            {"role": "user", "content": "Don't summarize the diff at the end of every reply."},
            {"role": "user", "content": "Check the Grafana dashboard at https://grafana.internal/d/api-latency when touching request code."},
        ]
        result = extractor.extract(messages, include_team=True)

        types = [candidate.memory_type for candidate in result.candidates]
        self.assertIn("feedback", types)
        self.assertIn("reference", types)


class SessionMemoryTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.manager = SessionMemoryManager(root_dir=self.temp_dir.name, session_id="abc123")

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_session_update_writes_template_sections(self) -> None:
        messages = [
            {"role": "user", "content": "Implement a memory system for the agent."},
            {"role": "assistant", "content": "Added the first memory modules and tests."},
        ]
        content = self.manager.update(messages, current_token_count=12_000, last_message_id="m2")

        self.assertIn("# Session Title", content)
        self.assertIn("Implement a memory system", content)
        self.assertIn("Added the first memory modules", content)
        self.assertFalse(self.manager.is_empty())


class TeamSyncTests(unittest.TestCase):
    def setUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.store = MemoryStore(self.temp_dir.name)

    def tearDown(self) -> None:
        self.temp_dir.cleanup()

    def test_secret_scan_and_snapshot_import(self) -> None:
        matches = scan_for_secrets("token ghp_123456789012345678901234567890123456")
        self.assertTrue(matches)

        snapshot = TeamMemorySnapshot(
            organization_id="org-1",
            repo="repo-1",
            version=1,
            last_modified="2026-04-01T00:00:00+00:00",
            checksum="sha256:test",
            content=TeamMemoryContent(
                entries={
                    "safe.md": "---\nname: Safe\ndescription: ok\ntype: project\nscope: team\ncreated_at: 2026-04-01T00:00:00+00:00\nupdated_at: 2026-04-01T00:00:00+00:00\nslug: safe\n---\nSafe body\n",
                    "secret.md": "ghp_123456789012345678901234567890123456",
                },
                entry_checksums={},
            ),
        )
        result = self.store.render_system_prompt(query="", include_team=True)
        self.assertIn("Team memory index", result)

        from agent_brain.memory.team_sync import TeamMemorySyncManager

        manager = TeamMemorySyncManager(self.store.team_dir)
        sync_result = manager.import_snapshot(snapshot, merge=True)
        self.assertTrue(sync_result.success)
        self.assertEqual(len(sync_result.skipped_secrets), 1)
        self.assertTrue((self.store.team_dir / "safe.md").exists())


class MemoryServiceTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self) -> None:
        self.temp_dir = tempfile.TemporaryDirectory()
        self.service = MemoryService(self.temp_dir.name)

    async def asyncTearDown(self) -> None:
        self.temp_dir.cleanup()

    async def test_memory_service_save_and_list(self) -> None:
        save_response = await self.service.handle(
            type("Request", (), {
                "request_id": "mem-1",
                "action": "save",
                "payload": {
                    "title": "Feedback: Keep responses terse",
                    "body": "The user prefers terse close-out messages.",
                    "memory_type": "feedback",
                    "scope": "private",
                    "description": "User prefers terse close-out messages.",
                },
            })()
        )
        list_response = await self.service.handle(
            type("Request", (), {
                "request_id": "mem-2",
                "action": "list",
                "payload": {"include_team": False},
            })()
        )

        self.assertTrue(save_response.ok)
        self.assertEqual(len(list_response.payload["items"]), 1)
