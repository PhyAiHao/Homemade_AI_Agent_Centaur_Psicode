from __future__ import annotations

import unittest

from agent_brain.models.selector import ModelSelectionContext, select_model


class SelectorTests(unittest.TestCase):
    def test_default_model_for_payg_is_sonnet(self) -> None:
        selected = select_model(ModelSelectionContext(subscriber_tier="payg"))
        self.assertEqual(selected.resolved.descriptor.key, "sonnet46")

    def test_default_model_for_max_is_opus(self) -> None:
        selected = select_model(ModelSelectionContext(subscriber_tier="max"))
        self.assertEqual(selected.resolved.descriptor.key, "opus46")

    def test_plan_mode_upgrades_haiku_to_sonnet(self) -> None:
        selected = select_model(
            ModelSelectionContext(
                subscriber_tier="payg",
                requested_model="haiku",
                permission_mode="plan",
            )
        )
        self.assertEqual(selected.resolved.descriptor.key, "sonnet46")

    def test_long_context_applies_1m_to_supported_models(self) -> None:
        selected = select_model(
            ModelSelectionContext(
                requested_model="sonnet",
                long_context=True,
            )
        )
        self.assertTrue(selected.resolved.resolved_model.endswith("[1m]"))
        self.assertEqual(selected.resolved.context_window, 1_000_000)
