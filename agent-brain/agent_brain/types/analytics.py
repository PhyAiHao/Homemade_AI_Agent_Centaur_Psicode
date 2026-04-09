from __future__ import annotations

from datetime import datetime

from .._compat import Field

from .auth import PublicApiAuth
from .base import AgentBaseModel


class GitHubActionsMetadata(AgentBaseModel):
    actor_id: str = ""
    repository_id: str = ""
    repository_owner_id: str = ""


class EnvironmentMetadata(AgentBaseModel):
    platform: str = ""
    node_version: str = ""
    terminal: str = ""
    package_managers: str = ""
    runtimes: str = ""
    is_running_with_bun: bool = False
    is_ci: bool = False
    is_claubbit: bool = False
    is_github_action: bool = False
    is_claude_code_action: bool = False
    is_claude_ai_auth: bool = False
    version: str = ""
    github_event_name: str = ""
    github_actions_runner_environment: str = ""
    github_actions_runner_os: str = ""
    github_action_ref: str = ""
    wsl_version: str = ""
    github_actions_metadata: GitHubActionsMetadata | None = None
    arch: str = ""
    is_claude_code_remote: bool = False
    remote_environment_type: str = ""
    claude_code_container_id: str = ""
    claude_code_remote_session_id: str = ""
    tags: list[str] = Field(default_factory=list)
    deployment_environment: str = ""
    is_conductor: bool = False
    version_base: str = ""
    coworker_type: str = ""
    build_time: str = ""
    is_local_agent_mode: bool = False
    linux_distro_id: str = ""
    linux_distro_version: str = ""
    linux_kernel: str = ""
    vcs: str = ""
    platform_raw: str = ""


class SlackContext(AgentBaseModel):
    slack_team_id: str = ""
    is_enterprise_install: bool = False
    trigger: str = ""
    creation_method: str = ""


class ClaudeCodeInternalEvent(AgentBaseModel):
    event_name: str = ""
    client_timestamp: datetime | None = None
    model: str = ""
    session_id: str = ""
    user_type: str = ""
    betas: str = ""
    env: EnvironmentMetadata | None = None
    entrypoint: str = ""
    agent_sdk_version: str = ""
    is_interactive: bool = False
    client_type: str = ""
    process: str = ""
    additional_metadata: str = ""
    auth: PublicApiAuth | None = None
    server_timestamp: datetime | None = None
    event_id: str = ""
    device_id: str = ""
    swe_bench_run_id: str = ""
    swe_bench_instance_id: str = ""
    swe_bench_task_id: str = ""
    email: str = ""
    agent_id: str = ""
    parent_session_id: str = ""
    agent_type: str = ""
    slack: SlackContext | None = None
    team_name: str = ""
    skill_name: str = ""
    plugin_name: str = ""
    marketplace_name: str = ""


class GrowthbookExperimentEvent(AgentBaseModel):
    event_id: str = ""
    timestamp: datetime | None = None
    experiment_id: str = ""
    variation_id: int = 0
    environment: str = ""
    user_attributes: str = ""
    experiment_metadata: str = ""
    device_id: str = ""
    auth: PublicApiAuth | None = None
    session_id: str = ""
    anonymous_id: str = ""
    event_metadata_vars: str = ""
