from __future__ import annotations

from typing import Any, Literal, Union

from ._compat import Field
from .types.base import AgentBaseModel


class BaseIpcMessage(AgentBaseModel):
    type: str
    request_id: str


class ApiRequest(BaseIpcMessage):
    type: Literal["api_request"] = "api_request"
    messages: list[dict[str, Any]]
    model: str
    tools: list[dict[str, Any]] = Field(default_factory=list)
    system_prompt: str | list[Any] | None = None
    max_output_tokens: int | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)
    tool_choice: dict[str, Any] | None = None
    thinking: dict[str, Any] | None = None
    betas: list[str] = Field(default_factory=list)
    provider: str = "first_party"
    api_key: str | None = None
    base_url: str | None = None
    fast_mode: bool = False


class TextDelta(BaseIpcMessage):
    type: Literal["text_delta"] = "text_delta"
    delta: str


class ToolUse(BaseIpcMessage):
    type: Literal["tool_use"] = "tool_use"
    tool_call_id: str
    name: str
    input: dict[str, Any] = Field(default_factory=dict)
    server_tool_use: bool = False


class ToolResult(BaseIpcMessage):
    type: Literal["tool_result"] = "tool_result"
    tool_call_id: str
    output: Any
    is_error: bool = False


class MessageDone(BaseIpcMessage):
    type: Literal["message_done"] = "message_done"
    usage: dict[str, Any] = Field(default_factory=dict)
    stop_reason: str | None = None


class CostRequest(BaseIpcMessage):
    type: Literal["cost_request"] = "cost_request"
    reset: bool = False


class CostResponse(BaseIpcMessage):
    type: Literal["cost_response"] = "cost_response"
    usage: dict[str, Any] = Field(default_factory=dict)
    diagnostics: dict[str, Any] = Field(default_factory=dict)


class MemoryRequest(BaseIpcMessage):
    type: Literal["memory_request"] = "memory_request"
    action: str
    payload: dict[str, Any] = Field(default_factory=dict)


class MemoryResponse(BaseIpcMessage):
    type: Literal["memory_response"] = "memory_response"
    ok: bool
    payload: dict[str, Any] = Field(default_factory=dict)
    error: str | None = None


class CompactRequest(BaseIpcMessage):
    type: Literal["compact_request"] = "compact_request"
    messages: list[dict[str, Any]]
    token_budget: int | None = None


class CompactResponse(BaseIpcMessage):
    type: Literal["compact_response"] = "compact_response"
    summary: str
    messages: list[dict[str, Any]] = Field(default_factory=list)


class SkillRequest(BaseIpcMessage):
    type: Literal["skill_request"] = "skill_request"
    skill_name: str
    arguments: dict[str, Any] = Field(default_factory=dict)


class SkillResponse(BaseIpcMessage):
    type: Literal["skill_response"] = "skill_response"
    content: str
    metadata: dict[str, Any] = Field(default_factory=dict)


class VoiceStart(BaseIpcMessage):
    type: Literal["voice_start"] = "voice_start"
    language: str | None = None
    audio_b64: str | None = None
    audio_path: str | None = None
    keyterms: list[str] = Field(default_factory=list)
    recent_files: list[str] = Field(default_factory=list)
    project_dir: str | None = None
    branch_name: str | None = None
    transcript_hint: str | None = None


class VoiceTranscript(BaseIpcMessage):
    type: Literal["voice_transcript"] = "voice_transcript"
    text: str
    is_final: bool = True
    metadata: dict[str, Any] = Field(default_factory=dict)


class OutputStyleRequest(BaseIpcMessage):
    type: Literal["output_style_request"] = "output_style_request"
    style_name: str


class OutputStyleResponse(BaseIpcMessage):
    type: Literal["output_style_response"] = "output_style_response"
    style: dict[str, Any] = Field(default_factory=dict)


class IpcPing(BaseIpcMessage):
    """Heartbeat ping from Rust to Python. Expects IpcPong response."""
    type: Literal["ipc_ping"] = "ipc_ping"


class IpcPong(BaseIpcMessage):
    """Heartbeat pong from Python to Rust."""
    type: Literal["ipc_pong"] = "ipc_pong"
    status: str = "ok"
    uptime_ms: int = 0


IncomingMessage = Union[
    ApiRequest,
    ToolResult,
    CostRequest,
    MemoryRequest,
    CompactRequest,
    SkillRequest,
    VoiceStart,
    OutputStyleRequest,
    IpcPing,
]

OutgoingMessage = Union[
    TextDelta,
    ToolUse,
    MessageDone,
    CostResponse,
    MemoryResponse,
    CompactResponse,
    SkillResponse,
    VoiceTranscript,
    OutputStyleResponse,
    IpcPong,
]


MESSAGE_TYPES: dict[str, type[BaseIpcMessage]] = {
    "api_request": ApiRequest,
    "text_delta": TextDelta,
    "tool_use": ToolUse,
    "tool_result": ToolResult,
    "message_done": MessageDone,
    "cost_request": CostRequest,
    "cost_response": CostResponse,
    "memory_request": MemoryRequest,
    "memory_response": MemoryResponse,
    "compact_request": CompactRequest,
    "compact_response": CompactResponse,
    "skill_request": SkillRequest,
    "skill_response": SkillResponse,
    "voice_start": VoiceStart,
    "voice_transcript": VoiceTranscript,
    "output_style_request": OutputStyleRequest,
    "output_style_response": OutputStyleResponse,
    "ipc_ping": IpcPing,
    "ipc_pong": IpcPong,
}


def parse_ipc_message(payload: dict[str, Any]) -> BaseIpcMessage:
    message_type = payload.get("type")
    if not isinstance(message_type, str):
        raise ValueError("IPC payload missing string 'type' field")
    cls = MESSAGE_TYPES.get(message_type)
    if cls is None:
        raise ValueError(f"Unsupported IPC message type: {message_type}")
    return cls(**payload)
