from __future__ import annotations

from datetime import datetime, timezone
from typing import Any

from .._compat import Field
from ..types.base import AgentBaseModel


class GrowthBookUserAttributes(AgentBaseModel):
    id: str = ""
    session_id: str = ""
    device_id: str = ""
    platform: str = ""
    user_type: str = ""
    subscription_type: str = ""
    rate_limit_tier: str = ""
    app_version: str = ""
    organization_uuid: str = ""
    account_uuid: str = ""


class GrowthBookExperimentAssignment(AgentBaseModel):
    experiment_id: str
    variation_id: int
    in_experiment: bool = True
    hash_attribute: str | None = None
    hash_value: str | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)


class GrowthBookExposure(AgentBaseModel):
    feature_name: str
    experiment_id: str
    variation_id: int
    timestamp: str
    in_experiment: bool = True
    user_attributes: dict[str, Any] = Field(default_factory=dict)
    metadata: dict[str, Any] = Field(default_factory=dict)


class GrowthBookFeatureStore:
    """Local GrowthBook-style feature cache with override and exposure support."""

    def __init__(
        self,
        *,
        user_attributes: GrowthBookUserAttributes | None = None,
        features: dict[str, Any] | None = None,
        experiments: dict[str, GrowthBookExperimentAssignment] | None = None,
    ) -> None:
        self.user_attributes = user_attributes or GrowthBookUserAttributes()
        self._features: dict[str, Any] = dict(features or {})
        self._experiments: dict[str, GrowthBookExperimentAssignment] = dict(
            experiments or {}
        )
        self._overrides: dict[str, Any] = {}
        self._pending_exposures: list[GrowthBookExposure] = []
        self._seen_exposures: set[str] = set()

    def set_user_attributes(self, attributes: GrowthBookUserAttributes) -> None:
        self.user_attributes = attributes

    def refresh(
        self,
        *,
        features: dict[str, Any] | None = None,
        experiments: dict[str, GrowthBookExperimentAssignment] | None = None,
    ) -> None:
        if features is not None:
            self._features = dict(features)
        if experiments is not None:
            self._experiments = dict(experiments)
            self._seen_exposures.clear()

    def set_override(self, feature_name: str, value: Any) -> None:
        self._overrides[feature_name] = value

    def clear_override(self, feature_name: str) -> None:
        self._overrides.pop(feature_name, None)

    def get_feature_value(self, feature_name: str, default: Any = None) -> Any:
        value = self._resolve(feature_name, default)
        self._mark_exposure(feature_name)
        return value

    def check_gate(self, feature_name: str, default: bool = False) -> bool:
        value = self.get_feature_value(feature_name, default)
        if isinstance(value, bool):
            return value
        if value is None:
            return default
        return bool(value)

    def get_dynamic_config(self, feature_name: str, default: Any) -> Any:
        value = self.get_feature_value(feature_name, default)
        return default if value is None else value

    def get_all_features(self) -> dict[str, Any]:
        merged = dict(self._features)
        merged.update(self._overrides)
        return merged

    def consume_pending_exposures(self) -> list[GrowthBookExposure]:
        exposures = list(self._pending_exposures)
        self._pending_exposures.clear()
        return exposures

    def _resolve(self, feature_name: str, default: Any) -> Any:
        if feature_name in self._overrides:
            return self._overrides[feature_name]
        if feature_name in self._features:
            return self._features[feature_name]
        return default

    def _mark_exposure(self, feature_name: str) -> None:
        assignment = self._experiments.get(feature_name)
        if assignment is None:
            return
        dedupe_key = f"{feature_name}:{assignment.experiment_id}:{assignment.variation_id}"
        if dedupe_key in self._seen_exposures:
            return
        self._seen_exposures.add(dedupe_key)
        self._pending_exposures.append(
            GrowthBookExposure(
                feature_name=feature_name,
                experiment_id=assignment.experiment_id,
                variation_id=assignment.variation_id,
                timestamp=datetime.now(timezone.utc).isoformat(),
                in_experiment=assignment.in_experiment,
                user_attributes=self.user_attributes.model_dump(),
                metadata=assignment.metadata,
            )
        )
