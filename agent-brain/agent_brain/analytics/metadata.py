from __future__ import annotations

import json
import os
import platform
import socket
import sys
from datetime import datetime, timezone
from uuid import uuid4

from ..types.analytics import ClaudeCodeInternalEvent, EnvironmentMetadata, GrowthbookExperimentEvent
from ..types.base import AgentBaseModel
from .growthbook import GrowthBookExposure


class EventMetadataBuilder(AgentBaseModel):
    app_version: str = "0.1.0"
    client_type: str = "centaur-agent-brain"
    user_type: str = ""
    entrypoint: str = "ipc"
    process_name: str = "agent-brain"
    session_id: str = ""
    device_id: str = ""

    def build_environment_metadata(self) -> EnvironmentMetadata:
        return EnvironmentMetadata(
            platform=platform.system().lower(),
            node_version="",
            terminal=os.environ.get("TERM", ""),
            package_managers="",
            runtimes=f"python/{platform.python_version()}",
            is_running_with_bun=False,
            is_ci=bool(os.environ.get("CI")),
            is_claubbit=False,
            is_github_action=bool(os.environ.get("GITHUB_ACTIONS")),
            is_claude_code_action=False,
            is_claude_ai_auth=False,
            version=self.app_version,
            arch=platform.machine(),
            platform_raw=platform.platform(),
            linux_kernel=platform.release(),
            vcs=os.environ.get("VCS", ""),
            version_base=self.app_version.split("+", 1)[0],
            deployment_environment=os.environ.get("DEPLOYMENT_ENVIRONMENT", ""),
        )

    def build_internal_event(
        self,
        event_name: str,
        *,
        metadata: dict[str, object] | None = None,
        model: str = "",
        session_id: str | None = None,
    ) -> ClaudeCodeInternalEvent:
        payload = metadata or {}
        serialized = json.dumps(payload, sort_keys=True, default=str)
        return ClaudeCodeInternalEvent(
            event_name=event_name,
            client_timestamp=datetime.now(timezone.utc),
            server_timestamp=datetime.now(timezone.utc),
            event_id=str(uuid4()),
            model=model or str(payload.get("model", "")),
            session_id=session_id or str(payload.get("session_id", self.session_id)),
            user_type=self.user_type or str(payload.get("user_type", "")),
            env=self.build_environment_metadata(),
            entrypoint=self.entrypoint,
            agent_sdk_version=self.app_version,
            is_interactive=bool(payload.get("interactive", False)),
            client_type=self.client_type,
            process=self.process_name,
            additional_metadata=serialized,
            device_id=self.device_id or socket.gethostname(),
            parent_session_id=str(payload.get("parent_session_id", "")),
            team_name=str(payload.get("team_name", "")),
            skill_name=str(payload.get("skill_name", "")),
            plugin_name=str(payload.get("plugin_name", "")),
            marketplace_name=str(payload.get("marketplace_name", "")),
        )

    def build_growthbook_experiment_event(
        self,
        exposure: GrowthBookExposure,
    ) -> GrowthbookExperimentEvent:
        return GrowthbookExperimentEvent(
            event_id=str(uuid4()),
            timestamp=datetime.now(timezone.utc),
            experiment_id=exposure.experiment_id,
            variation_id=exposure.variation_id,
            environment="production",
            user_attributes=json.dumps(exposure.user_attributes, sort_keys=True),
            experiment_metadata=json.dumps(exposure.metadata, sort_keys=True),
            device_id=self.device_id or socket.gethostname(),
            session_id=self.session_id,
            anonymous_id=self.device_id or socket.gethostname(),
            event_metadata_vars=json.dumps(
                {
                    "feature_name": exposure.feature_name,
                    "in_experiment": exposure.in_experiment,
                    "timestamp": exposure.timestamp,
                },
                sort_keys=True,
            ),
        )
