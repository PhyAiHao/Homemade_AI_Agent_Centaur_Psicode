"""Centaur Psicode Web GUI Server.

A single Python process that:
  - Serves the GUI HTML on HTTP (port 8420)
  - Provides REST endpoints for settings and sessions
  - Bridges WebSocket messages to BackendRouter for real-time streaming chat

No IPC required — imports BackendRouter directly from agent_brain.
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import signal
import sys
import time
import uuid
from pathlib import Path
from typing import Any
from urllib.parse import parse_qs, urlparse

# Ensure agent-brain is importable
_BRAIN_DIR = Path(__file__).resolve().parent.parent / "agent-brain"
if str(_BRAIN_DIR) not in sys.path:
    sys.path.insert(0, str(_BRAIN_DIR))

logger = logging.getLogger("gui.server")

# ═══════════════════════════════════════════════════════════════════════
# Settings Manager
# ═══════════════════════════════════════════════════════════════════════

class SettingsManager:
    """Reads/writes .env file, tracks which providers have keys."""

    def __init__(self, env_path: Path) -> None:
        self.env_path = env_path
        self.config: dict[str, str] = {}
        self.load()

    def load(self) -> None:
        self.config.clear()
        if not self.env_path.exists():
            return
        for line in self.env_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                key, _, value = line.partition("=")
                key = key.strip()
                value = value.strip().strip('"').strip("'")
                self.config[key] = value
                os.environ.setdefault(key, value)

    def save(self) -> None:
        lines: list[str] = []
        written_keys: set[str] = set()

        # Preserve existing file structure
        if self.env_path.exists():
            for line in self.env_path.read_text(encoding="utf-8").splitlines():
                stripped = line.strip()
                if stripped and not stripped.startswith("#") and "=" in stripped:
                    key = stripped.partition("=")[0].strip()
                    if key in self.config:
                        val = self.config[key]
                        if any(c in val for c in (" ", "=", "#", "'", '"')):
                            lines.append(f'{key}="{val}"')
                        else:
                            lines.append(f"{key}={val}")
                        written_keys.add(key)
                    # Skip keys removed from config
                else:
                    lines.append(line)

        # Append new keys (quote values containing special characters)
        for key, value in self.config.items():
            if key not in written_keys:
                if any(c in value for c in (" ", "=", "#", "'", '"')):
                    lines.append(f'{key}="{value}"')
                else:
                    lines.append(f"{key}={value}")

        self.env_path.parent.mkdir(parents=True, exist_ok=True)
        self.env_path.write_text("\n".join(lines) + "\n", encoding="utf-8")

    def _mask(self, key: str) -> str:
        val = self.config.get(key, "")
        if not val or len(val) < 8:
            return ""
        return f"...{val[-4:]}"

    def get_status(self) -> dict[str, Any]:
        # Check both .env config AND live environment variables
        has_anthropic = bool(self.config.get("ANTHROPIC_API_KEY") or os.environ.get("ANTHROPIC_API_KEY"))
        has_openai = bool(self.config.get("OPENAI_API_KEY") or os.environ.get("OPENAI_API_KEY"))
        has_gemini = bool(self.config.get("GEMINI_API_KEY") or os.environ.get("GEMINI_API_KEY"))

        # Auto-detect best default provider based on what's configured
        explicit_provider = self.config.get("AGENT_PROVIDER", "")
        if explicit_provider:
            default_provider = explicit_provider
        elif has_anthropic:
            default_provider = "first_party"
        elif has_openai:
            default_provider = "openai"
        elif has_gemini:
            default_provider = "gemini"
        else:
            default_provider = "ollama"

        # Auto-detect best default model
        explicit_model = self.config.get("CLAUDE_MODEL", "")
        if explicit_model:
            default_model = explicit_model
        elif default_provider == "first_party":
            default_model = "claude-sonnet-4-6"
        elif default_provider == "openai":
            default_model = "gpt-4o"
        elif default_provider == "gemini":
            default_model = "gemini-2.0-flash"
        else:
            # Pick first actually installed Ollama model
            installed = _detect_ollama_models()
            default_model = installed[0]["id"] if installed else "llama3.1:latest"

        return {
            "anthropic": {
                "configured": has_anthropic,
                "masked_key": self._mask("ANTHROPIC_API_KEY") or self._mask_env("ANTHROPIC_API_KEY"),
            },
            "openai": {
                "configured": has_openai,
                "masked_key": self._mask("OPENAI_API_KEY") or self._mask_env("OPENAI_API_KEY"),
            },
            "gemini": {
                "configured": has_gemini,
                "masked_key": self._mask("GEMINI_API_KEY") or self._mask_env("GEMINI_API_KEY"),
            },
            "ollama": {"configured": True, "url": "http://localhost:11434"},
            "selected_model": default_model,
            "selected_provider": default_provider,
            # Media provider keys
            "elevenlabs": {
                "configured": bool(self.config.get("ELEVENLABS_API_KEY") or os.environ.get("ELEVENLABS_API_KEY")),
                "masked_key": self._mask("ELEVENLABS_API_KEY") or self._mask_env("ELEVENLABS_API_KEY"),
            },
            "stability": {
                "configured": bool(self.config.get("STABILITY_API_KEY") or os.environ.get("STABILITY_API_KEY")),
                "masked_key": self._mask("STABILITY_API_KEY") or self._mask_env("STABILITY_API_KEY"),
            },
            "runway": {
                "configured": bool(self.config.get("RUNWAY_API_KEY") or os.environ.get("RUNWAY_API_KEY")),
                "masked_key": self._mask("RUNWAY_API_KEY") or self._mask_env("RUNWAY_API_KEY"),
            },
            "kling": {
                "configured": bool(self.config.get("KLING_API_KEY") or os.environ.get("KLING_API_KEY")),
                "masked_key": self._mask("KLING_API_KEY") or self._mask_env("KLING_API_KEY"),
            },
            "tts_provider": self.config.get("TTS_PROVIDER", "auto"),
            "image_provider": self.config.get("IMAGE_PROVIDER", "auto"),
            "video_provider": self.config.get("VIDEO_PROVIDER", "auto"),
        }

    def _mask_env(self, key: str) -> str:
        """Mask an API key from the live environment (not just .env file)."""
        val = os.environ.get(key, "")
        if not val or len(val) < 8:
            return ""
        return f"...{val[-4:]}"

    def update(self, updates: dict[str, str]) -> None:
        key_map = {
            "anthropic_key": "ANTHROPIC_API_KEY",
            "openai_key": "OPENAI_API_KEY",
            "gemini_key": "GEMINI_API_KEY",
            "model": "CLAUDE_MODEL",
            "provider": "AGENT_PROVIDER",
            # Media keys
            "elevenlabs_key": "ELEVENLABS_API_KEY",
            "stability_key": "STABILITY_API_KEY",
            "runway_key": "RUNWAY_API_KEY",
            "kling_key": "KLING_API_KEY",
            "tts_provider": "TTS_PROVIDER",
            "image_provider": "IMAGE_PROVIDER",
            "video_provider": "VIDEO_PROVIDER",
        }
        for ui_key, env_key in key_map.items():
            if ui_key in updates:
                val = updates[ui_key].strip()
                if val:
                    self.config[env_key] = val
                    os.environ[env_key] = val
                elif env_key in self.config:
                    del self.config[env_key]
                    os.environ.pop(env_key, None)
        self.save()


# ═══════════════════════════════════════════════════════════════════════
# Chat Session
# ═══════════════════════════════════════════════════════════════════════

class ChatSession:
    """Holds conversation history for one chat session."""

    def __init__(self, session_id: str, model: str = "claude-sonnet-4-6", provider: str = "first_party") -> None:
        self.session_id = session_id
        self.messages: list[dict[str, Any]] = []
        self.created_at = time.time()
        self.model = model
        self.provider = provider
        self.title = "New Chat"

    def add_user_message(self, content: str) -> None:
        self.messages.append({"role": "user", "content": content})
        if self.title == "New Chat" and len(self.messages) == 1:
            self.title = content[:60] + ("..." if len(content) > 60 else "")

    def add_assistant_message(self, content_blocks: list[dict[str, Any]]) -> None:
        self.messages.append({"role": "assistant", "content": content_blocks})

    def to_dict(self) -> dict[str, Any]:
        return {
            "session_id": self.session_id,
            "title": self.title,
            "model": self.model,
            "provider": self.provider,
            "created_at": self.created_at,
            "message_count": len(self.messages),
        }


# ═══════════════════════════════════════════════════════════════════════
# Web Server
# ═══════════════════════════════════════════════════════════════════════

MODEL_CATALOG_CLOUD = {
    "anthropic": [
        {"id": "claude-sonnet-4-6", "name": "Claude Sonnet 4.6", "context": "200K"},
        {"id": "claude-opus-4-6", "name": "Claude Opus 4.6", "context": "200K"},
        {"id": "claude-haiku-4-5-20251001", "name": "Claude Haiku 4.5", "context": "200K"},
    ],
    "openai": [
        {"id": "gpt-4o", "name": "GPT-4o", "context": "128K"},
        {"id": "gpt-4o-mini", "name": "GPT-4o Mini", "context": "128K"},
        {"id": "o3-mini", "name": "o3-mini", "context": "128K"},
    ],
    "gemini": [
        {"id": "gemini-2.0-flash", "name": "Gemini 2.0 Flash", "context": "1M"},
        {"id": "gemini-2.5-pro-preview-05-06", "name": "Gemini 2.5 Pro", "context": "1M"},
    ],
}


def _detect_ollama_models() -> list[dict[str, str]]:
    """Query Ollama for actually installed models."""
    import subprocess
    try:
        result = subprocess.run(
            ["ollama", "list"],
            capture_output=True, text=True, timeout=5,
        )
        if result.returncode != 0:
            return []
        models = []
        for line in result.stdout.strip().splitlines()[1:]:  # skip header
            parts = line.split()
            if not parts:
                continue
            model_name = parts[0]  # e.g. "gemma4:31b"
            size = parts[2] if len(parts) >= 3 else "?"
            models.append({
                "id": model_name,
                "name": f"{model_name} ({size})",
                "context": "local",
            })
        return models
    except (FileNotFoundError, subprocess.TimeoutExpired):
        return []


def _build_model_catalog() -> dict[str, list[dict[str, str]]]:
    """Build full model catalog with live Ollama detection."""
    catalog = dict(MODEL_CATALOG_CLOUD)
    ollama_models = _detect_ollama_models()
    catalog["ollama"] = ollama_models if ollama_models else [
        {"id": "llama3.1:latest", "name": "llama3.1 (install via: ollama pull llama3.1)", "context": "local"},
    ]
    return catalog

PROVIDER_FOR_MODEL = {
    "claude": "first_party",
    "gpt": "openai",
    "o1": "openai",
    "o3": "openai",
    "o4": "openai",
    "gemini": "gemini",
}


class WebServer:
    """HTTP + WebSocket server bridging the GUI to BackendRouter."""

    def __init__(
        self,
        host: str = "127.0.0.1",
        http_port: int = 8420,
        ws_port: int = 8421,
    ) -> None:
        self.host = host
        self.http_port = http_port
        self.ws_port = ws_port
        self.gui_dir = Path(__file__).resolve().parent
        self.project_dir = self.gui_dir.parent

        # Settings
        env_path = self.project_dir / ".env"
        self.settings = SettingsManager(env_path)

        # Backend
        self.backend = self._build_backend()

        # Sessions
        self.sessions: dict[str, ChatSession] = {}

    def _build_backend(self) -> Any:
        from agent_brain.api.client import (
            AnthropicBackend,
            BackendRouter,
            GeminiBackend,
            OllamaBackend,
            OpenAIBackend,
        )
        ak = os.environ.get("ANTHROPIC_API_KEY")
        ok_ = os.environ.get("OPENAI_API_KEY")
        gk = os.environ.get("GEMINI_API_KEY")
        return BackendRouter(
            anthropic=AnthropicBackend(api_key=ak) if ak else None,
            openai=OpenAIBackend(api_key=ok_) if ok_ else None,
            gemini=GeminiBackend(api_key=gk) if gk else None,
            ollama=OllamaBackend(),
        )

    # ── HTTP Server ─────────────────────────────────────────────────

    async def handle_http(self, reader: asyncio.StreamReader, writer: asyncio.StreamWriter) -> None:
        try:
            request_line = await asyncio.wait_for(reader.readline(), timeout=10)
            if not request_line:
                writer.close()
                return

            parts = request_line.decode("utf-8", errors="replace").strip().split(" ")
            if len(parts) < 2:
                writer.close()
                return

            method, raw_path = parts[0], parts[1]
            parsed = urlparse(raw_path)
            path = parsed.path

            # Read headers
            headers: dict[str, str] = {}
            content_length = 0
            while True:
                line = await asyncio.wait_for(reader.readline(), timeout=5)
                decoded = line.decode("utf-8", errors="replace").strip()
                if not decoded:
                    break
                if ":" in decoded:
                    hk, _, hv = decoded.partition(":")
                    headers[hk.strip().lower()] = hv.strip()
                    if hk.strip().lower() == "content-length":
                        content_length = int(hv.strip())

            # Read body
            body = b""
            if content_length > 0:
                body = await asyncio.wait_for(reader.readexactly(content_length), timeout=10)

            # Route
            if method == "GET" and path == "/":
                await self._serve_file(writer, self.gui_dir / "index.html", "text/html")
            elif method == "GET" and path == "/api/settings":
                await self._json_response(writer, self.settings.get_status())
            elif method == "POST" and path == "/api/settings":
                data = json.loads(body) if body else {}
                self.settings.update(data)
                self.backend = self._build_backend()
                await self._json_response(writer, {"ok": True, **self.settings.get_status()})
            elif method == "GET" and path == "/api/models":
                await self._json_response(writer, _build_model_catalog())
            elif method == "GET" and path == "/api/sessions":
                sessions = [s.to_dict() for s in self.sessions.values()]
                sessions.sort(key=lambda s: s["created_at"], reverse=True)
                await self._json_response(writer, sessions)
            elif method == "DELETE" and path.startswith("/api/sessions/"):
                sid = path.split("/")[-1]
                self.sessions.pop(sid, None)
                await self._json_response(writer, {"ok": True})
            # ── Media status API ──
            elif method == "GET" and path == "/api/media/status":
                await self._json_response(writer, self._check_media_deps())
            # ── Documents API ──
            elif method == "GET" and path == "/api/documents":
                category = parse_qs(parsed.query).get("category", [""])[0]
                await self._json_response(writer, self._list_documents(category))
            elif method == "POST" and path == "/api/documents/upload":
                await self._json_response(writer, self._handle_upload(headers, body))
            elif method == "DELETE" and path.startswith("/api/documents/"):
                file_path = parse_qs(parsed.query).get("path", [""])[0]
                await self._json_response(writer, self._delete_document(file_path))
            # ── CrewAI API ──
            elif method == "GET" and path == "/api/crews":
                from agent_brain.crewai_service import CrewAIService
                crews = CrewAIService._list_crew_templates()
                await self._json_response(writer, {"crews": crews})
            elif method == "GET" and path == "/api/crew":
                crew_name = parse_qs(parsed.query).get("name", [""])[0]
                from agent_brain.crewai_service import CrewAIService
                cfg = CrewAIService._load_crew_template(crew_name)
                if cfg:
                    await self._json_response(writer, cfg)
                else:
                    await self._json_response(writer, {"error": f"Crew '{crew_name}' not found"}, 404)
            elif method == "POST" and path == "/api/crew":
                data = json.loads(body) if body else {}
                await self._json_response(writer, self._save_crew_template(data))
            elif method == "POST" and path == "/api/crew/run":
                data = json.loads(body) if body else {}
                # Run crew via WebSocket chat so it streams — just return the config
                await self._json_response(writer, {"ok": True, "message": "Send via chat: 'Run the <name> crew'"})
            # ── File system API ──
            elif method == "GET" and path == "/api/roots":
                await self._json_response(writer, self._list_roots())
            elif method == "GET" and path == "/api/files":
                dir_path = parse_qs(parsed.query).get("path", [str(self.project_dir)])[0]
                await self._json_response(writer, self._list_dir(dir_path))
            elif method == "GET" and path == "/api/file":
                file_path = parse_qs(parsed.query).get("path", [""])[0]
                await self._json_response(writer, self._read_file(file_path))
            elif method == "POST" and path == "/api/file":
                data = json.loads(body) if body else {}
                await self._json_response(writer, self._write_file(data))
            else:
                await self._text_response(writer, "Not Found", 404)
        except Exception as e:
            logger.exception("HTTP handler error")
            try:
                await self._text_response(writer, f"Internal Server Error: {e}", 500)
            except Exception:
                pass
        finally:
            try:
                writer.close()
                await writer.wait_closed()
            except Exception:
                pass

    async def _serve_file(self, writer: asyncio.StreamWriter, path: Path, content_type: str) -> None:
        if not path.exists():
            await self._text_response(writer, "Not Found", 404)
            return
        data = path.read_bytes()
        header = (
            f"HTTP/1.1 200 OK\r\n"
            f"Content-Type: {content_type}; charset=utf-8\r\n"
            f"Content-Length: {len(data)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        )
        writer.write(header.encode() + data)
        await writer.drain()

    async def _json_response(self, writer: asyncio.StreamWriter, data: Any, status: int = 200) -> None:
        body = json.dumps(data, ensure_ascii=False).encode("utf-8")
        header = (
            f"HTTP/1.1 {status} OK\r\n"
            f"Content-Type: application/json; charset=utf-8\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Access-Control-Allow-Origin: *\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        )
        writer.write(header.encode() + body)
        await writer.drain()

    async def _text_response(self, writer: asyncio.StreamWriter, text: str, status: int = 200) -> None:
        body = text.encode("utf-8")
        header = (
            f"HTTP/1.1 {status} {'OK' if status == 200 else 'Error'}\r\n"
            f"Content-Type: text/plain; charset=utf-8\r\n"
            f"Content-Length: {len(body)}\r\n"
            f"Connection: close\r\n"
            f"\r\n"
        )
        writer.write(header.encode() + body)
        await writer.drain()

    # ── WebSocket Server ────────────────────────────────────────────

    async def handle_websocket(self, websocket: Any) -> None:
        """Handle one WebSocket connection. Routes by first message or path."""
        # Check if this is a terminal connection (first message = {"type":"terminal_init"})
        try:
            first_raw = await asyncio.wait_for(websocket.recv(), timeout=5)
            first_msg = json.loads(first_raw)
            if first_msg.get("type") == "terminal_init":
                await self._handle_terminal(websocket)
                return
            # Not terminal — process as chat message and continue
            await self._dispatch_ws_message(websocket, first_msg)
        except asyncio.TimeoutError:
            return
        except Exception as e:
            logger.debug("WebSocket init error: %s", e)
            return

        try:
            async for raw_msg in websocket:
                try:
                    msg = json.loads(raw_msg)
                except json.JSONDecodeError:
                    await websocket.send(json.dumps({"type": "error", "message": "Invalid JSON"}))
                    continue
                await self._dispatch_ws_message(websocket, msg)

        except Exception as e:
            logger.debug("WebSocket closed: %s", e)

    async def _dispatch_ws_message(self, websocket: Any, msg: dict[str, Any]) -> None:
        """Dispatch a single WebSocket message by type."""
        msg_type = msg.get("type", "")

        if msg_type == "chat_message":
            await self._handle_chat(websocket, msg)
        elif msg_type == "new_session":
            sid = str(uuid.uuid4())[:8]
            model = msg.get("model", self.settings.config.get("CLAUDE_MODEL", "claude-sonnet-4-6"))
            provider = msg.get("provider") or self._provider_for_model(model)
            self.sessions[sid] = ChatSession(sid, model=model, provider=provider)
            await websocket.send(json.dumps({"type": "session_created", "session_id": sid}))
        elif msg_type == "ping":
            await websocket.send(json.dumps({"type": "pong"}))
        else:
            await websocket.send(json.dumps({"type": "error", "message": f"Unknown type: {msg_type}"}))

    async def _handle_chat(self, websocket: Any, msg: dict[str, Any]) -> None:
        from agent_brain.api.client import StreamRequest

        content = str(msg.get("content", "")).strip()
        if not content:
            await websocket.send(json.dumps({"type": "error", "message": "Empty message"}))
            return

        # Get or create session
        sid = msg.get("session_id")
        if not sid or sid not in self.sessions:
            sid = str(uuid.uuid4())[:8]
            model = msg.get("model", self.settings.config.get("CLAUDE_MODEL", "claude-sonnet-4-6"))
            provider = msg.get("provider") or self._provider_for_model(model)
            self.sessions[sid] = ChatSession(sid, model=model, provider=provider)
            await websocket.send(json.dumps({"type": "session_created", "session_id": sid}))

        session = self.sessions[sid]

        # Update model/provider if specified
        if msg.get("model"):
            session.model = msg["model"]
        if msg.get("provider"):
            session.provider = msg["provider"]

        session.add_user_message(content)

        # Build system prompt
        system_prompt = (
            "You are Centaur Psicode, a powerful AI coding assistant. "
            "You help users write, debug, and understand code. "
            "Be concise, precise, and helpful."
        )

        # Try to load memory context
        try:
            from agent_brain.memory.layers import MemoryStack
            from agent_brain.memory.memdir import MemoryStore
            store = MemoryStore()
            stack = MemoryStack(store=store, vector=store.vector_store)
            wake = stack.wake_up(topic=content[:200])
            if wake:
                system_prompt += f"\n\n{wake}"
        except Exception:
            pass  # Memory not available, continue without

        request_id = str(uuid.uuid4())
        request = StreamRequest(
            request_id=request_id,
            model=session.model,
            messages=session.messages,
            system_prompt=system_prompt,
            max_output_tokens=8192,
            provider=session.provider,
        )

        # Stream from BackendRouter
        accumulated_text = ""
        content_blocks: list[dict[str, Any]] = []

        try:
            async for event in self.backend.stream_message(request):
                event_type = event.get("type", "")

                if event_type == "text_delta":
                    delta = event.get("delta", "")
                    accumulated_text += delta
                    await websocket.send(json.dumps({
                        "type": "text_delta",
                        "session_id": sid,
                        "delta": delta,
                    }))

                elif event_type == "tool_use":
                    tool_data = {
                        "type": "tool_use",
                        "session_id": sid,
                        "tool_call_id": event.get("tool_call_id", ""),
                        "name": event.get("name", ""),
                        "input": event.get("input", {}),
                    }
                    content_blocks.append({
                        "type": "tool_use",
                        "id": event.get("tool_call_id", ""),
                        "name": event.get("name", ""),
                        "input": event.get("input", {}),
                    })
                    await websocket.send(json.dumps(tool_data))

                elif event_type == "message_done":
                    usage = event.get("usage", {})
                    stop_reason = event.get("stop_reason", "end_turn")

                    # Build content blocks for history
                    if accumulated_text:
                        content_blocks.insert(0, {"type": "text", "text": accumulated_text})
                    if content_blocks:
                        session.add_assistant_message(content_blocks)

                    await websocket.send(json.dumps({
                        "type": "message_done",
                        "session_id": sid,
                        "usage": usage,
                        "stop_reason": stop_reason,
                    }))
                    break

        except Exception as e:
            error_msg = str(e)
            logger.error("Chat stream error: %s", error_msg)
            await websocket.send(json.dumps({
                "type": "error",
                "session_id": sid,
                "message": error_msg,
            }))
            # Still save partial response
            if accumulated_text:
                content_blocks.insert(0, {"type": "text", "text": accumulated_text})
                session.add_assistant_message(content_blocks)

    # ── File System Helpers ──────────────────────────────────────────

    HIDDEN_DIRS = {".git", "node_modules", "__pycache__", ".venv", "target", ".mypy_cache", ".pytest_cache"}
    TEXT_EXTS = {
        ".py", ".rs", ".ts", ".tsx", ".js", ".jsx", ".json", ".toml", ".yaml", ".yml",
        ".md", ".txt", ".html", ".css", ".sh", ".sql", ".env", ".cfg", ".ini", ".lock",
        ".h", ".c", ".cpp", ".go", ".java", ".rb", ".swift", ".kt", ".r", ".lua",
        ".dockerfile", ".makefile", ".gitignore", ".editorconfig",
    }

    def _list_dir(self, dir_path: str) -> dict[str, Any]:
        """List directory contents, sorted: folders first, then files."""
        p = Path(dir_path).resolve()
        if not p.is_dir():
            return {"error": f"Not a directory: {dir_path}", "items": []}

        items = []
        try:
            for child in sorted(p.iterdir(), key=lambda x: (not x.is_dir(), x.name.lower())):
                if child.name in self.HIDDEN_DIRS:
                    continue
                # Skip hidden files starting with . (but show .env, .gitignore etc)
                if child.name.startswith(".") and child.is_dir() and child.name not in {".claude", ".agent"}:
                    continue
                try:
                    items.append({
                        "name": child.name,
                        "path": str(child),
                        "is_dir": child.is_dir(),
                        "size": child.stat().st_size if child.is_file() else 0,
                    })
                except (PermissionError, OSError):
                    continue
        except PermissionError:
            return {"error": "Permission denied", "items": []}
        return {"path": str(p), "parent": str(p.parent), "items": items}

    def _list_roots(self) -> dict[str, Any]:
        """List useful starting directories for the file browser."""
        home = Path.home()
        roots = [
            {"name": "Project", "path": str(self.project_dir), "icon": "project"},
            {"name": "Home", "path": str(home), "icon": "home"},
            {"name": "Desktop", "path": str(home / "Desktop"), "icon": "desktop"},
        ]
        # Add recent parent dirs
        for name in ["Documents", "Downloads"]:
            d = home / name
            if d.is_dir():
                roots.append({"name": name, "path": str(d), "icon": "folder"})
        return {"roots": roots}

    def _read_file(self, file_path: str) -> dict[str, Any]:
        """Read a text file, return content + language."""
        p = Path(file_path).resolve()
        if not p.is_file():
            return {"error": f"Not a file: {file_path}"}

        ext = p.suffix.lower()
        if ext not in self.TEXT_EXTS and p.name.lower() not in {"makefile", "dockerfile", "gemfile", "rakefile"}:
            return {"error": f"Binary or unsupported file type: {ext}"}
        try:
            content = p.read_text(encoding="utf-8", errors="replace")
            lang_map = {
                ".py": "python", ".rs": "rust", ".ts": "typescript", ".tsx": "typescript",
                ".js": "javascript", ".jsx": "javascript", ".json": "json", ".toml": "toml",
                ".yaml": "yaml", ".yml": "yaml", ".md": "markdown", ".html": "html",
                ".css": "css", ".sh": "bash", ".sql": "sql", ".go": "go", ".java": "java",
                ".c": "c", ".cpp": "cpp", ".h": "c", ".swift": "swift", ".rb": "ruby",
            }
            return {
                "path": str(p),
                "name": p.name,
                "content": content,
                "language": lang_map.get(ext, "plaintext"),
                "size": len(content),
            }
        except Exception as e:
            return {"error": str(e)}

    def _write_file(self, data: dict[str, Any]) -> dict[str, Any]:
        """Write content to a file."""
        file_path = data.get("path", "")
        content = data.get("content", "")
        p = Path(file_path).resolve()
        try:
            p.write_text(content, encoding="utf-8")
            return {"ok": True, "path": str(p), "size": len(content)}
        except Exception as e:
            return {"error": str(e)}

    # ── Terminal ───────────────────────────────────────────────────

    async def _handle_terminal(self, websocket: Any) -> None:
        """Handle a terminal WebSocket — run commands one at a time.

        Each command spawns a subprocess, streams output back, then returns
        the exit code. This is simpler and more reliable than a PTY.
        """
        cwd = str(self.project_dir)
        shell = os.environ.get("SHELL", "/bin/zsh")

        # Send initial prompt
        await websocket.send(json.dumps({
            "type": "output",
            "data": f"Centaur Psicode Terminal\nWorking directory: {cwd}\n\n",
        }))

        try:
            async for raw_msg in websocket:
                try:
                    msg = json.loads(raw_msg)
                except json.JSONDecodeError:
                    continue

                if msg.get("type") == "command":
                    cmd = msg.get("data", "").strip()
                    if not cmd:
                        continue

                    # Handle cd specially — update cwd
                    if cmd.startswith("cd "):
                        target = cmd[3:].strip().strip('"').strip("'")
                        new_dir = Path(cwd).expanduser() / target if not target.startswith("/") else Path(target)
                        new_dir = new_dir.expanduser().resolve()
                        if new_dir.is_dir():
                            cwd = str(new_dir)
                            await websocket.send(json.dumps({
                                "type": "output",
                                "data": f"Changed directory to {cwd}\n",
                            }))
                        else:
                            await websocket.send(json.dumps({
                                "type": "output",
                                "data": f"cd: no such directory: {target}\n",
                            }))
                        await websocket.send(json.dumps({"type": "prompt", "cwd": cwd}))
                        continue

                    # Run command as subprocess
                    try:
                        process = await asyncio.create_subprocess_shell(
                            cmd,
                            stdout=asyncio.subprocess.PIPE,
                            stderr=asyncio.subprocess.STDOUT,
                            cwd=cwd,
                            env={**os.environ, "TERM": "dumb", "NO_COLOR": "1"},
                        )
                        assert process.stdout is not None

                        # Stream output in real time
                        while True:
                            chunk = await asyncio.wait_for(
                                process.stdout.read(4096), timeout=30
                            )
                            if not chunk:
                                break
                            await websocket.send(json.dumps({
                                "type": "output",
                                "data": chunk.decode("utf-8", errors="replace"),
                            }))

                        await process.wait()
                        exit_code = process.returncode

                        if exit_code != 0:
                            await websocket.send(json.dumps({
                                "type": "output",
                                "data": f"\n[exit code: {exit_code}]\n",
                            }))

                    except asyncio.TimeoutError:
                        await websocket.send(json.dumps({
                            "type": "output",
                            "data": "\n[command timed out after 30s]\n",
                        }))
                        if process.returncode is None:
                            process.kill()
                    except Exception as e:
                        await websocket.send(json.dumps({
                            "type": "output",
                            "data": f"\n[error: {e}]\n",
                        }))

                    # Send prompt ready signal
                    await websocket.send(json.dumps({"type": "prompt", "cwd": cwd}))

        except Exception:
            pass

    # ── Media Dependency Check ──────────────────────────────────

    @staticmethod
    def _check_media_deps() -> dict[str, Any]:
        """Check which media packages and tools are installed."""
        deps: dict[str, Any] = {}

        # Python packages
        for pkg, label in [
            ("edge_tts", "Edge TTS (free)"),
            ("elevenlabs", "ElevenLabs TTS"),
            ("openai", "OpenAI (TTS + DALL-E)"),
            ("sounddevice", "Microphone capture"),
        ]:
            try:
                __import__(pkg)
                deps[pkg] = {"installed": True, "label": label}
            except ImportError:
                deps[pkg] = {"installed": False, "label": label}

        # ffmpeg
        import shutil
        deps["ffmpeg"] = {
            "installed": shutil.which("ffmpeg") is not None,
            "label": "FFmpeg (video composition)",
        }

        return {"dependencies": deps}

    # ── Document Library Helpers ─────────────────────────────────

    DOCS_DIR_NAME = "documents"
    DOC_CATEGORIES = ["papers", "articles", "transcripts", "downloads"]
    DOC_EXTENSIONS = {
        ".md", ".txt", ".pdf", ".json", ".csv", ".html", ".xml",
        ".py", ".rs", ".ts", ".js", ".yaml", ".yml", ".toml",
        ".tex", ".bib", ".rtf", ".org",
    }

    @property
    def docs_dir(self) -> Path:
        return self.project_dir / self.DOCS_DIR_NAME

    def _list_documents(self, category: str = "") -> dict[str, Any]:
        """List documents, optionally filtered by category (subfolder)."""
        base = self.docs_dir
        if not base.is_dir():
            base.mkdir(parents=True, exist_ok=True)

        # List categories with counts
        categories = []
        for cat in self.DOC_CATEGORIES:
            cat_dir = base / cat
            if not cat_dir.is_dir():
                cat_dir.mkdir(exist_ok=True)
            count = sum(1 for f in cat_dir.iterdir() if f.is_file() and not f.name.startswith("."))
            categories.append({"name": cat, "count": count})

        # List files in the selected category (or all)
        files = []
        scan_dirs = [base / category] if category and (base / category).is_dir() else [base / c for c in self.DOC_CATEGORIES]

        for d in scan_dirs:
            if not d.is_dir():
                continue
            for f in sorted(d.iterdir(), key=lambda x: x.stat().st_mtime, reverse=True):
                if f.is_file() and not f.name.startswith("."):
                    stat = f.stat()
                    files.append({
                        "name": f.name,
                        "path": str(f),
                        "category": d.name,
                        "size": stat.st_size,
                        "size_human": self._human_size(stat.st_size),
                        "modified": int(stat.st_mtime),
                        "ext": f.suffix.lower(),
                        "can_ingest": f.suffix.lower() in {".md", ".txt", ".html", ".json", ".yaml", ".yml"},
                        "can_view": f.suffix.lower() in self.TEXT_EXTS,
                    })

        return {"categories": categories, "files": files, "docs_dir": str(base)}

    def _handle_upload(self, headers: dict[str, str], body: bytes) -> dict[str, Any]:
        """Handle file upload — save to documents folder."""
        try:
            data = json.loads(body)
            filename = data.get("filename", "").strip()
            content = data.get("content", "")
            category = data.get("category", "papers")

            if not filename:
                return {"error": "Filename is required"}
            if category not in self.DOC_CATEGORIES:
                category = "papers"

            target = self.docs_dir / category / filename
            target.parent.mkdir(parents=True, exist_ok=True)

            if data.get("encoding") == "base64":
                import base64
                target.write_bytes(base64.b64decode(content))
            else:
                target.write_text(content, encoding="utf-8")

            return {"ok": True, "path": str(target), "name": filename, "category": category}
        except Exception as e:
            return {"error": str(e)}

    def _delete_document(self, file_path: str) -> dict[str, Any]:
        """Delete a document file."""
        p = Path(file_path).resolve()
        try:
            p.relative_to(self.docs_dir.resolve())
        except ValueError:
            return {"error": "File is not in the documents directory"}
        if p.is_file():
            p.unlink()
            return {"ok": True}
        return {"error": "File not found"}

    @staticmethod
    def _human_size(size: int) -> str:
        for unit in ("B", "KB", "MB", "GB"):
            if size < 1024:
                return f"{size:.0f} {unit}" if unit == "B" else f"{size:.1f} {unit}"
            size /= 1024
        return f"{size:.1f} TB"

    def _save_crew_template(self, data: dict[str, Any]) -> dict[str, Any]:
        """Save a crew config as a YAML template."""
        import yaml
        name = data.get("name", "").strip().lower().replace(" ", "_")
        if not name:
            return {"error": "Crew name is required"}
        crews_dir = Path(__file__).resolve().parent.parent / "agent-brain" / "agent_brain" / "crews"
        crews_dir.mkdir(parents=True, exist_ok=True)
        path = crews_dir / f"{name}.yaml"
        try:
            path.write_text(yaml.dump(data, default_flow_style=False, sort_keys=False), encoding="utf-8")
            return {"ok": True, "path": str(path), "name": name}
        except Exception as e:
            return {"error": str(e)}

    def _provider_for_model(self, model: str) -> str:
        model_lower = model.lower()
        for prefix, provider in PROVIDER_FOR_MODEL.items():
            if model_lower.startswith(prefix):
                return provider
        return "ollama"

    # ── Lifecycle ───────────────────────────────────────────────────

    async def start_and_serve(self) -> None:
        import websockets

        self._http_server = await asyncio.start_server(
            self.handle_http, self.host, self.http_port,
        )
        self._ws_server = await websockets.serve(
            self.handle_websocket, self.host, self.ws_port,
        )
        self._stop_event = asyncio.Event()

        print(f"\n  Centaur Psicode Web GUI", file=sys.stderr)
        print(f"  ─────────────────────────────────", file=sys.stderr)
        print(f"  GUI:       http://{self.host}:{self.http_port}", file=sys.stderr)
        print(f"  WebSocket: ws://{self.host}:{self.ws_port}", file=sys.stderr)
        status = self.settings.get_status()
        providers = []
        if status["anthropic"]["configured"]:
            providers.append("Anthropic")
        if status["openai"]["configured"]:
            providers.append("OpenAI")
        if status["gemini"]["configured"]:
            providers.append("Gemini")
        providers.append("Ollama (local)")
        print(f"  Providers: {', '.join(providers)}", file=sys.stderr)
        print(f"  Model:     {status['selected_model']}", file=sys.stderr)
        print(f"\n  Open http://{self.host}:{self.http_port} in your browser.\n", file=sys.stderr)
        print(f"  Press Ctrl+C to stop.\n", file=sys.stderr)

        # Wait until stop_event is set (by shutdown handler)
        await self._stop_event.wait()

        # Clean up
        self._http_server.close()
        await self._http_server.wait_closed()
        self._ws_server.close()
        await self._ws_server.wait_closed()
        print("Shutdown complete.", file=sys.stderr)

    def request_stop(self) -> None:
        """Signal the server to stop. Safe to call from signal handler."""
        if hasattr(self, "_stop_event") and self._stop_event is not None:
            self._stop_event.set()


# ═══════════════════════════════════════════════════════════════════════
# Entry Point
# ═══════════════════════════════════════════════════════════════════════

def main() -> None:
    logging.basicConfig(level=logging.INFO, format="%(levelname)s: %(message)s")

    # Load .env
    for env_path in [".env", "../.env", str(Path(__file__).resolve().parent.parent / ".env")]:
        if os.path.isfile(env_path):
            with open(env_path) as f:
                for line in f:
                    line = line.strip()
                    if line and not line.startswith("#") and "=" in line:
                        key, _, value = line.partition("=")
                        os.environ.setdefault(key.strip(), value.strip().strip('"').strip("'"))
            break

    server = WebServer(host="127.0.0.1", http_port=8420, ws_port=8421)

    loop = asyncio.new_event_loop()

    def _shutdown(sig: int, frame: Any) -> None:
        print("\nShutting down...", file=sys.stderr)
        loop.call_soon_threadsafe(server.request_stop)

    signal.signal(signal.SIGINT, _shutdown)
    signal.signal(signal.SIGTERM, _shutdown)

    try:
        loop.run_until_complete(server.start_and_serve())
    except KeyboardInterrupt:
        server.request_stop()
        # Give the loop a moment to process the stop
        loop.run_until_complete(asyncio.sleep(0.1))
    finally:
        loop.close()


if __name__ == "__main__":
    main()
