from .base import AgentBaseModel


class PublicApiAuth(AgentBaseModel):
    """Authentication context injected by upstream APIs."""

    account_id: int = 0
    organization_uuid: str = ""
    account_uuid: str = ""
