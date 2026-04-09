from __future__ import annotations

from dataclasses import dataclass

from .snip import estimate_messages_tokens


DEFAULT_THRESHOLD_RATIO = 0.85
DEFAULT_OUTPUT_RESERVE_TOKENS = 1_024
DEFAULT_MIN_EFFECTIVE_BUDGET = 2_048


@dataclass
class AutoCompactDecision:
    estimated_tokens: int
    effective_budget: int
    threshold: int
    should_compact: bool


class AutoCompactPolicy:
    def __init__(
        self,
        *,
        threshold_ratio: float = DEFAULT_THRESHOLD_RATIO,
        output_reserve_tokens: int = DEFAULT_OUTPUT_RESERVE_TOKENS,
        min_effective_budget: int = DEFAULT_MIN_EFFECTIVE_BUDGET,
    ) -> None:
        self.threshold_ratio = threshold_ratio
        self.output_reserve_tokens = output_reserve_tokens
        self.min_effective_budget = min_effective_budget

    def evaluate(
        self,
        messages: list[dict[str, object]],
        token_budget: int | None,
    ) -> AutoCompactDecision:
        estimated_tokens = estimate_messages_tokens(messages)
        requested_budget = token_budget or 24_000
        effective_budget = max(
            self.min_effective_budget,
            requested_budget - self.output_reserve_tokens,
        )
        threshold = max(1, int(effective_budget * self.threshold_ratio))
        return AutoCompactDecision(
            estimated_tokens=estimated_tokens,
            effective_budget=effective_budget,
            threshold=threshold,
            should_compact=estimated_tokens >= threshold,
        )
