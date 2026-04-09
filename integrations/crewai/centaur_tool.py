"""Centaur Psicode as a CrewAI Tool.

Wraps the Centaur Psicode AI agent as a CrewAI-compatible tool.
CrewAI agents can call this to delegate complex software engineering
tasks (file editing, debugging, multi-step reasoning) to your agent.

Usage:
    from integrations.crewai.centaur_tool import centaur_psicode, centaur_psicode_async
    agent = Agent(role="...", tools=[centaur_psicode])
"""

from __future__ import annotations

import asyncio
import subprocess
from typing import Any

from pydantic import BaseModel, Field

try:
    from crewai.tools.base_tool import BaseTool
except ImportError:
    raise ImportError(
        "crewai is not installed. Install with: pip install crewai"
    )


# ── Pydantic input schema ──────────────────────────────────────────────────

class CentaurPsicodeInput(BaseModel):
    """Input schema for the Centaur Psicode tool."""
    prompt: str = Field(..., description="The task for the agent to perform")
    working_dir: str = Field(
        ".", description="Working directory for the agent"
    )
    model: str = Field(
        "claude-sonnet-4-6",
        description="LLM model to use (e.g., claude-sonnet-4-6, claude-opus-4-6)",
    )
    max_turns: int = Field(
        30, description="Maximum reasoning turns before stopping"
    )
    timeout: int = Field(
        300, description="Timeout in seconds for the agent execution"
    )


# ── Synchronous tool ───────────────────────────────────────────────────────

class CentaurPsicodeTool(BaseTool):
    """Run a task using the Centaur Psicode AI agent.

    Use this for complex software engineering tasks that require file editing,
    code analysis, debugging, or multi-step reasoning. The agent has access to
    the full codebase, terminal, memory system, and 47+ built-in tools.
    """

    name: str = "centaur_psicode"
    description: str = (
        "Run a task using the Centaur Psicode AI agent. Use for complex "
        "software engineering tasks: file editing, code analysis, debugging, "
        "multi-step reasoning. The agent has full codebase access, terminal, "
        "memory, and 47+ built-in tools."
    )
    args_schema: type[BaseModel] = CentaurPsicodeInput

    def _run(
        self,
        prompt: str,
        working_dir: str = ".",
        model: str = "claude-sonnet-4-6",
        max_turns: int = 30,
        timeout: int = 300,
        **kwargs: Any,
    ) -> str:
        """Execute a task synchronously via the agent CLI."""
        cmd = ["agent", "--bare"]
        if model:
            cmd.extend(["--model", model])
        cmd.append(prompt)

        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=timeout,
                cwd=working_dir,
            )
            output = result.stdout.strip()
            if result.returncode != 0 and result.stderr:
                output += f"\n[stderr]: {result.stderr.strip()}"
            return output or "[No output from agent]"
        except subprocess.TimeoutExpired:
            return f"[Agent timed out after {timeout}s]"
        except FileNotFoundError:
            return (
                "[Error: 'agent' binary not found. "
                "Ensure Centaur Psicode is installed and 'agent' is in PATH.]"
            )

    async def _arun(
        self,
        prompt: str,
        working_dir: str = ".",
        model: str = "claude-sonnet-4-6",
        max_turns: int = 30,
        timeout: int = 300,
        **kwargs: Any,
    ) -> str:
        """Execute a task asynchronously via the agent CLI."""
        cmd = ["agent", "--bare"]
        if model:
            cmd.extend(["--model", model])
        cmd.append(prompt)

        try:
            proc = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
                cwd=working_dir,
            )
            stdout, stderr = await asyncio.wait_for(
                proc.communicate(), timeout=timeout
            )
            output = stdout.decode().strip()
            if proc.returncode != 0 and stderr:
                output += f"\n[stderr]: {stderr.decode().strip()}"
            return output or "[No output from agent]"
        except asyncio.TimeoutError:
            proc.kill()
            return f"[Agent timed out after {timeout}s]"
        except FileNotFoundError:
            return (
                "[Error: 'agent' binary not found. "
                "Ensure Centaur Psicode is installed and 'agent' is in PATH.]"
            )


# Convenience instances for direct import
centaur_psicode = CentaurPsicodeTool()
centaur_psicode_async = CentaurPsicodeTool()  # Same class, both sync+async supported
