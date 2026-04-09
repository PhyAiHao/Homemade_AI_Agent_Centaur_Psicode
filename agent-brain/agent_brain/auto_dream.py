from __future__ import annotations

from dataclasses import dataclass
from datetime import datetime, timedelta, timezone


@dataclass
class AutoDreamDecision:
    should_run: bool
    sessions_since_last: int
    hours_since_last: float
    reason: str


class AutoDreamService:
    def __init__(self, *, min_hours: int = 12, min_sessions: int = 5) -> None:
        self.min_hours = min_hours
        self.min_sessions = min_sessions

    def evaluate(
        self,
        *,
        last_consolidated_at: datetime,
        session_timestamps: list[datetime],
        now: datetime | None = None,
    ) -> AutoDreamDecision:
        current_time = now or datetime.now(timezone.utc)
        hours_since_last = max(
            0.0, (current_time - last_consolidated_at).total_seconds() / 3600.0
        )
        sessions_since_last = sum(
            1 for timestamp in session_timestamps if timestamp > last_consolidated_at
        )

        if hours_since_last < self.min_hours:
            return AutoDreamDecision(
                should_run=False,
                sessions_since_last=sessions_since_last,
                hours_since_last=hours_since_last,
                reason="time_gate",
            )

        if sessions_since_last < self.min_sessions:
            return AutoDreamDecision(
                should_run=False,
                sessions_since_last=sessions_since_last,
                hours_since_last=hours_since_last,
                reason="session_gate",
            )

        return AutoDreamDecision(
            should_run=True,
            sessions_since_last=sessions_since_last,
            hours_since_last=hours_since_last,
            reason="ready",
        )

    def build_consolidation_prompt(
        self,
        *,
        memory_root: str,
        session_summaries: list[str],
    ) -> str:
        if not session_summaries:
            session_summaries = ["No recent sessions were provided."]
        return "\n".join(
            [
                "# AutoDream Consolidation",
                f"Memory root: {memory_root}",
                "",
                "Recent sessions to consolidate:",
                *[f"- {summary}" for summary in session_summaries],
                "",
                "Synthesize durable learnings, update persistent memory, and skip transient task noise.",
            ]
        ).strip()


def utc_now_minus(hours: int) -> datetime:
    return datetime.now(timezone.utc) - timedelta(hours=hours)
