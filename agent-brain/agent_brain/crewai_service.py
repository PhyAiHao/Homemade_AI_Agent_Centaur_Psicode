"""CrewAI service — runs CrewAI crews on behalf of the Centaur Psicode agent.

When the LLM calls the CrewAI tool, the Rust side sends a MemoryRequest with
action="crewai_run" to the Python brain. This service parses the crew config,
creates CrewAI agents/tasks, executes the crew, and returns the result.

Crews can be defined in two ways:
  1. **Inline JSON** — the LLM builds the config dynamically per request
  2. **YAML templates** — reusable crew definitions in agent_brain/crews/*.yaml
     that you customize once and invoke by name

Usage:
  Inline:  {"crew_config": {"agents": [...], "tasks": [...]}, "inputs": {...}}
  Template: {"crew_name": "code_review", "inputs": {"topic": "auth module"}}
  List:    {"list_crews": true}  — returns available crew templates
"""

from __future__ import annotations

import logging
from pathlib import Path
from typing import Any

from .ipc_types import MemoryRequest, MemoryResponse

logger = logging.getLogger(__name__)

# Directory containing crew YAML templates
CREWS_DIR = Path(__file__).resolve().parent / "crews"


class CrewAIService:
    """Runs CrewAI crews on behalf of the Centaur Psicode agent."""

    async def handle(self, request: MemoryRequest) -> MemoryResponse:
        """Handle a 'crewai_run' IPC action."""
        try:
            payload = request.payload

            # ── List available crew templates ──
            if payload.get("list_crews"):
                crews = self._list_crew_templates()
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=True,
                    payload={"crews": crews},
                )

            # ── Load crew config: from template name OR inline JSON ──
            crew_name = payload.get("crew_name", "")
            crew_config = payload.get("crew_config", {})
            inputs = payload.get("inputs", {})

            if crew_name:
                # Load from YAML template
                crew_config = self._load_crew_template(crew_name)
                if crew_config is None:
                    available = [c["name"] for c in self._list_crew_templates()]
                    return MemoryResponse(
                        request_id=request.request_id,
                        ok=False,
                        error=(
                            f"Crew template '{crew_name}' not found. "
                            f"Available crews: {available}. "
                            f"Create new ones in: {CREWS_DIR}/"
                        ),
                        payload={},
                    )

            if not crew_config:
                return MemoryResponse(
                    request_id=request.request_id,
                    ok=False,
                    error=(
                        "crewai_run requires either 'crew_name' (template) or "
                        "'crew_config' (inline JSON) in payload. "
                        "Use {\"list_crews\": true} to see available templates."
                    ),
                    payload={},
                )

            # Convert to dict if needed
            if hasattr(crew_config, "items"):
                crew_config = dict(crew_config)
            elif isinstance(crew_config, str):
                import json
                crew_config = json.loads(crew_config)

            if hasattr(inputs, "items"):
                inputs = dict(inputs)

            result = await self._run_crew(crew_config, inputs)

            return MemoryResponse(
                request_id=request.request_id,
                ok=True,
                payload={"result": result},
            )

        except ImportError:
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=(
                    "CrewAI is not installed. "
                    "Install with: /opt/homebrew/opt/python@3.10/libexec/bin/python3 -m pip install crewai"
                ),
                payload={},
            )
        except Exception as error:
            logger.exception("CrewAI execution failed")
            return MemoryResponse(
                request_id=request.request_id,
                ok=False,
                error=f"CrewAI execution error: {error}",
                payload={},
            )

    # ── Template Management ────────────────────────────────────────

    @staticmethod
    def _list_crew_templates() -> list[dict[str, str]]:
        """List all available crew YAML templates."""
        import yaml
        crews = []
        if not CREWS_DIR.is_dir():
            return crews
        for f in sorted(CREWS_DIR.glob("*.yaml")):
            try:
                data = yaml.safe_load(f.read_text(encoding="utf-8"))
                crews.append({
                    "name": data.get("name", f.stem),
                    "description": data.get("description", ""),
                    "file": str(f),
                    "agents": len(data.get("agents", [])),
                    "tasks": len(data.get("tasks", [])),
                    "process": data.get("process", "sequential"),
                })
            except Exception:
                crews.append({"name": f.stem, "description": "(parse error)", "file": str(f)})
        return crews

    @staticmethod
    def _load_crew_template(name: str) -> dict[str, Any] | None:
        """Load a crew template by name from the crews/ directory."""
        import yaml
        path = CREWS_DIR / f"{name}.yaml"
        if not path.exists():
            # Try case-insensitive match
            for f in CREWS_DIR.glob("*.yaml"):
                if f.stem.lower() == name.lower():
                    path = f
                    break
            else:
                return None
        try:
            return yaml.safe_load(path.read_text(encoding="utf-8"))
        except Exception as e:
            logger.error("Failed to load crew template %s: %s", name, e)
            return None

    async def _run_crew(self, config: dict[str, Any], inputs: dict[str, Any]) -> str:
        """Parse a crew config dict and execute the crew."""
        from crewai import Agent, Crew, Process, Task

        # ── Parse agents ────────────────────────────────────────────────
        agents: dict[str, Agent] = {}
        for agent_cfg in config.get("agents", []):
            name = agent_cfg.get("name", agent_cfg.get("role", "agent"))
            agent = Agent(
                role=agent_cfg["role"],
                goal=agent_cfg["goal"],
                backstory=agent_cfg.get("backstory", ""),
                llm=agent_cfg.get("llm", "claude-sonnet-4-6"),
                verbose=agent_cfg.get("verbose", False),
                allow_delegation=agent_cfg.get("allow_delegation", False),
            )
            agents[name] = agent

        if not agents:
            raise ValueError("crew_config must define at least one agent")

        # ── Parse tasks ─────────────────────────────────────────────────
        tasks: list[Task] = []
        for task_cfg in config.get("tasks", []):
            agent_name = task_cfg.get("agent")
            agent = agents.get(agent_name) if agent_name else None

            # Resolve context references (indices into the tasks list)
            context_indices = task_cfg.get("context_indices", [])
            context_tasks = [tasks[i] for i in context_indices if i < len(tasks)]

            task = Task(
                description=task_cfg["description"],
                expected_output=task_cfg.get("expected_output", ""),
                agent=agent,
                context=context_tasks or None,
                async_execution=task_cfg.get("async_execution", False),
            )
            tasks.append(task)

        if not tasks:
            raise ValueError("crew_config must define at least one task")

        # ── Parse process type ──────────────────────────────────────────
        process_str = config.get("process", "sequential")
        process = (
            Process.hierarchical
            if process_str == "hierarchical"
            else Process.sequential
        )

        # ── Build and run crew ──────────────────────────────────────────
        crew_kwargs: dict[str, Any] = {
            "agents": list(agents.values()),
            "tasks": tasks,
            "process": process,
            "verbose": config.get("verbose", False),
        }

        # Optional: manager agent for hierarchical process
        manager_name = config.get("manager_agent")
        if manager_name and manager_name in agents:
            crew_kwargs["manager_agent"] = agents[manager_name]

        crew = Crew(**crew_kwargs)

        logger.info(
            "CrewAI: running crew with %d agents, %d tasks, process=%s",
            len(agents), len(tasks), process_str,
        )

        result = crew.kickoff(inputs=inputs)

        logger.info("CrewAI: crew completed, result_len=%d", len(str(result)))
        return str(result)
