from __future__ import annotations

from typing import Literal

from .._compat import Field
from ..types.base import AgentBaseModel
from .catalog import APIProvider, ResolvedModel, get_default_model_key, resolve_model

SubscriberTier = Literal[
    "free",
    "payg",
    "pro",
    "max",
    "team_standard",
    "team_premium",
    "enterprise",
]


class ModelSelectionContext(AgentBaseModel):
    provider: APIProvider = "first_party"
    subscriber_tier: SubscriberTier = "payg"
    requested_model: str | None = None
    permission_mode: Literal["default", "plan"] = "default"
    long_context: bool = False
    fast_mode: bool = False


class SelectionReason(AgentBaseModel):
    code: str
    message: str


class SelectedModel(AgentBaseModel):
    resolved: ResolvedModel
    reasons: list[SelectionReason] = Field(default_factory=list)


def select_model(context: ModelSelectionContext) -> SelectedModel:
    requested = context.requested_model
    reasons: list[SelectionReason] = []

    if requested:
        adjusted = _adjust_requested_model_for_mode(requested, context.permission_mode)
        if adjusted != requested:
            reasons.append(
                SelectionReason(
                    code="plan_mode_adjustment",
                    message=f"Adjusted requested model from {requested!r} to {adjusted!r} for plan mode.",
                )
            )
        requested = adjusted
    else:
        default_key = get_default_model_key(context.subscriber_tier)
        requested = default_key
        reasons.append(
            SelectionReason(
                code="default_model",
                message=f"Selected {default_key} from subscriber tier {context.subscriber_tier}.",
            )
        )

    if context.long_context and "[1m]" not in requested:
        requested = f"{requested}[1m]"
        reasons.append(
            SelectionReason(
                code="long_context",
                message="Applied 1M-context suffix because long-context mode was requested.",
            )
        )

    resolved = resolve_model(requested, provider=context.provider)
    return SelectedModel(resolved=resolved, reasons=reasons)


def _adjust_requested_model_for_mode(
    requested_model: str, permission_mode: Literal["default", "plan"]
) -> str:
    if permission_mode != "plan":
        return requested_model

    normalized = requested_model.lower().strip()
    if normalized == "haiku":
        return "sonnet"
    if normalized == "haiku[1m]":
        return "sonnet[1m]"
    if normalized == "opusplan":
        return "opus"
    return requested_model
