from __future__ import annotations

import asyncio
import json
import tempfile
import unittest
from pathlib import Path

from agent_brain.output_styles import OutputStyleLoader, OutputStyleService
from agent_brain.plugins import PluginLoader


class OutputStyleTests(unittest.TestCase):
    def test_loads_builtin_user_and_plugin_output_styles(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            user_styles = root / "output_styles"
            user_styles.mkdir()
            (user_styles / "strict.md").write_text(
                "\n".join(
                    [
                        "---",
                        "name: strict",
                        "description: Strict style",
                        "---",
                        "Be strict and concise.",
                    ]
                ),
                encoding="utf-8",
            )

            plugins_dir = root / "plugins"
            plugin_dir = plugins_dir / "diagnostics"
            styles_dir = plugin_dir / "output-styles"
            styles_dir.mkdir(parents=True)
            (plugin_dir / "plugin.json").write_text(
                json.dumps(
                    {
                        "name": "diagnostics",
                        "description": "Diagnostic plugin",
                        "default_enabled": True,
                    }
                ),
                encoding="utf-8",
            )
            (styles_dir / "investigator.md").write_text(
                "Explain issues methodically.",
                encoding="utf-8",
            )

            loader = OutputStyleLoader(
                user_output_styles_dir=user_styles,
                plugin_loader=PluginLoader(user_plugins_dir=plugins_dir),
            )
            styles = loader.load_all()

            self.assertIn("default", styles)
            self.assertIn("strict", styles)
            self.assertIn("diagnostics:investigator", styles)

    def test_output_style_service_returns_style_payload(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            user_styles = Path(temp_dir)
            (user_styles / "focus.md").write_text(
                "Stay focused on the highest-signal details.",
                encoding="utf-8",
            )
            service = OutputStyleService(
                loader=OutputStyleLoader(user_output_styles_dir=user_styles)
            )

            class Request:
                request_id = "style-1"
                style_name = "focus"

            response = asyncio.run(service.handle(Request()))
            self.assertEqual(response.style["name"], "focus")
            self.assertIn("highest-signal", response.style["prompt"])
