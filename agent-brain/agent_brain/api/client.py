from __future__ import annotations

from collections.abc import AsyncIterator
from typing import Any

from .._compat import Field
try:
    from anthropic import AsyncAnthropic
except ImportError:  # pragma: no cover - dependency is declared in pyproject
    AsyncAnthropic = None  # type: ignore[assignment]

from ..models.catalog import APIProvider
from ..models.selector import ModelSelectionContext, select_model
from ..types.base import AgentBaseModel
from .errors import AgentApiError
from .streaming import AnthropicStreamNormalizer, OpenAIStreamNormalizer, GeminiStreamNormalizer

try:
    from openai import AsyncOpenAI
except ImportError:
    AsyncOpenAI = None  # type: ignore[assignment]

try:
    from google import genai as google_genai
except ImportError:
    google_genai = None  # type: ignore[assignment]


class StreamRequest(AgentBaseModel):
    request_id: str
    model: str
    messages: list[dict[str, Any]]
    tools: list[dict[str, Any]] = Field(default_factory=list)
    system_prompt: str | list[Any] | None = None
    metadata: dict[str, Any] = Field(default_factory=dict)
    max_output_tokens: int | None = None
    tool_choice: dict[str, Any] | None = None
    thinking: dict[str, Any] | None = None
    betas: list[str] = Field(default_factory=list)
    provider: APIProvider = "first_party"
    api_key: str | None = None
    base_url: str | None = None
    fast_mode: bool = False


class AnthropicBackend:
    def __init__(
        self,
        *,
        api_key: str | None = None,
        base_url: str | None = None,
        max_retries: int = 0,
        client: Any | None = None,
    ) -> None:
        self.api_key = api_key
        self.base_url = base_url
        self.max_retries = max_retries
        self._client = client

    async def stream_message(self, request: StreamRequest) -> AsyncIterator[dict[str, Any]]:
        try:
            client = self._client or self._create_client(request)
            selected = select_model(
                ModelSelectionContext(
                    provider=request.provider,
                    requested_model=request.model,
                    long_context="[1m]" in request.model.lower(),
                    fast_mode=request.fast_mode,
                )
            )
            params = self._build_request_params(request, selected.resolved)
            normalizer = AnthropicStreamNormalizer(
                request_id=request.request_id,
                model=selected.resolved.resolved_model,
                provider=request.provider,
                fast_mode=request.fast_mode,
            )
            async with client.beta.messages.stream(**params) as raw_stream:
                async for event in normalizer.normalize(raw_stream):
                    yield event
        except Exception as error:
            if isinstance(error, AgentApiError):
                raise
            raise AgentApiError.from_exception(error) from error

    def _create_client(self, request: StreamRequest) -> Any:
        if AsyncAnthropic is None:
            raise RuntimeError(
                "anthropic is not installed. Inject a fake client for tests or install dependencies."
            )
        api_key = request.api_key or self.api_key
        if not api_key:
            raise RuntimeError(
                "No Anthropic API key found. "
                "Set ANTHROPIC_API_KEY in .env or export it in your shell. "
                "If using the VS Code extension, restart the agent after setting the key."
            )
        kwargs: dict[str, Any] = {
            "api_key": api_key,
            "max_retries": self.max_retries,
        }
        base_url = request.base_url or self.base_url
        if base_url:
            kwargs["base_url"] = base_url
        return AsyncAnthropic(**kwargs)

    def _build_request_params(self, request: StreamRequest, resolved_model: Any) -> dict[str, Any]:
        params: dict[str, Any] = {
            "model": resolved_model.resolved_model,
            "messages": request.messages,
            "max_tokens": request.max_output_tokens or resolved_model.default_max_output_tokens,
        }
        if request.system_prompt is not None:
            params["system"] = request.system_prompt
        if request.tools:
            params["tools"] = request.tools
        if request.tool_choice is not None:
            params["tool_choice"] = request.tool_choice
        if request.thinking is not None:
            params["thinking"] = request.thinking
        if request.metadata:
            params["metadata"] = request.metadata
        if request.betas:
            params["betas"] = request.betas
        return params


