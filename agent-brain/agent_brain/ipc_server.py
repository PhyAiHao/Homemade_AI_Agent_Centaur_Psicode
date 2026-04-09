from __future__ import annotations

import asyncio
import time
from collections.abc import Awaitable, Callable
from pathlib import Path
from typing import Any

from .analytics import AnalyticsService
from .api.client import AnthropicBackend, BackendRouter, OpenAIBackend, GeminiBackend, OllamaBackend, StreamRequest
from .api.errors import AgentApiError
from .compact import CompactService
from .crewai_service import CrewAIService
from .dream import DreamConsolidationService
from .wiki import WikiService
from .ipc_types import (
    ApiRequest,
    CompactRequest,
    CompactResponse,
    CostRequest,
    CostResponse,
    IpcPing,
    IpcPong,
    MemoryRequest,
    MemoryResponse,
    OutputStyleRequest,
    OutputStyleResponse,
    SkillRequest,
    SkillResponse,
    TextDelta,
    ToolResult,
    ToolUse,
    VoiceStart,
    VoiceTranscript,
    parse_ipc_message,
)
from .ipc_wire import read_frame, write_frame
from .memory import MemoryService
from .output_styles import OutputStyleLoader, OutputStyleService
from .plugins import PluginLoader
from .skills import SkillService
from .voice import VoiceService

MemoryHandler = Callable[[MemoryRequest], Awaitable[MemoryResponse]]
CompactHandler = Callable[[CompactRequest], Awaitable[CompactResponse]]
SkillHandler = Callable[[SkillRequest], Awaitable[SkillResponse]]
VoiceHandler = Callable[[VoiceStart], Awaitable[VoiceTranscript]]
OutputStyleHandler = Callable[[OutputStyleRequest], Awaitable[OutputStyleResponse]]
CostHandler = Callable[[CostRequest], Awaitable[CostResponse]]


