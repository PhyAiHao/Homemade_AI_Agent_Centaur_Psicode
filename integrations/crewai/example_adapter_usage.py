"""Example: Centaur Psicode as a CrewAI Agent (Sequential Crew).

The Centaur agent handles implementation with its full tool suite,
then a standard CrewAI agent reviews the output.
"""

from crewai import Agent, Crew, Task, Process
from centaur_adapter import CentaurPsicodeAdapter

# ── Centaur Psicode agent (full tool access) ────────────────────────────

centaur = CentaurPsicodeAdapter(
    role="Full-Stack Developer",
    goal="Implement features and fix bugs with deep codebase access",
    backstory=(
        "Expert agent with 47+ tools: file read/write/edit, grep, glob, "
        "bash terminal, sub-agents, git worktrees, memory system, and more. "
        "You have full access to the project codebase."
    ),
    socket_path="/tmp/agent-ipc.sock",
    centaur_model="claude-sonnet-4-6",
)

# ── Standard CrewAI agent (for review) ──────────────────────────────────

reviewer = Agent(
    role="Code Reviewer",
    goal="Review code changes for correctness, security, and quality",
    backstory="Senior engineer focused on finding edge cases and security issues.",
    llm="claude-sonnet-4-6",
    verbose=True,
)

# ── Tasks ───────────────────────────────────────────────────────────────

implement = Task(
    description=(
        "Refactor the payment processing module to use the strategy pattern. "
        "Create separate strategy classes for each payment method (credit card, "
        "PayPal, crypto). Update the existing code to use the new pattern."
    ),
    expected_output=(
        "Summary of changes: which files were modified, what was added, "
        "what was removed, and the design rationale."
    ),
    agent=centaur,
)

review = Task(
    description=(
        "Review the refactoring for:\n"
        "1. Correctness — does the strategy pattern make sense here?\n"
        "2. Backwards compatibility — are existing callers broken?\n"
        "3. Test coverage — are the new strategies tested?\n"
        "4. Edge cases — what happens with invalid payment methods?"
    ),
    expected_output="Detailed review with approval or requested changes",
    agent=reviewer,
    context=[implement],
)

# ── Run the crew ────────────────────────────────────────────────────────

crew = Crew(
    agents=[centaur, reviewer],
    tasks=[implement, review],
    process=Process.sequential,
    verbose=True,
)

if __name__ == "__main__":
    result = crew.kickoff()
    print(result)
