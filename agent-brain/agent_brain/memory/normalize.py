"""Multi-format transcript normalization.

Auto-detects and parses 6 chat export formats into a unified message list.
Inspired by MemPalace's normalize.py, adapted for the agent memory system.

Supported formats:
  1. Claude Code JSONL (.jsonl) — current native format
  2. Claude.ai JSON export (.json with conversations array)
  3. ChatGPT conversations.json export
  4. Slack channel export (.json with messages array)
  5. OpenAI Codex JSONL
  6. Plain text with > markers
"""

from __future__ import annotations

import json
import logging
from pathlib import Path
from typing import Any

logger = logging.getLogger(__name__)


def detect_format(path: Path) -> str:
    """Auto-detect transcript format from file content.

    Returns one of: 'claude_code_jsonl', 'claude_json', 'chatgpt_json',
    'slack_json', 'openai_codex_jsonl', 'plain_text', 'unknown'.
    """
    try:
        content = path.read_text(encoding="utf-8", errors="replace")[:4000]
    except OSError:
        return "unknown"

    name = path.name.lower()

    # JSONL formats (line-separated JSON)
    if name.endswith(".jsonl"):
        first_line = content.strip().split("\n", 1)[0].strip()
        if first_line.startswith("{"):
            try:
                obj = json.loads(first_line)
                if "role" in obj and "content" in obj:
                    # Check for OpenAI Codex pattern (has "model" field)
                    if "model" in obj:
                        return "openai_codex_jsonl"
                    return "claude_code_jsonl"
            except json.JSONDecodeError:
                pass
        return "claude_code_jsonl"  # Default JSONL guess

    # JSON formats
    if name.endswith(".json"):
        try:
            data = json.loads(content + "]" if content.rstrip().endswith(",") else content)
        except json.JSONDecodeError:
            try:
                data = json.loads(content)
            except json.JSONDecodeError:
                return "unknown"

        if isinstance(data, list):
            if data and isinstance(data[0], dict):
                # ChatGPT: list of conversation objects with "mapping"
                if "mapping" in data[0]:
                    return "chatgpt_json"
                # Slack: list of message objects with "ts" and "text"
                if "ts" in data[0] and "text" in data[0]:
                    return "slack_json"
                # Claude.ai: list with "uuid" and "chat_messages"
                if "chat_messages" in data[0] or "uuid" in data[0]:
                    return "claude_json"

        if isinstance(data, dict):
            # Single conversation
            if "mapping" in data:
                return "chatgpt_json"
            if "chat_messages" in data:
                return "claude_json"
            if "messages" in data and isinstance(data["messages"], list):
                # Check if messages look like Slack
                msgs = data["messages"]
                if msgs and isinstance(msgs[0], dict) and "ts" in msgs[0]:
                    return "slack_json"

        return "unknown"

    # Plain text
    if name.endswith((".txt", ".md")):
        if content.strip().startswith(">") or "\n> " in content:
            return "plain_text"

    return "unknown"


def normalize_transcript(path: Path) -> list[dict[str, str]]:
    """Parse any supported format into unified messages.

    Returns: [{"role": "user"|"assistant", "content": "...", "timestamp": ""}]
    """
    fmt = detect_format(path)
    try:
        if fmt == "claude_code_jsonl":
            return _parse_claude_code_jsonl(path)
        elif fmt == "claude_json":
            return _parse_claude_json(path)
        elif fmt == "chatgpt_json":
            return _parse_chatgpt_json(path)
        elif fmt == "slack_json":
            return _parse_slack_json(path)
        elif fmt == "openai_codex_jsonl":
            return _parse_openai_codex_jsonl(path)
        elif fmt == "plain_text":
            return _parse_plain_text(path)
        else:
            logger.warning("Unknown transcript format for %s", path.name)
            return []
    except Exception as e:
        logger.error("Failed to parse %s (%s): %s", path.name, fmt, e)
        return []


def normalize_directory(dir_path: Path) -> list[dict[str, str]]:
    """Normalize all transcript files in a directory."""
    all_messages: list[dict[str, str]] = []
    for ext in ("*.jsonl", "*.json", "*.txt", "*.md"):
        for p in sorted(dir_path.glob(ext)):
            messages = normalize_transcript(p)
            all_messages.extend(messages)
    return all_messages


# ── Format-specific parsers ─────────────────────────────────────────


