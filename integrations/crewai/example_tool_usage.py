"""Example: Centaur Psicode as a CrewAI Tool.

A coder agent uses Centaur Psicode for implementation, then a reviewer agent
(standard CrewAI) reviews the changes.

Usage:
    python example_tool_usage.py
"""

from crewai import Agent, Task, Crew, Process
from centaur_tool import centaur_psicode

# ── Agents ──────────────────────────────────────────────────────────────────

coder = Agent(
    role="Software Engineer",
    goal="Implement features and fix bugs using the Centaur Psicode agent",
    backstory=(
        "You are a developer with access to a powerful coding agent via the "
        "centaur_psicode tool. Use it for all implementation tasks — it can "
        "read files, edit code, run tests, and debug issues."
    ),
    tools=[centaur_psicode],
    llm="claude-sonnet-4-6",
    verbose=True,
)

reviewer = Agent(
    role="Code Reviewer",
    goal="Review code changes for correctness, security, and quality",
    backstory=(
        "You are a senior engineer. You receive implementation summaries "
        "and review them for edge cases, security issues, and design quality."
    ),
    llm="claude-sonnet-4-6",
    verbose=True,
)

# ── Tasks ───────────────────────────────────────────────────────────────────

implement = Task(
    description="Fix the authentication bug in the login endpoint",
    expected_output="A description of what was changed, which files were modified, and why",
    agent=coder,
)

review = Task(
    description=(
        "Review the implementation changes. Check for:\n"
        "- Edge cases not handled\n"
        "- Security vulnerabilities\n"
        "- Code quality issues\n"
        "- Missing tests"
    ),
    expected_output="Detailed review with approval or requested changes",
    agent=reviewer,
    context=[implement],
)

# ── Crew ────────────────────────────────────────────────────────────────────

crew = Crew(
    agents=[coder, reviewer],
    tasks=[implement, review],
    process=Process.sequential,
    verbose=True,
)

if __name__ == "__main__":
    result = crew.kickoff()
    print("\n" + "=" * 60)
    print("FINAL RESULT")
    print("=" * 60)
    print(result)
