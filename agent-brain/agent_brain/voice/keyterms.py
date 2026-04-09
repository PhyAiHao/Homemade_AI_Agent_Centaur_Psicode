from __future__ import annotations

from pathlib import Path


GLOBAL_KEYTERMS: tuple[str, ...] = (
    "MCP",
    "symlink",
    "grep",
    "regex",
    "localhost",
    "codebase",
    "TypeScript",
    "JSON",
    "OAuth",
    "webhook",
    "gRPC",
    "dotfiles",
    "subagent",
    "worktree",
)

MAX_KEYTERMS = 50


def split_identifier(name: str) -> list[str]:
    fragments = (
        name.replace("/", " ")
        .replace("\\", " ")
        .replace(".", " ")
        .replace("-", " ")
        .replace("_", " ")
        .replace(":", " ")
        .split()
    )
    return [
        piece
        for fragment in fragments
        for piece in _split_camel_case(fragment)
        if 2 < len(piece) <= 32
    ]


def get_voice_keyterms(
    *,
    project_dir: str | None = None,
    branch_name: str | None = None,
    recent_files: list[str] | None = None,
    extra_terms: list[str] | None = None,
    max_keyterms: int = MAX_KEYTERMS,
) -> list[str]:
    ordered_terms: list[str] = []
    seen: set[str] = set()

    def add(term: str) -> None:
        value = term.strip()
        if not value:
            return
        key = value.lower()
        if key in seen:
            return
        seen.add(key)
        ordered_terms.append(value)

    for term in GLOBAL_KEYTERMS:
        add(term)

    if project_dir:
        add(Path(project_dir).expanduser().resolve().name)

    if branch_name:
        for term in split_identifier(branch_name):
            add(term)

    for file_path in recent_files or []:
        stem = Path(file_path).name.rsplit(".", 1)[0]
        for term in split_identifier(stem):
            add(term)

    for term in extra_terms or []:
        add(term)

    return ordered_terms[:max_keyterms]


def _split_camel_case(fragment: str) -> list[str]:
    if not fragment:
        return []
    items: list[str] = []
    current = fragment[0]
    for char in fragment[1:]:
        if char.isupper() and current[-1].islower():
            items.append(current)
            current = char
        else:
            current += char
    items.append(current)
    return [item for item in items if item]
