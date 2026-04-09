from .._compat import BaseModel, ConfigDict


class AgentBaseModel(BaseModel):
    """Shared strict base model for the Python intelligence layer."""

    model_config = ConfigDict(extra="ignore", populate_by_name=True)
