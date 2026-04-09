from __future__ import annotations

from dataclasses import dataclass
from typing import Any


TOOL_RESULT_OMISSION_TEMPLATE = "[micro-summary] tool result truncated ({removed_chars} chars omitted)"


@dataclass
class MicroCompactStats:
    modified_messages: int = 0
    summarized_tool_results: int = 0
    chars_saved: int = 0


class MicroCompactor:
    def __init__(self, *, max_tool_result_chars: int = 400) -> None:
        self.max_tool_result_chars = max_tool_result_chars

    def compact_messages(
        self,
        messages: list[dict[str, Any]],
    ) -> tuple[list[dict[str, Any]], MicroCompactStats]:
        stats = MicroCompactStats()
        compacted_messages: list[dict[str, Any]] = []
        for message in messages:
            compacted_message, changed, summarized, chars_saved = self._compact_message(
                message
            )
            if changed:
                stats.modified_messages += 1
            stats.summarized_tool_results += summarized
            stats.chars_saved += chars_saved
            compacted_messages.append(compacted_message)
        return compacted_messages, stats

    def _compact_message(
        self,
        message: dict[str, Any],
    ) -> tuple[dict[str, Any], bool, int, int]:
        content = message.get("content")
        if not isinstance(content, list):
            return dict(message), False, 0, 0

        changed = False
        summarized = 0
        chars_saved = 0
        compacted_blocks: list[Any] = []
        for block in content:
            if not isinstance(block, dict):
                compacted_blocks.append(block)
                continue

            if block.get("type") != "tool_result":
                compacted_blocks.append(dict(block))
                continue

            compacted_block, block_changed, block_chars_saved = self._compact_tool_result(
                block
            )
            compacted_blocks.append(compacted_block)
            if block_changed:
                changed = True
                summarized += 1
                chars_saved += block_chars_saved

        updated = dict(message)
        updated["content"] = compacted_blocks
        return updated, changed, summarized, chars_saved

    def _compact_tool_result(
        self,
        block: dict[str, Any],
    ) -> tuple[dict[str, Any], bool, int]:
        content = block.get("content")
        if isinstance(content, str):
            compacted, changed, chars_saved = self._compact_text(content)
            updated = dict(block)
            updated["content"] = compacted
            return updated, changed, chars_saved

        if isinstance(content, list):
            updated_items: list[Any] = []
            changed = False
            chars_saved = 0
            for item in content:
                if not isinstance(item, dict) or item.get("type") != "text":
                    updated_items.append(item)
                    continue
                compacted, item_changed, item_chars_saved = self._compact_text(
                    str(item.get("text", ""))
                )
                updated_item = dict(item)
                updated_item["text"] = compacted
                updated_items.append(updated_item)
                if item_changed:
                    changed = True
                    chars_saved += item_chars_saved
            updated = dict(block)
            updated["content"] = updated_items
            return updated, changed, chars_saved

        return dict(block), False, 0

    def _compact_text(self, text: str) -> tuple[str, bool, int]:
        if len(text) <= self.max_tool_result_chars:
            return text, False, 0

        visible_window = max(40, self.max_tool_result_chars // 4)
        prefix = text[:visible_window].rstrip()
        suffix = text[-visible_window:].lstrip()
        removed_chars = max(0, len(text) - (len(prefix) + len(suffix)))
        compacted = "\n".join(
            [
                prefix,
                TOOL_RESULT_OMISSION_TEMPLATE.format(removed_chars=removed_chars),
                suffix,
            ]
        ).strip()
        chars_saved = max(0, len(text) - len(compacted))
        return compacted, True, chars_saved