class IpcServer:
    def __init__(
        self,
        *,
        socket_path: str | Path,
        backend: AnthropicBackend | BackendRouter | None = None,
        memory_root_dir: str | Path | None = None,
        skill_root_dir: str | Path | None = None,
        plugin_root_dir: str | Path | None = None,
        output_styles_dir: str | Path | None = None,
        analytics_service: AnalyticsService | None = None,
        memory_handler: MemoryHandler | None = None,
        compact_handler: CompactHandler | None = None,
        skill_handler: SkillHandler | None = None,
        voice_handler: VoiceHandler | None = None,
        output_style_handler: OutputStyleHandler | None = None,
        cost_handler: CostHandler | None = None,
    ) -> None:
        self.socket_path = Path(socket_path)
        if isinstance(backend, BackendRouter):
            self.backend = backend
        elif isinstance(backend, AnthropicBackend):
            self.backend = BackendRouter(anthropic=backend)
        else:
            self.backend = BackendRouter()
        self.memory_service = MemoryService(memory_root_dir or (self.socket_path.parent / "memory"))
        self.compact_service = CompactService()
        self.plugin_loader = PluginLoader(user_plugins_dir=plugin_root_dir)
        self.output_style_service = OutputStyleService(
            loader=OutputStyleLoader(
                user_output_styles_dir=output_styles_dir,
                plugin_loader=self.plugin_loader,
            )
        )
        self.analytics_service = analytics_service or AnalyticsService()
        self.analytics_service.initialize()
        self.skill_service = SkillService(project_dir=skill_root_dir or self.socket_path.parent)
        self.voice_service = VoiceService()
        self.crewai_service = CrewAIService()
        self.wiki_service = WikiService(
            store=self.memory_service.store,
            backend=self.backend,
        )
        self.dream_service = DreamConsolidationService(
            store=self.memory_service.store,
            backend=self.backend,
        )
        self.memory_handler = memory_handler or self.memory_service.handle
        self.compact_handler = compact_handler or self.compact_service.handle
        self.skill_handler = skill_handler or self.skill_service.handle
        self.voice_handler = voice_handler or self.voice_service.handle
        self.output_style_handler = output_style_handler or self.output_style_service.handle
        self.cost_handler = cost_handler or self._handle_cost_request
        self._server: asyncio.base_events.Server | None = None
        self._start_time_ms: int = int(time.time() * 1000)
        self.tool_results: dict[str, dict[str, Any]] = {}
        self.tool_calls: dict[str, dict[str, str]] = {}
        self.plugin_events: dict[str, list[dict[str, Any]]] = {}

    async def start(self) -> "IpcServer":
        if self.socket_path.exists():
            self.socket_path.unlink()
        self.socket_path.parent.mkdir(parents=True, exist_ok=True)
        self._server = await asyncio.start_unix_server(
            self._handle_client, path=str(self.socket_path)
        )
        return self

    async def close(self) -> None:
        await self.analytics_service.shutdown()
        if self._server is not None:
            self._server.close()
            await self._server.wait_closed()
            self._server = None
        if self.socket_path.exists():
            self.socket_path.unlink()

    async def serve_forever(self) -> None:
        if self._server is None:
            await self.start()
        assert self._server is not None
        async with self._server:
            await self._server.serve_forever()

    async def _handle_client(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
    ) -> None:
        try:
            while True:
                payload = await read_frame(reader)
                if payload is None:
                    break
                message = parse_ipc_message(payload)
                await self._dispatch(message, writer)
        except asyncio.IncompleteReadError:
            pass
        finally:
            writer.close()
            await writer.wait_closed()

    async def _dispatch(self, message: Any, writer: asyncio.StreamWriter) -> None:
        # Heartbeat — respond immediately, no processing
        if isinstance(message, IpcPing):
            uptime_ms = int(time.time() * 1000) - self._start_time_ms
            pong = IpcPong(request_id=message.request_id, status="ok", uptime_ms=uptime_ms)
            await write_frame(writer, pong.model_dump())
            return
        if isinstance(message, ApiRequest):
            await self._handle_api_request(message, writer)
            return
        # Media tool execution (TextToSpeech, TextToImage, TextToVideo)
        if isinstance(message, dict) and message.get("type") == "media_tool":
            await self._handle_media_tool(message, writer)
            return
        if isinstance(message, ToolResult):
            self.tool_results.setdefault(message.request_id, {})[
                message.tool_call_id
            ] = message.output
            return
        if isinstance(message, CostRequest):
            await write_frame(writer, (await self.cost_handler(message)).model_dump())
            return
        if isinstance(message, MemoryRequest):
            if message.action == "dream_consolidate":
                await write_frame(writer, (await self.dream_service.handle(message)).model_dump())
            elif message.action == "crewai_run":
                await write_frame(writer, (await self.crewai_service.handle(message)).model_dump())
            elif message.action in ("wiki_ingest", "wiki_query", "wiki_lint"):
                await write_frame(writer, (await self.wiki_service.handle(message)).model_dump())
            elif message.action.startswith("kg_"):
                await write_frame(writer, self._handle_kg(message).model_dump())
            elif message.action in ("memory_wake_up", "memory_l2", "memory_l3"):
                await write_frame(writer, self._handle_layers(message).model_dump())
            else:
                await write_frame(writer, (await self.memory_handler(message)).model_dump())
            return
        if isinstance(message, CompactRequest):
            await write_frame(writer, (await self._handle_compact_request(message)).model_dump())
            return
        if isinstance(message, SkillRequest):
            await write_frame(writer, (await self.skill_handler(message)).model_dump())
            return
        if isinstance(message, VoiceStart):
            await write_frame(writer, (await self.voice_handler(message)).model_dump())
            return
        if isinstance(message, OutputStyleRequest):
            await write_frame(
                writer, (await self.output_style_handler(message)).model_dump()
            )
            return
        raise ValueError(f"Unsupported IPC message: {message}")

    async def _handle_api_request(
        self, message: ApiRequest, writer: asyncio.StreamWriter
    ) -> None:
        import sys
        print(f"[IPC] API request: provider={message.provider!r} model={message.model!r}", file=sys.stderr)
        request = StreamRequest(
            request_id=message.request_id,
            model=message.model,
            messages=message.messages,
            tools=message.tools,
            system_prompt=self._augment_system_prompt(
                message.system_prompt,
                self._plugin_message_notes(message.request_id, message.messages),
            ),
            max_output_tokens=message.max_output_tokens,
            metadata=message.metadata,
            tool_choice=message.tool_choice,
            thinking=message.thinking,
            betas=message.betas,
            provider=message.provider,
            api_key=message.api_key,
            base_url=message.base_url,
            fast_mode=message.fast_mode,
        )
        started_at = time.perf_counter()
        try:
            async for event in self.backend.stream_message(request):
                ipc_event = parse_ipc_message(event)
                if isinstance(ipc_event, ToolUse):
                    self.tool_calls.setdefault(message.request_id, {})[
                        ipc_event.tool_call_id
                    ] = ipc_event.name
                if ipc_event.type == "message_done":
                    await self.analytics_service.record_api_success(
                        request=request,
                        usage=ipc_event.usage,
                        duration_ms=(time.perf_counter() - started_at) * 1000,
                        stop_reason=ipc_event.stop_reason,
                    )
                await write_frame(writer, ipc_event.model_dump())
        except AgentApiError as error:
            await self.analytics_service.record_api_error(
                request=request,
                error=error,
                duration_ms=(time.perf_counter() - started_at) * 1000,
            )
            await write_frame(
                writer,
                TextDelta(
                    request_id=message.request_id,
                    delta=f"API Error: {error}",
                ).model_dump(),
            )
            await write_frame(
                writer,
                {
                    "type": "message_done",
                    "request_id": message.request_id,
                    "usage": {},
                    "stop_reason": "error",
                },
            )

    async def _handle_media_tool(
        self, message: dict, writer: asyncio.StreamWriter
    ) -> None:
        """Handle media tool calls (TextToSpeech, TextToImage, TextToVideo)."""
        from .api.media_tools import execute_media_tool
        tool_name = message.get("tool_name", "")
        input_data = message.get("input", {})
        request_id = message.get("request_id", "")
        try:
            result = await execute_media_tool(tool_name, input_data)
            await write_frame(writer, {
                "type": "media_tool_result",
                "request_id": request_id,
                "tool_name": tool_name,
                "result": result,
                "is_error": False,
            })
        except Exception as e:
            await write_frame(writer, {
                "type": "media_tool_result",
                "request_id": request_id,
                "tool_name": tool_name,
                "result": {"error": str(e)},
                "is_error": True,
            })

    def _handle_kg(self, request: MemoryRequest) -> MemoryResponse:
        """Handle knowledge graph IPC actions."""
        try:
            from .memory.knowledge_graph import KnowledgeGraph
            kg = KnowledgeGraph(self.memory_service.store.root_dir / "knowledge_graph.sqlite3")
            payload = request.payload
            action = request.action

            if action == "kg_query":
                entity = str(payload.get("entity", ""))
                as_of = payload.get("as_of")
                direction = str(payload.get("direction", "both"))
                results = kg.query_entity(entity, as_of=as_of, direction=direction)
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"relationships": results, "count": len(results)},
                )
            elif action == "kg_add":
                tid = kg.add_triple(
                    subject=str(payload.get("subject", "")),
                    predicate=str(payload.get("predicate", "")),
                    obj=str(payload.get("object", "")),
                    valid_from=payload.get("valid_from"),
                    confidence=float(payload.get("confidence", 1.0)),
                    source_slug=payload.get("source_slug"),
                    subject_type=str(payload.get("subject_type", "concept")),
                    object_type=str(payload.get("object_type", "concept")),
                )
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"triple_id": tid},
                )
            elif action == "kg_invalidate":
                ok = kg.invalidate(
                    subject=str(payload.get("subject", "")),
                    predicate=str(payload.get("predicate", "")),
                    obj=str(payload.get("object", "")),
                    ended=payload.get("ended"),
                )
                return MemoryResponse(
                    request_id=request.request_id, ok=ok,
                    payload={"invalidated": ok},
                )
            elif action == "kg_timeline":
                entity = payload.get("entity")
                tl = kg.timeline(entity_name=entity)
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"timeline": tl, "count": len(tl)},
                )
            elif action == "kg_stats":
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload=kg.stats(),
                )
            else:
                return MemoryResponse(
                    request_id=request.request_id, ok=False,
                    error=f"Unknown KG action: {action}",
                )
        except Exception as e:
            return MemoryResponse(
                request_id=request.request_id, ok=False,
                error=f"KG error: {e}",
            )

    def _handle_layers(self, request: MemoryRequest) -> MemoryResponse:
        """Handle memory layer IPC actions (L0+L1, L2, L3)."""
        try:
            from .memory.layers import MemoryStack
            stack = MemoryStack(
                store=self.memory_service.store,
                vector=self.memory_service.store.vector_store,
            )
            payload = request.payload
            action = request.action

            if action == "memory_wake_up":
                topic = payload.get("topic")
                result = stack.wake_up(topic=topic)
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"context": result},
                )
            elif action == "memory_l2":
                topic = str(payload.get("topic", ""))
                wing = payload.get("wing")
                result = stack.l2_on_demand(topic, wing=wing)
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"context": result},
                )
            elif action == "memory_l3":
                query = str(payload.get("query", ""))
                limit = int(payload.get("limit", 10))
                result = stack.l3_deep_search(query, limit=limit)
                return MemoryResponse(
                    request_id=request.request_id, ok=True,
                    payload={"results": result},
                )
            else:
                return MemoryResponse(
                    request_id=request.request_id, ok=False,
                    error=f"Unknown layers action: {action}",
                )
        except Exception as e:
            return MemoryResponse(
                request_id=request.request_id, ok=False,
                error=f"Layers error: {e}",
            )

    async def _handle_cost_request(self, request: CostRequest) -> CostResponse:
        report = self.analytics_service.build_cost_report()
        response = CostResponse(
            request_id=request.request_id,
            usage=report.usage.model_dump(),
            diagnostics=report.diagnostics.model_dump(),
        )
        if request.reset:
            self.analytics_service.reset_usage()
        return response

    async def _handle_compact_request(
        self, request: CompactRequest
    ) -> CompactResponse:
        response = await self.compact_handler(request)
        hook_results = self.plugin_loader.dispatch_on_compact(
            summary=response.summary,
            messages=response.messages,
            context={"request_id": request.request_id},
        )
        if not hook_results:
            return response
        notes = "\n".join(f"- {item.content}" for item in hook_results)
        self.plugin_events.setdefault(request.request_id, []).extend(
            [item.model_dump() for item in hook_results]
        )
        return CompactResponse(
            request_id=response.request_id,
            summary=f"{response.summary}\n\n# Plugin Notes\n{notes}".strip(),
            messages=response.messages,
        )

    def _plugin_message_notes(
        self,
        request_id: str,
        messages: list[dict[str, Any]],
    ) -> list[str]:
        last_user_message = next(
            (message for message in reversed(messages) if message.get("role") == "user"),
            None,
        )
        if last_user_message is None:
            return []
        hook_results = self.plugin_loader.dispatch_on_message(
            last_user_message,
            context={"request_id": request_id},
        )
        if hook_results:
            self.plugin_events.setdefault(request_id, []).extend(
                [item.model_dump() for item in hook_results]
            )
        return [item.content for item in hook_results]

    def _augment_system_prompt(
        self,
        system_prompt: str | list[str] | None,
        plugin_notes: list[str],
    ) -> str | list[str] | None:
        if not plugin_notes:
            return system_prompt
        notes_block = "# Plugin Hook Notes\n" + "\n".join(
            f"- {note}" for note in plugin_notes
        )
        if system_prompt is None:
            return notes_block
        if isinstance(system_prompt, list):
            return [*system_prompt, notes_block]
        return f"{system_prompt}\n\n{notes_block}".strip()


