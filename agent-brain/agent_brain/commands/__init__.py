from .base import CommandContext, PromptCommand
from .commit import COMMIT_COMMAND, prepare_commit
from .init import INIT_COMMAND, prepare_init
from .insights import INSIGHTS_COMMAND, prepare_insights
from .review import REVIEW_COMMAND, prepare_review
from .security_review import SECURITY_REVIEW_COMMAND, prepare_security_review
from .ultraplan import ULTRAPLAN_COMMAND, prepare_ultraplan

COMMAND_REGISTRY = {
    REVIEW_COMMAND.name: REVIEW_COMMAND,
    COMMIT_COMMAND.name: COMMIT_COMMAND,
    ULTRAPLAN_COMMAND.name: ULTRAPLAN_COMMAND,
    SECURITY_REVIEW_COMMAND.name: SECURITY_REVIEW_COMMAND,
    INIT_COMMAND.name: INIT_COMMAND,
    INSIGHTS_COMMAND.name: INSIGHTS_COMMAND,
}

__all__ = [
    "COMMAND_REGISTRY",
    "COMMIT_COMMAND",
    "CommandContext",
    "INIT_COMMAND",
    "INSIGHTS_COMMAND",
    "PromptCommand",
    "REVIEW_COMMAND",
    "SECURITY_REVIEW_COMMAND",
    "ULTRAPLAN_COMMAND",
    "prepare_commit",
    "prepare_init",
    "prepare_insights",
    "prepare_review",
    "prepare_security_review",
    "prepare_ultraplan",
]
