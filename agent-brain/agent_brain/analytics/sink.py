from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from ..api.client import StreamRequest
from .datadog import DatadogAnalyticsSink
from .diagnostics import AnalyticsUsageReport, DiagnosticsTracker
from .first_party_logger import FirstPartyEventLogger
from .growthbook import GrowthBookExperimentAssignment, GrowthBookFeatureStore
from .metadata import EventMetadataBuilder

DATADOG_GATE_NAME = "tengu_log_datadog_events"
SINK_KILLSWITCH_CONFIG_NAME = "tengu_frond_boric"


@dataclass
class _QueuedEvent:
    event_name: str
    metadata: dict[str, Any]


class AnalyticsService:
    def __init__(
        self,
        *,
        growthbook: GrowthBookFeatureStore | None = None,
        datadog: DatadogAnalyticsSink | None = None,
        first_party: FirstPartyEventLogger | None = None,
        metadata_builder: EventMetadataBuilder | None = None,
        diagnostics: DiagnosticsTracker | None = None,
    ) -> None:
        self.growthbook = growthbook or GrowthBookFeatureStore()
        self.metadata_builder = metadata_builder or EventMetadataBuilder()
        self.diagnostics = diagnostics or DiagnosticsTracker()
        self.datadog = datadog or DatadogAnalyticsSink()
        self.first_party = first_party or FirstPartyEventLogger(
            metadata_builder=self.metadata_builder,
            growthbook=self.growthbook,
        )
        self._event_queue: list[_QueuedEvent] = []
        self._initialized = False

    def initialize(self) -> None:
        self._initialized = True
        queued = list(self._event_queue)
        self._event_queue.clear()
        for item in queued:
            self._dispatch(item.event_name, item.metadata)

    def refresh_growthbook(
        self,
        *,
        features: dict[str, Any] | None = None,
        experiments: dict[str, GrowthBookExperimentAssignment] | None = None,
    ) -> None:
        self.growthbook.refresh(features=features, experiments=experiments)
        self._flush_growthbook_exposures()

    def log_event(self, event_name: str, metadata: dict[str, Any] | None = None) -> None:
        payload = dict(metadata or {})
        self.diagnostics.record_event(event_name)
        if not self._initialized:
            self._event_queue.append(_QueuedEvent(event_name, payload))
            return
        self._dispatch(event_name, payload)

    async def log_event_async(
        self, event_name: str, metadata: dict[str, Any] | None = None
    ) -> None:
        self.log_event(event_name, metadata)

    async def record_api_success(
        self,
        *,
        request: StreamRequest,
        usage: dict[str, Any],
        duration_ms: float,
        stop_reason: str | None,
    ) -> None:
        self.diagnostics.record_api_success(
            request_id=request.request_id,
            model=request.model,
            usage=usage,
            provider=request.provider,
            fast_mode=request.fast_mode,
            duration_ms=duration_ms,
            stop_reason=stop_reason,
        )
        self.log_event(
            "tengu_api_success",
            {
                "model": request.model,
                "request_id": request.request_id,
                "tool_count": len(request.tools),
                "fast_mode": request.fast_mode,
                "duration_ms": int(duration_ms),
                "input_tokens": int(usage.get("input_tokens", 0) or 0),
                "output_tokens": int(usage.get("output_tokens", 0) or 0),
                "web_search_requests": int(usage.get("web_search_requests", 0) or 0),
            },
        )

    async def record_api_error(
        self,
        *,
        request: StreamRequest,
        error: Exception | str,
        duration_ms: float,
    ) -> None:
        self.diagnostics.record_api_error(
            request_id=request.request_id,
            model=request.model,
            error=error,
            duration_ms=duration_ms,
        )
        self.log_event(
            "tengu_api_error",
            {
                "model": request.model,
                "request_id": request.request_id,
                "duration_ms": int(duration_ms),
                "fast_mode": request.fast_mode,
            },
        )

    def build_cost_report(self) -> AnalyticsUsageReport:
        return self.diagnostics.build_report()

    def reset_usage(self) -> None:
        self.diagnostics.reset_usage()

    async def shutdown(self) -> None:
        await self.datadog.flush()
        await self.first_party.flush()

    def _dispatch(self, event_name: str, metadata: dict[str, Any]) -> None:
        if self._should_dispatch_to_datadog():
            try:
                self.datadog.log_event(event_name, metadata)
            except Exception as error:  # pragma: no cover - defensive
                self.diagnostics.record_sink_failure("datadog", error)

        if not self._is_sink_killed("first_party"):
            try:
                self.first_party.log_event(event_name, metadata)
            except Exception as error:  # pragma: no cover - defensive
                self.diagnostics.record_sink_failure("first_party", error)

        self._flush_growthbook_exposures()

    def _should_dispatch_to_datadog(self) -> bool:
        if self._is_sink_killed("datadog"):
            return False
        return self.growthbook.check_gate(DATADOG_GATE_NAME, default=False)

    def _is_sink_killed(self, sink_name: str) -> bool:
        killswitch = self.growthbook.get_dynamic_config(SINK_KILLSWITCH_CONFIG_NAME, {})
        if isinstance(killswitch, dict):
            return bool(killswitch.get(sink_name))
        return False

    def _flush_growthbook_exposures(self) -> None:
        for exposure in self.growthbook.consume_pending_exposures():
            try:
                self.first_party.log_growthbook_experiment(exposure)
            except Exception as error:  # pragma: no cover - defensive
                self.diagnostics.record_sink_failure("first_party", error)