class OpenAIBackend:
    """OpenAI-compatible backend (works with OpenAI, Azure OpenAI, etc.)."""

    def __init__(self, *, api_key: str | None = None, base_url: str | None = None) -> None:
        self.api_key = api_key
        self.base_url = base_url

    async def stream_message(self, request: StreamRequest) -> AsyncIterator[dict[str, Any]]:
        try:
            client = self._create_client(request)
            params = self._build_params(request)
            normalizer = OpenAIStreamNormalizer(
                request_id=request.request_id,
                model=request.model,
            )
            stream = await client.chat.completions.create(**params, stream=True, stream_options={"include_usage": True})
            async for event in normalizer.normalize(stream):
                yield event
        except Exception as error:
            if isinstance(error, AgentApiError):
                raise
            raise AgentApiError.from_exception(error) from error

    def _create_client(self, request: StreamRequest) -> Any:
        if AsyncOpenAI is None:
            raise RuntimeError("openai package is not installed. Run: pip install openai")
        api_key = request.api_key or self.api_key
        if not api_key:
            raise RuntimeError(
                "No OpenAI API key found. Set OPENAI_API_KEY in .env or export it in your shell."
            )
        kwargs: dict[str, Any] = {"api_key": api_key}
        base_url = request.base_url or self.base_url
        if base_url:
            kwargs["base_url"] = base_url
        return AsyncOpenAI(**kwargs)

    def _build_params(self, request: StreamRequest) -> dict[str, Any]:
        messages = self._translate_messages(request)
        params: dict[str, Any] = {"model": request.model, "messages": messages}
        if request.max_output_tokens:
            params["max_tokens"] = request.max_output_tokens
        if request.tools:
            params["tools"] = self._translate_tools(request.tools)
        if request.tool_choice is not None:
            params["tool_choice"] = self._translate_tool_choice(request.tool_choice)
        # GLM-5 reasoning mode: enable "thinking" for models that support it
        if request.thinking and request.model.startswith("glm"):
            params["thinking"] = {"type": "enabled"}
        return params

    def _translate_messages(self, request: StreamRequest) -> list[dict[str, Any]]:
        import json as _json
        msgs: list[dict[str, Any]] = []
        # System prompt → system message
        if request.system_prompt:
            sp = request.system_prompt
            if isinstance(sp, list):
                sp = " ".join(b.get("text", "") if isinstance(b, dict) else str(b) for b in sp)
            msgs.append({"role": "system", "content": sp})
        for msg in request.messages:
            role = msg.get("role", "user")
            content = msg.get("content")
            # Simple string content
            if isinstance(content, str):
                msgs.append({"role": role, "content": content})
                continue
            # Content blocks (list)
            if not isinstance(content, list):
                msgs.append({"role": role, "content": str(content) if content else ""})
                continue
            # Process content blocks
            text_parts: list[str] = []
            tool_calls: list[dict[str, Any]] = []
            tool_results: list[dict[str, Any]] = []
            for block in content:
                if not isinstance(block, dict):
                    continue
                btype = block.get("type", "")
                if btype == "text":
                    text_parts.append(block.get("text", ""))
                elif btype == "tool_use":
                    tool_calls.append({
                        "id": block.get("id", ""),
                        "type": "function",
                        "function": {
                            "name": block.get("name", ""),
                            "arguments": _json.dumps(block.get("input", {})),
                        },
                    })
                elif btype == "tool_result":
                    rc = block.get("content", "")
                    if isinstance(rc, list):
                        rc = " ".join(b.get("text", "") if isinstance(b, dict) else str(b) for b in rc)
                    tool_results.append({
                        "role": "tool",
                        "tool_call_id": block.get("tool_use_id", ""),
                        "content": str(rc),
                    })
            # Emit assistant message with tool_calls
            if role == "assistant":
                m: dict[str, Any] = {"role": "assistant"}
                if text_parts:
                    m["content"] = "\n".join(text_parts)
                else:
                    m["content"] = None
                if tool_calls:
                    m["tool_calls"] = tool_calls
                msgs.append(m)
            elif tool_results:
                # Tool results become separate "tool" role messages
                for tr in tool_results:
                    msgs.append(tr)
            else:
                msgs.append({"role": role, "content": "\n".join(text_parts) if text_parts else ""})
        return msgs

    @staticmethod
    def _translate_tools(tools: list[dict[str, Any]]) -> list[dict[str, Any]]:
        return [
            {
                "type": "function",
                "function": {
                    "name": t.get("name", ""),
                    "description": t.get("description", ""),
                    "parameters": t.get("input_schema", {}),
                },
            }
            for t in tools
        ]

    @staticmethod
    def _translate_tool_choice(tc: dict[str, Any]) -> Any:
        tc_type = tc.get("type", "auto")
        if tc_type == "auto":
            return "auto"
        if tc_type == "any":
            return "required"
        if tc_type == "tool":
            return {"type": "function", "function": {"name": tc.get("name", "")}}
        return "auto"


