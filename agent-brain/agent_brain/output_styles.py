from __future__ import annotations

from pathlib import Path
from typing import Any

from .ipc_types import OutputStyleRequest, OutputStyleResponse
from .plugins import PluginLoader
from .types.base import AgentBaseModel

try:
    import yaml
except ImportError:  # pragma: no cover - dependency declared in pyproject
    yaml = None  # type: ignore[assignment]


class OutputStyleDefinition(AgentBaseModel):
    name: str
    description: str
    prompt: str
    source: str
    path: str = ""
    keep_coding_instructions: bool | None = None
    force_for_plugin: bool | None = None


BUILTIN_OUTPUT_STYLES = [
    OutputStyleDefinition(
        name="default",
        description="Balanced engineering style.",
        prompt="Be clear, practical, and concise. Focus on high-signal software engineering help.",
        source="builtin",
    ),
    OutputStyleDefinition(
        name="terse",
        description="Short, direct answers.",
        prompt="Prefer the shortest answer that still safely completes the task.",
        source="builtin",
    ),
    OutputStyleDefinition(
        name="explainer",
        description="Explain tradeoffs and map concepts for the user.",
        prompt="Optimize for teaching and intuition, while staying grounded in the actual code and task.",
        source="builtin",
    ),
]


class OutputStyleLoader:
    def __init__(
        self,
        *,
        user_output_styles_dir: str | Path | None = None,
        plugin_loader: PluginLoader | None = None,
    ) -> None:
        self.user_output_styles_dir = Path(
            user_output_styles_dir or (Path.home() / ".agent" / "output_styles")
        ).expanduser()
        self.plugin_loader = plugin_loader or PluginLoader()

    def load_all(self) -> dict[str, OutputStyleDefinition]:
        styles = {style.name: style for style in BUILTIN_OUTPUT_STYLES}
        styles.update(self._load_plugin_styles())
        styles.update(self._load_user_styles())
        return styles

    def get(self, style_name: str) -> OutputStyleDefinition | None:
        return self.load_all().get(style_name)

    def _load_user_styles(self) -> dict[str, OutputStyleDefinition]:
        return self._load_styles_from_directory(self.user_output_styles_dir, source="user")

    def _load_plugin_styles(self) -> dict[str, OutputStyleDefinition]:
        styles: dict[str, OutputStyleDefinition] = {}
        for plugin in self.plugin_loader.get_enabled_plugins():
            if not plugin.output_styles_path:
                continue
            plugin_path = Path(plugin.output_styles_path)
            for name, style in self._load_styles_from_directory(
                plugin_path,
                source=f"plugin:{plugin.manifest.name}",
            ).items():
                namespaced = f"{plugin.manifest.name}:{name}"
                styles[namespaced] = OutputStyleDefinition(
                    name=namespaced,
                    description=style.description,
                    prompt=style.prompt,
                    source=style.source,
                    path=style.path,
                    keep_coding_instructions=style.keep_coding_instructions,
                    force_for_plugin=style.force_for_plugin,
                )
        return styles

    def _load_styles_from_directory(
        self,
        directory: Path,
        *,
        source: str,
    ) -> dict[str, OutputStyleDefinition]:
        if not directory.exists():
            return {}

        styles: dict[str, OutputStyleDefinition] = {}
        for path in sorted({*directory.rglob("*.md"), *directory.rglob("*.markdown")}):
            definition = _load_style_file(path, source=source)
            if definition is not None:
                styles[definition.name] = definition
        return styles


class OutputStyleService:
    def __init__(self, *, loader: OutputStyleLoader | None = None) -> None:
        self.loader = loader or OutputStyleLoader()

    async def handle(self, request: OutputStyleRequest) -> OutputStyleResponse:
        style = self.loader.get(request.style_name)
        if style is None:
            return OutputStyleResponse(
                request_id=request.request_id,
                style={
                    "name": request.style_name,
                    "status": "not_found",
                    "available": sorted(self.loader.load_all().keys()),
                },
            )

        return OutputStyleResponse(
            request_id=request.request_id,
            style=style.model_dump(),
        )


def _load_style_file(path: Path, *, source: str) -> OutputStyleDefinition | None:
    raw_text = path.read_text(encoding="utf-8")
    frontmatter, body = _parse_frontmatter(raw_text)
    style_name = str(frontmatter.get("name", path.stem))
    description = str(
        frontmatter.get("description", f"Custom {style_name} output style")
    )
    keep_coding_instructions = _coerce_bool(frontmatter.get("keep-coding-instructions"))
    force_for_plugin = _coerce_bool(frontmatter.get("force-for-plugin"))
    prompt = body.strip()
    if not prompt:
        return None
    return OutputStyleDefinition(
        name=style_name,
        description=description,
        prompt=prompt,
        source=source,
        path=str(path),
        keep_coding_instructions=keep_coding_instructions,
        force_for_plugin=force_for_plugin,
    )


def _parse_frontmatter(text: str) -> tuple[dict[str, Any], str]:
    if not text.startswith("---\n"):
        return {}, text

    end_marker = "\n---\n"
    end_index = text.find(end_marker, 4)
    if end_index == -1:
        return {}, text

    frontmatter_text = text[4:end_index]
    body = text[end_index + len(end_marker) :]
    if yaml is not None:
        parsed = yaml.safe_load(frontmatter_text)
        if isinstance(parsed, dict):
            return parsed, body

    parsed: dict[str, Any] = {}
    for line in frontmatter_text.splitlines():
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        parsed[key.strip()] = value.strip()
    return parsed, body


def _coerce_bool(value: Any) -> bool | None:
    if value in (True, "true", "True"):
        return True
    if value in (False, "false", "False"):
        return False
    return None
