from __future__ import annotations

from typing import Literal

from .types.base import AgentBaseModel


RateLimitSeverity = Literal["error", "warning"]

RATE_LIMIT_ERROR_PREFIXES = [
    "You've hit your",
    "You've used",
    "You're now using extra usage",
    "You're close to",
    "You're out of extra usage",
]


class RateLimitState(AgentBaseModel):
    status: Literal["allowed", "allowed_warning", "rejected"]
    rate_limit_type: str | None = None
    utilization: float | None = None
    resets_at: str | None = None
    is_using_overage: bool = False
    overage_status: str | None = None


class RateLimitMessage(AgentBaseModel):
    message: str
    severity: RateLimitSeverity


def is_rate_limit_error_message(text: str) -> bool:
    return any(text.startswith(prefix) for prefix in RATE_LIMIT_ERROR_PREFIXES)


def get_rate_limit_message(
    state: RateLimitState,
    *,
    model: str = "",
) -> RateLimitMessage | None:
    if state.is_using_overage and state.overage_status == "allowed_warning":
        return RateLimitMessage(
            message="You're close to your extra usage spending limit",
            severity="warning",
        )

    if state.status == "rejected":
        return RateLimitMessage(
            message=_limit_reached_text(state, model=model),
            severity="error",
        )

    if state.status == "allowed_warning":
        return RateLimitMessage(
            message=_warning_text(state),
            severity="warning",
        )

    return None


def _limit_reached_text(state: RateLimitState, *, model: str) -> str:
    reset_suffix = f" · resets {state.resets_at}" if state.resets_at else ""
    limit_name = _limit_name(state.rate_limit_type, model=model)
    if state.overage_status == "rejected":
        return f"You're out of extra usage{reset_suffix}"
    return f"You've hit your {limit_name}{reset_suffix}"


def _warning_text(state: RateLimitState) -> str:
    limit_name = _limit_name(state.rate_limit_type, model="")
    used_prefix = ""
    if state.utilization is not None:
        used_prefix = f"You've used {int(state.utilization * 100)}% of your {limit_name}"
    else:
        used_prefix = f"You're close to your {limit_name}"
    if state.resets_at:
        return f"{used_prefix} · resets {state.resets_at}"
    return used_prefix


def _limit_name(rate_limit_type: str | None, *, model: str) -> str:
    if rate_limit_type == "five_hour":
        return "session limit"
    if rate_limit_type == "seven_day_opus":
        return "Opus limit"
    if rate_limit_type == "seven_day_sonnet":
        return "Sonnet limit"
    if rate_limit_type == "seven_day":
        return "weekly limit"
    if rate_limit_type == "overage":
        return "extra usage"
    if "opus" in model.lower():
        return "Opus limit"
    return "usage limit"
