from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

from ..types.base import AgentBaseModel

try:
    from jinja2 import Template
except ImportError:  # pragma: no cover - dependency declared in pyproject
    Template = None  # type: ignore[assignment]


class HookTemplates(AgentBaseModel):
    on_message: str | None = None
    on_tool_result: str | None = None
    on_compact: str | None = None


class PluginManifest(AgentBaseModel):
    name: str
    description: str = ""
    version: str = "0.1.0"
    default_enabled: bool = True
    hooks: HookTemplates = HookTemplates()
    output_styles_dir: str = "output-styles"
    metadata: dict[str, Any] = {}


class LoadedPlugin(AgentBaseModel):
    manifest: PluginManifest
    source: str
    path: str
    enabled: bool
    output_styles_path: str | None = None

    @property
    def name(self) -> str:
        return self.manifest.name


class PluginHookResult(AgentBaseModel):
    plugin_name: str
    hook_name: str
    content: str


class PluginLoader:
    def __init__(
        self,
        *,
        user_plugins_dir: str | Path | None = None,
        builtin_manifest_path: str | Path | None = None,
    ) -> None:
        self.user_plugins_dir = Path(
            user_plugins_dir or (Path.home() / ".agent" / "plugins")
        ).expanduser()
        self.builtin_manifest_path = Path(
            builtin_manifest_path
            or (Path(__file__).resolve().parent / "builtin_manifest.json")
        )

    def load_all(self) -> list[LoadedPlugin]:
        plugins = self._load_builtin_plugins()
        plugins.extend(self._load_user_plugins())
        return sorted(plugins, key=lambda plugin: (plugin.source, plugin.manifest.name))

    def get_enabled_plugins(self) -> list[LoadedPlugin]:
        return [plugin for plugin in self.load_all() if plugin.enabled]

    def dispatch_on_message(
        self,
        message: dict[str, Any],
        *,
        context: dict[str, Any] | None = None,
    ) -> list[PluginHookResult]:
        message_text = _extract_text_from_message(message)
        return self._dispatch(
            hook_name="on_message",
            context={
                "message_role": str(message.get("role", "unknown")),
                "message_text": message_text,
                "message": message,
                **dict(context or {}),
            },
        )

    def dispatch_on_tool_result(
        self,
        *,
        tool_name: str,
        tool_call_id: str,
        output: Any,
        context: dict[str, Any] | None = None,
    ) -> list[PluginHookResult]:
        return self._dispatch(
            hook_name="on_tool_result",
            context={
                "tool_name": tool_name,
                "tool_call_id": tool_call_id,
                "output": output,
                **dict(context or {}),
            },
        )

    def dispatch_on_compact(
        self,
        *,
        summary: str,
        messages: list[dict[str, Any]],
        context: dict[str, Any] | None = None,
    ) -> list[PluginHookResult]:
        return self._dispatch(
            hook_name="on_compact",
            context={
                "summary": summary,
                "messages": messages,
                "message_count": len(messages),
                **dict(context or {}),
            },
        )

    def _dispatch(
        self,
        *,
        hook_name: str,
        context: dict[str, Any],
    ) -> list[PluginHookResult]:
        results: list[PluginHookResult] = []
        for plugin in self.get_enabled_plugins():
            template = getattr(plugin.manifest.hooks, hook_name)
            if not template:
                continue
            rendered = _render_template(
                template,
                {"plugin_name": plugin.manifest.name, **context},
            ).strip()
            if not rendered:
                continue
            results.append(
                PluginHookResult(
                    plugin_name=plugin.manifest.name,
                    hook_name=hook_name,
                    content=rendered,
                )
            )
        return results

    def _load_builtin_plugins(self) -> list[LoadedPlugin]:
        if not self.builtin_manifest_path.exists():
            return []

        payload = json.loads(self.builtin_manifest_path.read_text(encoding="utf-8"))
        entries = payload.get("plugins", [])
        plugins: list[LoadedPlugin] = []
        for entry in entries:
            manifest = _build_manifest(entry)
            plugins.append(
                LoadedPlugin(
                    manifest=manifest,
                    source="builtin",
                    path=str(self.builtin_manifest_path),
                    enabled=manifest.default_enabled,
                    output_styles_path=None,
                )
            )
        return plugins

    def _load_user_plugins(self) -> list[LoadedPlugin]:
        if not self.user_plugins_dir.exists():
            return []

        plugins: list[LoadedPlugin] = []
        for path in sorted(self.user_plugins_dir.iterdir()):
            if not path.is_dir():
                continue
            manifest_path = path / "plugin.json"
            if not manifest_path.exists():
                continue
            manifest = _build_manifest(
                json.loads(manifest_path.read_text(encoding="utf-8"))
            )
            output_styles_dir = path / manifest.output_styles_dir
            plugins.append(
                LoadedPlugin(
                    manifest=manifest,
                    source="user",
                    path=str(path),
                    enabled=manifest.default_enabled,
                    output_styles_path=str(output_styles_dir)
                    if output_styles_dir.exists()
                    else None,
                )
            )
        return plugins


def _extract_text_from_message(message: dict[str, Any]) -> str:
    content = message.get("content", "")
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        parts: list[str] = []
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                parts.append(str(item.get("text", "")))
            else:
                parts.append(str(item))
        return "\n".join(part for part in parts if part).strip()
    return str(content)


def _build_manifest(payload: dict[str, Any]) -> PluginManifest:
    manifest_payload = dict(payload)
    hooks = manifest_payload.get("hooks")
    manifest_payload["hooks"] = HookTemplates(**hooks) if isinstance(hooks, dict) else HookTemplates()
    metadata = manifest_payload.get("metadata")
    manifest_payload["metadata"] = dict(metadata) if isinstance(metadata, dict) else {}
    return PluginManifest(**manifest_payload)


def _render_template(template: str, context: dict[str, Any]) -> str:
    if Template is not None:
        return Template(template).render(**context)

    def replace(match: re.Match[str]) -> str:
        key = match.group(1).strip()
        return str(context.get(key, ""))

    return re.sub(r"\{\{\s*([a-zA-Z0-9_]+)\s*\}\}", replace, template)
