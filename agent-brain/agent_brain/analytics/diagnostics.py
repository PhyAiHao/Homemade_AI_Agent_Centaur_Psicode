from __future__ import annotations

from copy import deepcopy
from datetime import datetime, timezone
from typing import Any

from .._compat import Field
from ..models import calculate_cost_usd
from ..types.base import AgentBaseModel


class ModelUsageTotals(AgentBaseModel):
    request_count: int = 0
    input_tokens: int = 0
    output_tokens: int = 0
    cache_read_input_tokens: int = 0
    cache_creation_input_tokens: int = 0
    web_search_requests: int = 0
    web_fetch_requests: int = 0
    total_cost_usd: float = 0.0
    total_duration_ms: float = 0.0


class UsageTotals(AgentBaseModel):
    request_count: int = 0
    successful_requests: int = 0
    failed_requests: int = 0
    total_input_tokens: int = 0
    total_output_tokens: int = 0
    total_cache_read_input_tokens: int = 0
    total_cache_creation_input_tokens: int = 0
    total_web_search_requests: int = 0
    total_web_fetch_requests: int = 0
    total_cost_usd: float = 0.0
    total_duration_ms: float = 0.0
    average_duration_ms: float = 0.0
    last_request_id: str | None = None
    last_model: str | None = None
    last_stop_reason: str | None = None
    per_model: dict[str, ModelUsageTotals] = Field(default_factory=dict)


class DiagnosticIncident(AgentBaseModel):
    timestamp: str
    kind: str
    message: str
    request_id: str | None = None
    model: str | None = None


class AnalyticsDiagnostics(AgentBaseModel):
    event_counts: dict[str, int] = Field(default_factory=dict)
    sink_failures: dict[str, int] = Field(default_factory=dict)
    recent_errors: list[DiagnosticIncident] = Field(default_factory=list)
    last_success_at: str | None = None


class AnalyticsUsageReport(AgentBaseModel):
    usage: UsageTotals
    diagnostics: AnalyticsDiagnostics


class DiagnosticsTracker:
    def __init__(self, *, max_recent_errors: int = 20) -> None:
        self.max_recent_errors = max_recent_errors
        self._usage = UsageTotals()
        self._diagnostics = AnalyticsDiagnostics()

    def record_event(self, event_name: str) -> None:
        self._diagnostics.event_counts[event_name] = (
            self._diagnostics.event_counts.get(event_name, 0) + 1
        )

    def record_api_success(
        self,
        *,
        request_id: str,
        model: str,
        usage: dict[str, Any],
        provider: str = "first_party",
        fast_mode: bool = False,
        duration_ms: float = 0.0,
        stop_reason: str | None = None,
    ) -> None:
        normalized = self._normalized_usage(model, usage, provider=provider, fast_mode=fast_mode)
        self._usage.request_count += 1
        self._usage.successful_requests += 1
        self._usage.total_input_tokens += normalized["input_tokens"]
        self._usage.total_output_tokens += normalized["output_tokens"]
        self._usage.total_cache_read_input_tokens += normalized["cache_read_input_tokens"]
        self._usage.total_cache_creation_input_tokens += normalized[
            "cache_creation_input_tokens"
        ]
        self._usage.total_web_search_requests += normalized["web_search_requests"]
        self._usage.total_web_fetch_requests += normalized["web_fetch_requests"]
        self._usage.total_cost_usd += normalized["cost_usd"]
        self._usage.total_duration_ms += duration_ms
        self._usage.average_duration_ms = (
            self._usage.total_duration_ms / self._usage.successful_requests
            if self._usage.successful_requests
            else 0.0
        )
        self._usage.last_request_id = request_id
        self._usage.last_model = model
        self._usage.last_stop_reason = stop_reason

        model_totals = self._usage.per_model.setdefault(model, ModelUsageTotals())
        model_totals.request_count += 1
        model_totals.input_tokens += normalized["input_tokens"]
        model_totals.output_tokens += normalized["output_tokens"]
        model_totals.cache_read_input_tokens += normalized["cache_read_input_tokens"]
        model_totals.cache_creation_input_tokens += normalized[
            "cache_creation_input_tokens"
        ]
        model_totals.web_search_requests += normalized["web_search_requests"]
        model_totals.web_fetch_requests += normalized["web_fetch_requests"]
        model_totals.total_cost_usd += normalized["cost_usd"]
        model_totals.total_duration_ms += duration_ms

        self._diagnostics.last_success_at = datetime.now(timezone.utc).isoformat()

    def record_api_error(
        self,
        *,
        request_id: str,
        model: str,
        error: Exception | str,
        duration_ms: float = 0.0,
    ) -> None:
        self._usage.request_count += 1
        self._usage.failed_requests += 1
        self._usage.total_duration_ms += duration_ms
        message = str(error)
        self._push_error(
            DiagnosticIncident(
                timestamp=datetime.now(timezone.utc).isoformat(),
                kind="api_error",
                message=message,
                request_id=request_id,
                model=model,
            )
        )

    def record_sink_failure(self, sink_name: str, error: Exception | str) -> None:
        self._diagnostics.sink_failures[sink_name] = (
            self._diagnostics.sink_failures.get(sink_name, 0) + 1
        )
        self._push_error(
            DiagnosticIncident(
                timestamp=datetime.now(timezone.utc).isoformat(),
                kind=f"{sink_name}_failure",
                message=str(error),
            )
        )

    def build_report(self) -> AnalyticsUsageReport:
        return AnalyticsUsageReport(
            usage=deepcopy(self._usage),
            diagnostics=deepcopy(self._diagnostics),
        )

    def reset_usage(self) -> None:
        self._usage = UsageTotals()

    def _push_error(self, incident: DiagnosticIncident) -> None:
        self._diagnostics.recent_errors.append(incident)
        if len(self._diagnostics.recent_errors) > self.max_recent_errors:
            self._diagnostics.recent_errors = self._diagnostics.recent_errors[
                -self.max_recent_errors :
            ]

    def _normalized_usage(
        self,
        model: str,
        usage: dict[str, Any],
        *,
        provider: str,
        fast_mode: bool,
    ) -> dict[str, float | int]:
        normalized: dict[str, float | int] = {
            "input_tokens": int(usage.get("input_tokens", 0) or 0),
            "output_tokens": int(usage.get("output_tokens", 0) or 0),
            "cache_read_input_tokens": int(
                usage.get("cache_read_input_tokens", 0) or 0
            ),
            "cache_creation_input_tokens": int(
                usage.get("cache_creation_input_tokens", 0) or 0
            ),
            "web_search_requests": int(usage.get("web_search_requests", 0) or 0),
            "web_fetch_requests": int(usage.get("web_fetch_requests", 0) or 0),
        }
        normalized["cost_usd"] = float(
            usage.get("cost_usd")
            or calculate_cost_usd(
                model,
                normalized,
                provider=provider,
                fast_mode=fast_mode,
            )
        )
        return normalized
