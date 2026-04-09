from __future__ import annotations

import hashlib
import re
from datetime import datetime, timezone
from pathlib import Path

from .._compat import Field
from ..types.base import AgentBaseModel


class TeamSecretMatch(AgentBaseModel):
    rule_id: str
    label: str


class TeamMemoryContent(AgentBaseModel):
    entries: dict[str, str] = Field(default_factory=dict)
    entry_checksums: dict[str, str] = Field(default_factory=dict)


class TeamMemorySnapshot(AgentBaseModel):
    organization_id: str
    repo: str
    version: int
    last_modified: str
    checksum: str
    content: TeamMemoryContent


class TeamSyncResult(AgentBaseModel):
    success: bool
    files_uploaded: int = 0
    checksum: str = ""
    skipped_secrets: list[dict[str, str]] = Field(default_factory=list)
    error: str = ""


SECRET_RULES: list[tuple[str, str, str]] = [
    ("aws-access-token", r"\b(?:AKIA|ASIA|ABIA|ACCA)[A-Z0-9]{16}\b", "AWS Access Token"),
    ("anthropic-api-key", r"\bsk-ant(?:-api)?[A-Za-z0-9_\-]{20,}\b", "Anthropic API Key"),
    ("openai-api-key", r"\bsk-(?:proj|svcacct|admin)-[A-Za-z0-9_-]{20,}\b", "OpenAI API Key"),
    ("github-pat", r"\bgh[pousr]_[A-Za-z0-9]{20,}\b", "GitHub Token"),
    ("slack-bot-token", r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b", "Slack Token"),
    ("private-key", r"-----BEGIN[ A-Z0-9_-]*PRIVATE KEY(?: BLOCK)?-----", "Private Key"),
]


def scan_for_secrets(content: str) -> list[TeamSecretMatch]:
    matches: list[TeamSecretMatch] = []
    for rule_id, pattern, label in SECRET_RULES:
        if re.search(pattern, content, flags=re.I | re.S):
            matches.append(TeamSecretMatch(rule_id=rule_id, label=label))
    return matches


def ensure_no_secrets(content: str) -> None:
    matches = scan_for_secrets(content)
    if not matches:
        return
    labels = ", ".join(match.label for match in matches)
    raise ValueError(
        f"Content contains potential secrets ({labels}) and cannot be stored in team memory."
    )


class TeamMemorySyncManager:
    def __init__(self, team_dir: str | Path) -> None:
        self.team_dir = Path(team_dir).expanduser()
        self.team_dir.mkdir(parents=True, exist_ok=True)

    def export_snapshot(self, *, organization_id: str, repo: str, version: int = 1) -> TeamMemorySnapshot:
        entries: dict[str, str] = {}
        entry_checksums: dict[str, str] = {}
        for path in sorted(self.team_dir.rglob("*.md")):
            relative = path.relative_to(self.team_dir).as_posix()
            content = path.read_text(encoding="utf-8")
            entries[relative] = content
            entry_checksums[relative] = sha256_prefixed(content)
        combined_checksum = sha256_prefixed("".join(f"{key}:{entry_checksums[key]}" for key in sorted(entry_checksums)))
        return TeamMemorySnapshot(
            organization_id=organization_id,
            repo=repo,
            version=version,
            last_modified=datetime.now(timezone.utc).replace(microsecond=0).isoformat(),
            checksum=combined_checksum,
            content=TeamMemoryContent(entries=entries, entry_checksums=entry_checksums),
        )

    def import_snapshot(self, snapshot: TeamMemorySnapshot, *, merge: bool = True) -> TeamSyncResult:
        skipped: list[dict[str, str]] = []
        if not merge:
            for path in sorted(self.team_dir.rglob("*.md"), reverse=True):
                path.unlink(missing_ok=True)
        files_written = 0
        for relative_path, content in snapshot.content.entries.items():
            safe_path = self._resolve_relative_path(relative_path)
            matches = scan_for_secrets(content)
            if matches:
                skipped.append(
                    {
                        "path": relative_path,
                        "rule_id": matches[0].rule_id,
                        "label": matches[0].label,
                    }
                )
                continue
            safe_path.parent.mkdir(parents=True, exist_ok=True)
            safe_path.write_text(content, encoding="utf-8")
            files_written += 1
        return TeamSyncResult(
            success=True,
            files_uploaded=files_written,
            checksum=snapshot.checksum,
            skipped_secrets=skipped,
        )

    def _resolve_relative_path(self, relative_path: str) -> Path:
        if relative_path.startswith("/") or "\0" in relative_path:
            raise ValueError(f"Invalid team memory path: {relative_path}")
        candidate = (self.team_dir / relative_path).resolve()
        team_root = self.team_dir.resolve()
        if candidate != team_root and not str(candidate).startswith(str(team_root) + str(Path("/"))):
            raise ValueError(f"Team memory path escapes root: {relative_path}")
        return candidate


def sha256_prefixed(content: str) -> str:
    return "sha256:" + hashlib.sha256(content.encode("utf-8")).hexdigest()