def _parse_claude_code_jsonl(path: Path) -> list[dict[str, str]]:
    """Parse Claude Code .jsonl format: one JSON object per line with role+content."""
    messages = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            obj = json.loads(line)
        except json.JSONDecodeError:
            continue
        role = obj.get("role", "")
        if role not in ("user", "assistant"):
            continue
        content = _extract_content(obj.get("content"))
        if content:
            messages.append({
                "role": role,
                "content": content,
                "timestamp": obj.get("timestamp", ""),
            })
    return messages


def _parse_claude_json(path: Path) -> list[dict[str, str]]:
    """Parse Claude.ai JSON export: conversations with chat_messages."""
    data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    messages = []

    conversations = data if isinstance(data, list) else [data]
    for conv in conversations:
        chat_msgs = conv.get("chat_messages", [])
        for msg in chat_msgs:
            sender = msg.get("sender", "")
            role = "user" if sender == "human" else "assistant"
            content = _extract_content(msg.get("text") or msg.get("content"))
            if content:
                messages.append({
                    "role": role,
                    "content": content,
                    "timestamp": msg.get("created_at", ""),
                })
    return messages


def _parse_chatgpt_json(path: Path) -> list[dict[str, str]]:
    """Parse ChatGPT conversations.json export."""
    data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    messages = []

    conversations = data if isinstance(data, list) else [data]
    for conv in conversations:
        mapping = conv.get("mapping", {})
        # Sort by create_time to get chronological order
        nodes = sorted(
            mapping.values(),
            key=lambda n: (n.get("message", {}) or {}).get("create_time") or 0,
        )
        for node in nodes:
            msg = node.get("message")
            if not msg:
                continue
            author = msg.get("author", {}).get("role", "")
            if author not in ("user", "assistant"):
                continue
            parts = msg.get("content", {}).get("parts", [])
            text = " ".join(str(p) for p in parts if isinstance(p, str)).strip()
            if text:
                messages.append({
                    "role": author,
                    "content": text,
                    "timestamp": str(msg.get("create_time", "")),
                })
    return messages


def _parse_slack_json(path: Path) -> list[dict[str, str]]:
    """Parse Slack channel export JSON."""
    data = json.loads(path.read_text(encoding="utf-8", errors="replace"))
    messages_data = data if isinstance(data, list) else data.get("messages", [])
    messages = []

    for msg in messages_data:
        if msg.get("subtype"):
            continue  # Skip system messages
        user = msg.get("user", "unknown")
        text = msg.get("text", "").strip()
        if text:
            messages.append({
                "role": "user",  # Slack doesn't distinguish assistant
                "content": f"[{user}] {text}",
                "timestamp": msg.get("ts", ""),
            })
    return messages


def _parse_openai_codex_jsonl(path: Path) -> list[dict[str, str]]:
    """Parse OpenAI Codex .jsonl: similar to Claude Code but with model field."""
    return _parse_claude_code_jsonl(path)  # Same structure


def _parse_plain_text(path: Path) -> list[dict[str, str]]:
    """Parse plain text with > markers for user messages."""
    content = path.read_text(encoding="utf-8", errors="replace")
    messages = []
    current_role = "assistant"
    current_text: list[str] = []

    for line in content.splitlines():
        if line.startswith("> "):
            # Flush previous
            if current_text:
                messages.append({
                    "role": current_role,
                    "content": "\n".join(current_text).strip(),
                    "timestamp": "",
                })
                current_text = []
            current_role = "user"
            current_text.append(line[2:])
        elif line.startswith(">"):
            current_text.append(line[1:])
        else:
            if current_role == "user" and current_text:
                # End of user turn, start of assistant
                messages.append({
                    "role": "user",
                    "content": "\n".join(current_text).strip(),
                    "timestamp": "",
                })
                current_text = []
                current_role = "assistant"
            current_text.append(line)

    if current_text:
        text = "\n".join(current_text).strip()
        if text:
            messages.append({
                "role": current_role,
                "content": text,
                "timestamp": "",
            })

    return messages


# ── Helpers ──────────────────────────────────────────────────────────


def _extract_content(content: Any) -> str:
    """Extract readable text from various content formats."""
    if isinstance(content, str):
        return content.strip()
    if isinstance(content, list):
        parts = []
        for block in content:
            if isinstance(block, dict):
                if block.get("type") == "text":
                    parts.append(block.get("text", ""))
                elif block.get("type") == "tool_use":
                    parts.append(f"[tool: {block.get('name', '?')}]")
            elif isinstance(block, str):
                parts.append(block)
        return " ".join(parts).strip()
    return ""
