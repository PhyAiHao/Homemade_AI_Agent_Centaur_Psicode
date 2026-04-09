from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from agent_brain.attachments import (
    build_file_attachment,
    build_memory_attachment,
    build_note_attachment,
    render_attachment_bundle,
    render_memory_bundle,
)


class AttachmentTests(unittest.TestCase):
    def test_build_file_attachment_truncates_large_files(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            path = Path(temp_dir) / "diff.txt"
            path.write_text("A" * 300, encoding="utf-8")

            attachment = build_file_attachment(path, max_chars=120)

            self.assertEqual(attachment.kind, "file")
            self.assertTrue(attachment.metadata["truncated"])
            self.assertIn("[truncated attachment content]", attachment.content)

    def test_render_bundle_includes_file_and_note(self) -> None:
        note = build_note_attachment("Summary", "Important repo note")
        memory = build_memory_attachment(
            "User preference",
            "Prefer concise close-outs.",
            memory_type="feedback",
            scope="private",
            description="Affects response style.",
        )

        rendered = render_attachment_bundle([note, memory])
        memory_rendered = render_memory_bundle([memory])

        self.assertIn("Note Attachment: Summary", rendered)
        self.assertIn("Memory Attachment: User preference", rendered)
        self.assertIn("feedback/private", memory_rendered)
