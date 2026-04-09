from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

from agent_brain.plugins import PluginLoader


class PluginLoaderTests(unittest.TestCase):
    def test_loads_builtin_and_user_plugins(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            plugins_dir = Path(temp_dir) / "plugins"
            plugin_dir = plugins_dir / "review-helper"
            plugin_dir.mkdir(parents=True)
            (plugin_dir / "plugin.json").write_text(
                json.dumps(
                    {
                        "name": "review-helper",
                        "description": "Adds review-focused hook notes.",
                        "default_enabled": True,
                        "hooks": {
                            "on_message": "Review helper saw: {{ message_text }}",
                            "on_compact": "Compacted {{ message_count }} messages.",
                        },
                    }
                ),
                encoding="utf-8",
            )

            loader = PluginLoader(user_plugins_dir=plugins_dir)
            plugins = loader.load_all()
            names = [plugin.manifest.name for plugin in plugins]

            self.assertIn("core-assistant", names)
            self.assertIn("review-helper", names)

    def test_dispatches_hooks_with_rendered_context(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            plugins_dir = Path(temp_dir) / "plugins"
            plugin_dir = plugins_dir / "hooky"
            plugin_dir.mkdir(parents=True)
            (plugin_dir / "plugin.json").write_text(
                json.dumps(
                    {
                        "name": "hooky",
                        "hooks": {
                            "on_message": "Saw {{ message_role }}: {{ message_text }}",
                            "on_tool_result": "Tool {{ tool_name }} finished {{ tool_call_id }}",
                            "on_compact": "Summary length {{ message_count }}",
                        },
                    }
                ),
                encoding="utf-8",
            )

            loader = PluginLoader(user_plugins_dir=plugins_dir)
            message_hooks = loader.dispatch_on_message(
                {"role": "user", "content": "Please review the latest diff."}
            )
            tool_hooks = loader.dispatch_on_tool_result(
                tool_name="Bash",
                tool_call_id="tool-1",
                output={"stdout": "ok"},
            )
            compact_hooks = loader.dispatch_on_compact(
                summary="Compact summary",
                messages=[{"role": "user", "content": "one"}],
            )

            self.assertIn("Saw user: Please review the latest diff.", message_hooks[0].content)
            self.assertIn("Tool Bash finished tool-1", tool_hooks[0].content)
            self.assertIn("Summary length 1", compact_hooks[0].content)