def _main() -> None:
    """Entry point for ``python -m agent_brain.ipc_server``."""
    import os
    import signal
    import sys

    # Verify msgpack is available (Rust sends msgpack, not JSON)
    try:
        import msgpack as _mp
        print(f"agent-brain: msgpack {_mp.version} OK, python={sys.executable}", file=sys.stderr)
    except ImportError:
        print(
            "WARNING: 'msgpack' package not found! IPC will fail.\n"
            f"  Python: {sys.executable}\n"
            f"  Fix:    {sys.executable} -m pip install msgpack",
            file=sys.stderr,
        )

    # Load .env from current dir or parent dir (monorepo layout)
    for env_path in [".env", "../.env"]:
        if os.path.isfile(env_path):
            with open(env_path) as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        key, _, value = line.partition("=")
                        os.environ.setdefault(key.strip(), value.strip())
            break

    socket_path = os.environ.get("AGENT_IPC_SOCKET", "/tmp/agent-ipc.sock")
    anthropic_key = os.environ.get("ANTHROPIC_API_KEY")
    openai_key = os.environ.get("OPENAI_API_KEY")
    gemini_key = os.environ.get("GEMINI_API_KEY")

    # Only create backends that have API keys. Ollama always available (no key needed).
    backend = BackendRouter(
        anthropic=AnthropicBackend(api_key=anthropic_key) if anthropic_key else None,
        openai=OpenAIBackend(api_key=openai_key) if openai_key else None,
        gemini=GeminiBackend(api_key=gemini_key) if gemini_key else None,
        ollama=OllamaBackend(),
    )

    configured = [k for k, v in {"Anthropic": anthropic_key, "OpenAI": openai_key, "Gemini": gemini_key}.items() if v]
    print(f"agent-brain: providers: Ollama (always) + {configured or ['none']}", file=sys.stderr)

    server = IpcServer(socket_path=socket_path, backend=backend)

    async def _run() -> None:
        await server.start()
        print(f"agent-brain IPC server listening on {socket_path}", file=sys.stderr)
        try:
            await server.serve_forever()
        except asyncio.CancelledError:
            pass
        finally:
            await server.close()

    loop = asyncio.new_event_loop()

    def _shutdown(sig: int, frame: Any) -> None:
        print("\nagent-brain shutting down...", file=sys.stderr)
        for task in asyncio.all_tasks(loop):
            task.cancel()

    signal.signal(signal.SIGINT, _shutdown)
    signal.signal(signal.SIGTERM, _shutdown)

    try:
        loop.run_until_complete(_run())
    except KeyboardInterrupt:
        pass
    finally:
        loop.close()


if __name__ == "__main__":
    _main()