class GeminiBackend:
    """Google Gemini backend using the google-genai SDK."""

    def __init__(self, *, api_key: str | None = None) -> None:
        self.api_key = api_key

    async def stream_message(self, request: StreamRequest) -> AsyncIterator[dict[str, Any]]:
        try:
            client = self._create_client(request)
            contents, config = self._build_params(request)
            normalizer = GeminiStreamNormalizer(
                request_id=request.request_id,
                model=request.model,
            )
            stream = client.models.generate_content_stream(
                model=request.model,
                contents=contents,
                config=config,
            )
            async for event in normalizer.normalize(stream):
                yield event
        except Exception as error:
            if isinstance(error, AgentApiError):
                raise
            raise AgentApiError.from_exception(error) from error

    def _create_client(self, request: StreamRequest) -> Any:
        if google_genai is None:
            raise RuntimeError("google-genai package is not installed. Run: pip install google-genai")
        api_key = request.api_key or self.api_key
        if not api_key:
            raise RuntimeError("No Gemini API key provided. Set GEMINI_API_KEY environment variable.")
        return google_genai.Client(api_key=api_key)

    def _build_params(self, request: StreamRequest) -> tuple[list[dict[str, Any]], dict[str, Any]]:
        import json as _json
        contents: list[dict[str, Any]] = []
        for msg in request.messages:
            role = msg.get("role", "user")
            gemini_role = "model" if role == "assistant" else "user"
            content = msg.get("content")
            if isinstance(content, str):
                contents.append({"role": gemini_role, "parts": [{"text": content}]})
                continue
            if not isinstance(content, list):
                contents.append({"role": gemini_role, "parts": [{"text": str(content) if content else ""}]})
                continue
            parts: list[dict[str, Any]] = []
            for block in content:
                if not isinstance(block, dict):
                    continue
                btype = block.get("type", "")
                if btype == "text":
                    parts.append({"text": block.get("text", "")})
                elif btype == "tool_use":
                    parts.append({"function_call": {"name": block.get("name", ""), "args": block.get("input", {})}})
                elif btype == "tool_result":
                    rc = block.get("content", "")
                    if isinstance(rc, list):
                        rc = " ".join(b.get("text", "") if isinstance(b, dict) else str(b) for b in rc)
                    parts.append({"function_response": {"name": block.get("name", "tool"), "response": {"result": str(rc)}}})
            if parts:
                contents.append({"role": gemini_role, "parts": parts})

        config: dict[str, Any] = {}
        if request.system_prompt:
            sp = request.system_prompt
            if isinstance(sp, list):
                sp = " ".join(b.get("text", "") if isinstance(b, dict) else str(b) for b in sp)
            config["system_instruction"] = sp
        if request.max_output_tokens:
            config["max_output_tokens"] = request.max_output_tokens
        if request.tools:
            config["tools"] = [{"function_declarations": [
                {"name": t.get("name", ""), "description": t.get("description", ""), "parameters": t.get("input_schema", {})}
                for t in request.tools
            ]}]
        return contents, config


class OllamaBackend:
    """Ollama backend — uses OpenAI-compatible API at localhost. No API key needed."""

    def __init__(self, *, base_url: str = "http://localhost:11434/v1") -> None:
        self._base_url = base_url
        self._inner = OpenAIBackend(api_key="ollama", base_url=base_url)

    async def stream_message(self, request: StreamRequest) -> AsyncIterator[dict[str, Any]]:
        # Always force Ollama's local URL and dummy key — never use env vars
        request = request.model_copy(update={
            "base_url": self._base_url,
            "api_key": "ollama",
        })
        async for event in self._inner.stream_message(request):
            yield event


class BackendRouter:
    """Routes StreamRequests to the correct provider backend."""

    def __init__(
        self,
        *,
        anthropic: AnthropicBackend | None = None,
        openai: OpenAIBackend | None = None,
        gemini: GeminiBackend | None = None,
        ollama: OllamaBackend | None = None,
    ) -> None:
        self._backends: dict[str, Any] = {
            "ollama": ollama or OllamaBackend(),
        }
        # Only register backends that have API keys configured
        if anthropic:
            self._backends["first_party"] = anthropic
        if openai:
            self._backends["openai"] = openai
        if gemini:
            self._backends["gemini"] = gemini

    # Models that should always route to Ollama (local models)
    _OLLAMA_MODEL_PREFIXES = (
        "llama", "gemma", "mistral", "codellama", "deepseek",
        "phi", "qwen", "starcoder", "vicuna", "wizard", "yi",
        "orca", "neural", "tinyllama", "dolphin", "falcon",
    )

    async def stream_message(self, request: StreamRequest) -> AsyncIterator[dict[str, Any]]:
        import sys
        provider = request.provider

        # Bedrock/vertex/foundry route through Anthropic
        if provider in ("bedrock", "vertex", "foundry"):
            provider = "first_party"

        # Auto-detect: if model looks like a local model, force Ollama
        model_lower = request.model.lower()
        if any(model_lower.startswith(p) for p in self._OLLAMA_MODEL_PREFIXES):
            provider = "ollama"

        backend = self._backends.get(provider)

        # Fallback: if the requested provider isn't configured (no API key)
        if backend is None:
            # Don't send cloud model names (Claude/GPT/Gemini) to Ollama — that will 404
            cloud_prefixes = ("claude", "gpt", "o1", "o3", "o4", "gemini")
            is_cloud_model = any(model_lower.startswith(p) for p in cloud_prefixes)

            if is_cloud_model:
                available = list(self._backends.keys())
                raise AgentApiError(
                    f"Model '{request.model}' requires provider '{request.provider}' "
                    f"but no API key is configured for it. "
                    f"Set the API key in .env or choose a local model (Ollama). "
                    f"Configured providers: {available}"
                )

            # For non-cloud models, fall back to Ollama
            if "ollama" in self._backends:
                provider = "ollama"
                backend = self._backends["ollama"]
                print(f"[Router] Falling back to Ollama for model '{request.model}'", file=sys.stderr)
            else:
                raise AgentApiError(f"No provider available for model '{request.model}'")
        print(f"[Router] provider={provider!r} model={request.model!r} → {type(backend).__name__}", file=sys.stderr)
        async for event in backend.stream_message(request):
            yield event
