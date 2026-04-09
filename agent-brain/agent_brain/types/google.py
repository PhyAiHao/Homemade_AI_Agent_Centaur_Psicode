from .base import AgentBaseModel


class Timestamp(AgentBaseModel):
    """Initial Pydantic port of google.protobuf.Timestamp."""

    seconds: int = 0
    nanos: int = 0
