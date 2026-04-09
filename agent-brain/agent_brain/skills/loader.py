from __future__ import annotations

from pathlib import Path
from typing import Any

from ..types.base import AgentBaseModel

try:
    import yaml
except ImportError:  # pragma: no cover - dependency declared in pyproject
    yaml = None  # type: ignore[assignment]


class SkillDefinition(AgentBaseModel):
    name: str
    description: str
    when_to_use: str = ""
    argument_hint: str = ""
    template: str
    source: str = "bundled"
    path: str = ""


class SkillLoader:
    def __init__(
        self,
        *,
        user_skills_dir: str | Path | None = None,
        bundled_skills_dir: str | Path | None = None,
    ) -> None:
        self.user_skills_dir = Path(user_skills_dir or (Path.home() / ".agent" / "skills")).expanduser()
        self.bundled_skills_dir = Path(
            bundled_skills_dir or (Path(__file__).resolve().parent / "bundled")
        )

    def load_all(self) -> dict[str, SkillDefinition]:
        skills: dict[str, SkillDefinition] = {}
        for definition in self._load_directory(self.bundled_skills_dir, source="bundled"):
            skills[definition.name] = definition
        for definition in self._load_directory(self.user_skills_dir, source="user"):
            skills[definition.name] = definition
        return skills

    def get(self, skill_name: str) -> SkillDefinition | None:
        return self.load_all().get(skill_name)

    def _load_directory(
        self, directory: Path, *, source: str
    ) -> list[SkillDefinition]:
        if not directory.exists():
            return []
        definitions: list[SkillDefinition] = []
        for path in _iter_skill_files(directory):
            parsed = parse_skill_yaml(path)
            if parsed is None:
                continue
            definitions.append(
                SkillDefinition(
                    name=str(parsed.get("name", path.stem)),
                    description=str(parsed.get("description", "")),
                    when_to_use=str(parsed.get("when_to_use", "")),
                    argument_hint=str(parsed.get("argument_hint", "")),
                    template=str(parsed.get("template", "")),
                    source=source,
                    path=str(path),
                )
            )
        return definitions


def _iter_skill_files(directory: Path) -> list[Path]:
    paths = {
        *directory.rglob("*.yaml"),
        *directory.rglob("*.yml"),
    }
    return sorted(paths)


def parse_skill_yaml(path: str | Path) -> dict[str, Any] | None:
    if yaml is not None:
        parsed = yaml.safe_load(Path(path).read_text(encoding="utf-8"))
        if isinstance(parsed, dict) and "template" in parsed and "name" in parsed:
            return parsed

    lines = Path(path).read_text(encoding="utf-8").splitlines()
    result: dict[str, Any] = {}
    index = 0
    while index < len(lines):
        raw_line = lines[index]
        line = raw_line.rstrip()
        index += 1
        if not line or line.lstrip().startswith("#"):
            continue
        if ":" not in line:
            continue
        key, value = line.split(":", 1)
        key = key.strip()
        value = value.lstrip()
        if value == "|":
            block_lines: list[str] = []
            while index < len(lines):
                block_line = lines[index]
                if block_line.startswith("  "):
                    block_lines.append(block_line[2:])
                    index += 1
                    continue
                if not block_line.strip():
                    block_lines.append("")
                    index += 1
                    continue
                break
            result[key] = "\n".join(block_lines).rstrip()
            continue
        result[key] = value
    if "template" not in result or "name" not in result:
        return None
    return result
