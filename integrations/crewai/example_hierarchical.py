"""Example: Hierarchical Crew with Multiple Centaur Agents.

A manager coordinates two Centaur agents (implementer + tester)
to build and verify a feature.
"""

from crewai import Agent, Crew, Task, Process
from centaur_adapter import CentaurPsicodeAdapter

# ── Centaur agents with different roles ─────────────────────────────────

implementer = CentaurPsicodeAdapter(
    role="Implementation Engineer",
    goal="Write clean, well-structured code",
    backstory="Expert at coding with full file and terminal access.",
    socket_path="/tmp/agent-ipc.sock",
    centaur_model="claude-sonnet-4-6",
)

tester = CentaurPsicodeAdapter(
    role="QA Engineer",
    goal="Write comprehensive tests and report any failures",
    backstory="Expert at finding edge cases and writing test suites.",
    socket_path="/tmp/agent-ipc.sock",
    centaur_model="claude-sonnet-4-6",
)

# ── Standard CrewAI manager ─────────────────────────────────────────────

manager = Agent(
    role="Project Manager",
    goal="Coordinate implementation and testing to deliver a working feature",
    backstory=(
        "Experienced PM who breaks work into clear tasks and ensures "
        "quality at every step."
    ),
    llm="claude-sonnet-4-6",
    allow_delegation=True,
    verbose=True,
)

# ── Tasks ───────────────────────────────────────────────────────────────

implement = Task(
    description="Add rate limiting to the API endpoints (max 100 req/min per user)",
    expected_output="Implementation summary with changed files and design notes",
    agent=implementer,
)

test = Task(
    description=(
        "Write integration tests for the rate limiting feature. Test:\n"
        "- Normal usage within limits\n"
        "- Exceeding the limit\n"
        "- Rate limit reset after timeout\n"
        "- Multiple users with independent limits"
    ),
    expected_output="Test results with pass/fail counts and any issues found",
    agent=tester,
    context=[implement],
)

# ── Hierarchical crew (manager coordinates) ─────────────────────────────

crew = Crew(
    agents=[implementer, tester, manager],
    tasks=[implement, test],
    process=Process.hierarchical,
    manager_agent=manager,
    verbose=True,
)

if __name__ == "__main__":
    result = crew.kickoff()
    print(result)
