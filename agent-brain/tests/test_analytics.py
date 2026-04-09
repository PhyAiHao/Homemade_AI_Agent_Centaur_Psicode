from __future__ import annotations

import unittest

from agent_brain.analytics import (
    AnalyticsService,
    DatadogAnalyticsSink,
    DiagnosticsTracker,
    FirstPartyEventLogger,
    GrowthBookExperimentAssignment,
    GrowthBookFeatureStore,
    InMemoryDatadogTransport,
    InMemoryFirstPartyTransport,
    should_sample_event,
)
from agent_brain.api.client import StreamRequest


class GrowthBookTests(unittest.TestCase):
    def test_feature_store_supports_overrides_and_exposures(self) -> None:
        store = GrowthBookFeatureStore(
            features={
                "tengu_log_datadog_events": True,
                "tengu_event_sampling_config": {
                    "tengu_api_success": {"sample_rate": 0.25}
                },
            },
            experiments={
                "tengu_log_datadog_events": GrowthBookExperimentAssignment(
                    experiment_id="exp-1",
                    variation_id=1,
                )
            },
        )

        self.assertTrue(store.check_gate("tengu_log_datadog_events"))
        exposures = store.consume_pending_exposures()
        self.assertEqual(len(exposures), 1)
        self.assertEqual(exposures[0].experiment_id, "exp-1")

        store.set_override("tengu_log_datadog_events", False)
        self.assertFalse(store.check_gate("tengu_log_datadog_events"))

    def test_should_sample_event_returns_zero_when_dropped(self) -> None:
        sampled = should_sample_event(
            "tengu_api_success",
            {"tengu_api_success": {"sample_rate": 0.25}},
            random_value=0.9,
        )
        self.assertEqual(sampled, 0.0)


class AnalyticsServiceTests(unittest.IsolatedAsyncioTestCase):
    async def test_service_queues_before_initialize_and_flushes_to_sinks(self) -> None:
        growthbook = GrowthBookFeatureStore(
            features={"tengu_log_datadog_events": True}
        )
        datadog_transport = InMemoryDatadogTransport()
        first_party_transport = InMemoryFirstPartyTransport()
        service = AnalyticsService(
            growthbook=growthbook,
            datadog=DatadogAnalyticsSink(sender=datadog_transport),
            first_party=FirstPartyEventLogger(
                growthbook=growthbook,
                sender=first_party_transport,
            ),
        )

        service.log_event("tengu_started", {"interactive": True})
        self.assertEqual(datadog_transport.batches, [])
        self.assertEqual(first_party_transport.batches, [])

        service.initialize()
        await service.shutdown()

        self.assertEqual(len(datadog_transport.batches), 1)
        self.assertEqual(
            datadog_transport.batches[0][0]["message"],
            "tengu_started",
        )
        self.assertEqual(len(first_party_transport.batches), 1)
        self.assertEqual(
            first_party_transport.batches[0][0]["event_name"],
            "tengu_started",
        )

    async def test_diagnostics_tracker_accumulates_usage_and_errors(self) -> None:
        tracker = DiagnosticsTracker()
        tracker.record_api_success(
            request_id="req-1",
            model="sonnet",
            usage={"input_tokens": 11, "output_tokens": 7, "cost_usd": 0.12},
            duration_ms=320,
            stop_reason="end_turn",
        )
        tracker.record_api_error(
            request_id="req-2",
            model="sonnet",
            error=RuntimeError("network timeout"),
            duration_ms=120,
        )

        report = tracker.build_report()
        self.assertEqual(report.usage.request_count, 2)
        self.assertEqual(report.usage.successful_requests, 1)
        self.assertEqual(report.usage.failed_requests, 1)
        self.assertEqual(report.usage.total_input_tokens, 11)
        self.assertAlmostEqual(report.usage.total_cost_usd, 0.12)
        self.assertEqual(report.diagnostics.recent_errors[-1].kind, "api_error")

    async def test_service_records_api_success_and_error_events(self) -> None:
        growthbook = GrowthBookFeatureStore(
            features={"tengu_log_datadog_events": True}
        )
        datadog_transport = InMemoryDatadogTransport()
        first_party_transport = InMemoryFirstPartyTransport()
        service = AnalyticsService(
            growthbook=growthbook,
            datadog=DatadogAnalyticsSink(sender=datadog_transport),
            first_party=FirstPartyEventLogger(
                growthbook=growthbook,
                sender=first_party_transport,
            ),
        )
        service.initialize()
        request = StreamRequest(
            request_id="req-3",
            model="sonnet",
            messages=[{"role": "user", "content": "hello"}],
        )

        await service.record_api_success(
            request=request,
            usage={"input_tokens": 8, "output_tokens": 2, "cost_usd": 0.03},
            duration_ms=45,
            stop_reason="end_turn",
        )
        await service.record_api_error(
            request=request,
            error=RuntimeError("bad gateway"),
            duration_ms=50,
        )
        await service.shutdown()

        self.assertEqual(service.build_cost_report().usage.request_count, 2)
        self.assertEqual(len(datadog_transport.batches), 1)
        datadog_messages = [item["message"] for item in datadog_transport.batches[0]]
        self.assertIn("tengu_api_success", datadog_messages)
        self.assertIn("tengu_api_error", datadog_messages)
        first_party_names = [item["event_name"] for item in first_party_transport.batches[0]]
        self.assertIn("tengu_api_success", first_party_names)
        self.assertIn("tengu_api_error", first_party_names)
