from __future__ import annotations

import unittest

from agent_brain.compact import AutoCompactPolicy, CompactService, MicroCompactor


def _make_history(message_count: int) -> list[dict[str, object]]:
    messages: list[dict[str, object]] = [
        {"role": "system", "content": "You are a coding agent working on a repo."}
    ]
    for index in range(message_count):
        role = "user" if index % 2 == 0 else "assistant"
        content = (
            f"Message {index}: "
            "Implement the next migration step and preserve behavior. " * 6
        )
        messages.append({"role": role, "content": content})
    return messages


class AutoCompactPolicyTests(unittest.TestCase):
    def test_auto_policy_detects_threshold_crossing(self) -> None:
        policy = AutoCompactPolicy()
        decision = policy.evaluate(_make_history(60), token_budget=2_000)

        self.assertTrue(decision.should_compact)
        self.assertGreater(decision.estimated_tokens, decision.threshold)


class MicroCompactTests(unittest.TestCase):
    def test_micro_compactor_summarizes_large_tool_result(self) -> None:
        compactor = MicroCompactor(max_tool_result_chars=120)
        messages = [
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "content": "traceback\n" + ("x" * 500) + "\nnext-step",
                    }
                ],
            }
        ]

        compacted, stats = compactor.compact_messages(messages)

        self.assertEqual(stats.summarized_tool_results, 1)
        compacted_text = compacted[0]["content"][0]["content"]
        self.assertIn("[micro-summary]", compacted_text)
        self.assertIn("next-step", compacted_text)


class CompactServiceTests(unittest.TestCase):
    def test_compact_service_compresses_100_message_history(self) -> None:
        service = CompactService()
        messages = _make_history(100)

        summary, compacted_messages = service.compact(messages, token_budget=2_500)

        self.assertIn("Compact Summary", summary)
        self.assertIn("Primary request:", summary)
        self.assertLess(len(compacted_messages), len(messages))
        self.assertEqual(compacted_messages[0]["role"], "system")
        self.assertEqual(compacted_messages[-1]["role"], messages[-1]["role"])
