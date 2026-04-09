"""Centaur Psicode as a CrewAI Agent Adapter.

Makes the Centaur Psicode agent a first-class CrewAI citizen. It receives
tasks from a CrewAI crew, executes them using its own tools and reasoning
engine, and returns results through CrewAI's orchestration.

Usage:
    from integrations.crewai.centaur_adapter import CentaurPsicodeAdapter

    agent = CentaurPsicodeAdapter(
        role="Full-Stack Developer",
        goal="Implement and debug code with deep codebase access",
        backstory="Expert agent with 47+ tools.",
        socket_path="/tmp/agent-ipc.sock",
    )
    crew = Crew(agents=[agent, ...], tasks=[...])
    crew.kickoff()
"""

from __future__ import annotations

import asyncio
import logging
from collections.abc import Sequence
from typing import Any

try:
    from crewai.agents.agent_adapters.base_agent_adapter import BaseAgentAdapter
    from crewai.agents.agent_builder.base_agent import BaseAgent
    from crewai.tools.base_tool import BaseTool
except ImportError:
    raise ImportError("crewai is not installed. Install with: pip install crewai")

from .ipc_bridge import CentaurIpcClient
from .launcher import ensure_agent_brain_running

logger = logging.getLogger(__name__)


class CentaurPsicodeAdapter(BaseAgentAdapter):
    """CrewAI agent adapter backed by the Centaur Psicode engine.

    The adapter connects to the running agent-brain Python process via IPC,
    sends tasks as prompts, and collects the streaming LLM response. The
    agent-brain handles tool execution, memory, and all internal reasoning.
    """

    # ── Configuration ───────────────────────────────────────────────────
    socket_path: str = "/tmp/agent-ipc.sock"
    centaur_model: str = "claude-sonnet-4-6"
    centaur_provider: str = "first_party"
    centaur_api_key: str | None = None
    centaur_timeout: float = 300.0

    # ── Internal state ──────────────────────────────────────────────────
    _ipc: CentaurIpcClient | None = None
    _external_tools_desc: str = ""
    _structured_output_model: Any = None

    class Config:
        arbitrary_types_allowed = True

    def __init__(self, **kwargs: Any):
        super().__init__(**kwargs)
        # Ensure agent-brain is running
        if not ensure_agent_brain_running(self.socket_path):
            logger.warning(
                "Centaur Psicode agent-brain not reachable at %s. "
                "Start it with: python -m agent_brain.ipc_server",
                self.socket_path,
            )

    # ── BaseAgentAdapter required methods ───────────────────────────────

    def configure_tools(self, tools: list[BaseTool] | None = None) -> None:
        """Convert CrewAI tools to text descriptions for the agent's prompt.

        Centaur Psicode uses its own 47+ built-in tools internally.
        External CrewAI tools are described in the system prompt so the
        agent knows they exist, but actual execution stays within Centaur.
        """
        self._external_tools_desc = ""
        if not tools:
            return
        lines = ["Additional tools available from the CrewAI crew:"]
        for t in tools:
            lines.append(f"- {t.name}: {t.description}")
            if t.args_schema:
                try:
                    schema = t.args_schema.model_json_schema()
                    props = schema.get("properties", {})
                    if props:
                        params = ", ".join(props.keys())
                        lines.append(f"  Parameters: {params}")
                except Exception:
                    pass
        self._external_tools_desc = "\n".join(lines)

    def configure_structured_output(self, task: Any) -> None:
        """Detect if the task expects structured JSON/Pydantic output."""
        self._structured_output_model = None
        if hasattr(task, "output_json") and task.output_json:
            self._structured_output_model = task.output_json
        elif hasattr(task, "output_pydantic") and task.output_pydantic:
            self._structured_output_model = task.output_pydantic
        elif hasattr(task, "response_model") and task.response_model:
            self._structured_output_model = task.response_model

    # ── Task execution ──────────────────────────────────────────────────

    def execute_task(
        self,
        task: Any,
        context: str | None = None,
        tools: list[BaseTool] | None = None,
    ) -> str:
        """Synchronous task execution. Wraps the async version."""
        try:
            loop = asyncio.get_running_loop()
        except RuntimeError:
            loop = None

        if loop and loop.is_running():
            # We're inside an existing event loop (common in CrewAI)
            import concurrent.futures
            with concurrent.futures.ThreadPoolExecutor() as pool:
                future = pool.submit(
                    asyncio.run,
                    self.aexecute_task(task, context, tools),
                )
                return future.result(timeout=self.centaur_timeout + 30)
        else:
            return asyncio.run(self.aexecute_task(task, context, tools))

    async def aexecute_task(
        self,
        task: Any,
        context: str | None = None,
        tools: list[BaseTool] | None = None,
    ) -> str:
        """Async task execution via Centaur Psicode IPC."""
        # Emit CrewAI events if available
        try:
            from crewai.utilities.events.event_emitter import EventEmitter
            event_emitter = EventEmitter()
        except ImportError:
            event_emitter = None

        # Configure tools for this task
        if tools:
            self.configure_tools(tools)
        self.configure_structured_output(task)

        try:
            # Build the prompt from CrewAI task
            prompt = self._build_prompt(task, context)
            system = self._build_system_prompt()

            # Connect to Centaur Psicode agent-brain
            if self._ipc is None or not self._ipc.is_connected:
                self._ipc = CentaurIpcClient(self.socket_path)
                await self._ipc.connect()

            logger.info(
                "Centaur adapter: executing task (model=%s, prompt_len=%d)",
                self.centaur_model, len(prompt),
            )

            # Execute via IPC streaming
            result = await self._ipc.run_agent_task(
                prompt=prompt,
                model=self.centaur_model,
                system_prompt=system,
                provider=self.centaur_provider,
                api_key=self.centaur_api_key,
                timeout=self.centaur_timeout,
            )

            if not result.strip():
                result = "[Agent returned empty response]"

            # Parse structured output if requested
            if self._structured_output_model:
                result = self._parse_structured(result)

            logger.info(
                "Centaur adapter: task complete (result_len=%d)", len(result)
            )
            return result

        except Exception as e:
            error_msg = f"Centaur Psicode execution failed: {e}"
            logger.error(error_msg, exc_info=True)
            return error_msg

    # ── Prompt building ─────────────────────────────────────────────────

    def _build_prompt(self, task: Any, context: str | None = None) -> str:
        """Build a prompt from a CrewAI task + optional context."""
        parts: list[str] = []

        # Task description
        desc = getattr(task, "description", str(task))
        parts.append(f"# Task\n{desc}")

        # Expected output format
        expected = getattr(task, "expected_output", None)
        if expected:
            parts.append(f"\n# Expected Output\n{expected}")

        # Context from previous tasks in the crew
        if context:
            parts.append(f"\n# Context from Previous Tasks\n{context}")

        # Structured output instructions
        if self._structured_output_model:
            try:
                schema = self._structured_output_model.model_json_schema()
                parts.append(
                    f"\n# Output Format\n"
                    f"Return your response as JSON matching this schema:\n"
                    f"```json\n{schema}\n```"
                )
            except Exception:
                pass

        return "\n".join(parts)

    def _build_system_prompt(self) -> str:
        """Build a system prompt from the agent's CrewAI role/goal/backstory."""
        parts = [
            f"You are a {self.role}.",
            f"Your goal: {self.goal}",
        ]
        if self.backstory:
            parts.append(f"Background: {self.backstory}")
        if self._external_tools_desc:
            parts.append(f"\n{self._external_tools_desc}")
        return "\n".join(parts)

    def _parse_structured(self, raw_result: str) -> str:
        """Try to parse structured output from the LLM response."""
        import json

        text = raw_result.strip()
        # Strip markdown fences
        if text.startswith("```"):
            first_nl = text.index("\n") if "\n" in text else 3
            text = text[first_nl + 1:]
            if text.endswith("```"):
                text = text[:-3].strip()

        try:
            parsed = json.loads(text)
            if self._structured_output_model:
                # Validate against the model
                instance = self._structured_output_model(**parsed)
                return instance.model_dump_json(indent=2)
        except (json.JSONDecodeError, Exception):
            pass

        return raw_result

    # ── BaseAgent required stubs ────────────────────────────────────────
    # These abstract methods from BaseAgent are not needed for the adapter
    # pattern, but must be implemented to satisfy the ABC.

    def create_agent_executor(self, tools: list[BaseTool] | None = None) -> None:
        """Not needed — Centaur Psicode has its own execution engine."""
        pass

    def get_delegation_tools(self, agents: Sequence[BaseAgent]) -> list[BaseTool]:
        """Delegation is handled by CrewAI's orchestration layer."""
        return []

    def get_platform_tools(self, apps: list[Any]) -> list[BaseTool]:
        """Not supported — Centaur uses its own tool system."""
        return []

    def get_mcp_tools(self, mcps: list[Any]) -> list[BaseTool]:
        """Centaur has its own MCP support via agent-core."""
        return []
