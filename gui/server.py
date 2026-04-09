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

        # Build the full IpcServer as a service container
        # This gives us ALL agent services: memory, wiki, dream, skills, crewai, compact
        self._init_services()

        # Sessions
        self.sessions: dict[str, ChatSession] = {}

        # Turn counter for dream triggers
        self._turn_count = 0

    def _init_services(self) -> None:
        """Initialize the full IpcServer as service container — gives us ALL features."""
        from agent_brain.api.client import (
            AnthropicBackend,
            BackendRouter,
            GeminiBackend,
            OllamaBackend,
            OpenAIBackend,
        )
        from agent_brain.ipc_server import IpcServer

        ak = os.environ.get("ANTHROPIC_API_KEY")
        ok_ = os.environ.get("OPENAI_API_KEY")
        gk = os.environ.get("GEMINI_API_KEY")
        backend = BackendRouter(
            anthropic=AnthropicBackend(api_key=ak) if ak else None,
            openai=OpenAIBackend(api_key=ok_) if ok_ else None,
            gemini=GeminiBackend(api_key=gk) if gk else None,
            ollama=OllamaBackend(),
        )

        # Create IpcServer but DON'T start the socket listener.
        # We just use it as a container for all services.
        # CRITICAL: memory_root_dir must point to the REAL persistent location,
        # not /tmp/ (which is what socket_path.parent would default to).
        self.ipc = IpcServer(
            socket_path="/tmp/gui-ipc-unused.sock",
            backend=backend,
            memory_root_dir=str(Path.home() / ".agent" / "memory"),
            skill_root_dir=str(self.project_dir),
        )
        # Shorthand for the most-used service
        self.backend = self.ipc.backend

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
                self._init_services()  # Rebuild all services with new keys
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

    # ── System prompt construction ─────────────────────────────────

    def _build_system_prompt(self, user_topic: str = "") -> str:
        """Build a rich system prompt with context, CLAUDE.md, memory, and environment."""
        parts: list[str] = []

        # Core identity
        parts.append(
            "You are Centaur Psicode, a powerful AI coding assistant with persistent memory, "
            "a wiki knowledge base, and tool execution capabilities. "
            "You can read/write files, run shell commands, search the web, manage memory, "
            "ingest documents into a wiki, and orchestrate multi-agent crews. "
            "Be concise, precise, and helpful. Use tools to accomplish tasks — don't just describe what to do, actually do it."
        )

        # Environment context
        import platform
        cwd = str(self.project_dir)
        parts.append(f"\n## Environment\n- Working directory: {cwd}\n- Platform: {platform.system()} {platform.machine()}\n- Shell: {os.environ.get('SHELL', '/bin/zsh')}\n- Date: {time.strftime('%Y-%m-%d')}")

        # Git context
        try:
            import subprocess
            branch = subprocess.run(["git", "branch", "--show-current"], capture_output=True, text=True, cwd=cwd, timeout=3).stdout.strip()
            status = subprocess.run(["git", "status", "--short"], capture_output=True, text=True, cwd=cwd, timeout=3).stdout.strip()
            if branch:
                git_info = f"\n## Git\n- Branch: {branch}"
                if status:
                    git_info += f"\n- Status:\n```\n{status[:500]}\n```"
                parts.append(git_info)
        except Exception:
            pass

        # CLAUDE.md files
        for name in ["CLAUDE.md", ".claude/CLAUDE.md"]:
            p = self.project_dir / name
            if p.is_file():
                content = p.read_text(encoding="utf-8", errors="replace")[:3000]
                parts.append(f"\n## Project Instructions (from {name})\n{content}")
                break

        # WIKI_SCHEMA.md
        for name in ["WIKI_SCHEMA.md", ".claude/WIKI_SCHEMA.md"]:
            p = self.project_dir / name
            if p.is_file():
                content = p.read_text(encoding="utf-8", errors="replace")[:2000]
                parts.append(f"\n## Wiki Schema\n{content}")
                break

        # Memory context (L0 + L1 + L2) via IpcServer's memory service
        try:
            from agent_brain.memory.layers import MemoryStack
            store = self.ipc.memory_service.store
            stack = MemoryStack(store=store, vector=store.vector_store)
            wake = stack.wake_up(topic=user_topic[:200] if user_topic else None)
            if wake:
                parts.append(f"\n{wake}")
        except Exception:
            pass

        return "\n".join(parts)

    async def _extract_memories_from_turn(self, user_msg: str, assistant_msg: str) -> None:
        """M1: Extract durable memories from the conversation turn (fire-and-forget)."""
        try:
            from agent_brain.memory.extract import MemoryExtractor
            store = self.ipc.memory_service.store
            extractor = MemoryExtractor()
            messages = [
                {"role": "user", "content": user_msg},
                {"role": "assistant", "content": assistant_msg},
            ]
            result = extractor.extract(messages, store=store)
            if hasattr(result, "candidates") and result.candidates:
                for c in result.candidates:
                    if c.confidence > 0.6:
                        store.save_memory(
                            title=c.title,
                            body=c.body,
                            memory_type=c.memory_type,
                            description=c.description or c.body[:80],
                        )
                logger.info("M1: extracted %d memory candidates", len(result.candidates))
        except Exception as e:
            logger.debug("Memory extraction skipped: %s", e)

    async def _maybe_dream(self) -> None:
        """SM4: Trigger dream consolidation after enough turns."""
        if self._turn_count > 0 and self._turn_count % 20 == 0:
            try:
                from agent_brain.ipc_types import MemoryRequest
                req = MemoryRequest(
                    request_id=str(uuid.uuid4()),
                    action="dream_consolidate",
                    payload={
                        "memory_dir": str(self.ipc.memory_service.store.root_dir),
                        "transcript_dir": str(Path.home() / ".agent" / "transcripts"),
                    },
                )
                resp = await self.ipc.dream_service.handle(req)
                if resp.ok:
                    logger.info("Dream consolidation completed: %s", resp.payload.get("summary", ""))
            except Exception as e:
                logger.debug("Dream consolidation skipped: %s", e)

    # ── Tool definitions sent to the LLM ──────────────────────────

    TOOL_DEFINITIONS = [
        # ── Core file & shell tools ────────────────────────────────
        {"name": "Bash", "description": "Execute a shell command and return its output. Use for: running scripts, git, ls, pwd, installing packages, etc.",
         "input_schema": {"type": "object", "properties": {"command": {"type": "string", "description": "The command to execute"}}, "required": ["command"]}},
        {"name": "FileRead", "description": "Read a file from disk. Returns content with line numbers. Supports text files and PDFs.",
         "input_schema": {"type": "object", "properties": {"file_path": {"type": "string", "description": "Absolute path to the file"}, "offset": {"type": "integer", "description": "Line to start from (0-based)"}, "limit": {"type": "integer", "description": "Max lines to read"}}, "required": ["file_path"]}},
        {"name": "FileWrite", "description": "Create or overwrite a file with the given content.",
         "input_schema": {"type": "object", "properties": {"file_path": {"type": "string", "description": "Absolute path"}, "content": {"type": "string", "description": "File content to write"}}, "required": ["file_path", "content"]}},
        {"name": "FileEdit", "description": "Replace a specific string in a file. The old_string must match exactly and be unique.",
         "input_schema": {"type": "object", "properties": {"file_path": {"type": "string", "description": "Absolute path"}, "old_string": {"type": "string", "description": "Exact text to find"}, "new_string": {"type": "string", "description": "Replacement text"}}, "required": ["file_path", "old_string", "new_string"]}},
        {"name": "Glob", "description": "Find files matching a glob pattern (e.g., '**/*.py'). Returns file paths.",
         "input_schema": {"type": "object", "properties": {"pattern": {"type": "string", "description": "Glob pattern"}, "path": {"type": "string", "description": "Directory to search in"}}, "required": ["pattern"]}},
        {"name": "Grep", "description": "Search file contents for a regex pattern. Returns matching lines with file paths and line numbers.",
         "input_schema": {"type": "object", "properties": {"pattern": {"type": "string", "description": "Regex pattern"}, "path": {"type": "string", "description": "File or directory to search"}, "glob": {"type": "string", "description": "Filter files by glob"}}, "required": ["pattern"]}},
        # ── Memory & Wiki tools ────────────────────────────────────
        {"name": "MemoryRecall", "description": "Search the persistent memory/wiki for relevant information. Use when the user asks about past conversations, stored knowledge, or project context.",
         "input_schema": {"type": "object", "properties": {"query": {"type": "string", "description": "What to search for"}, "limit": {"type": "integer", "description": "Max results (default 5)"}}, "required": ["query"]}},
        {"name": "MemorySave", "description": "Save important information to persistent memory for future sessions. Use for: user preferences, decisions, facts, project context.",
         "input_schema": {"type": "object", "properties": {"title": {"type": "string"}, "body": {"type": "string"}, "memory_type": {"type": "string", "enum": ["user", "feedback", "project", "reference"]}, "tags": {"type": "array", "items": {"type": "string"}}}, "required": ["title", "body", "memory_type"]}},
        {"name": "WikiIngest", "description": "Ingest a document into the wiki knowledge base. Extracts entities, concepts, summaries, and cross-references.",
         "input_schema": {"type": "object", "properties": {"content": {"type": "string", "description": "The document text to ingest"}, "title": {"type": "string", "description": "Title for the source"}, "source_type": {"type": "string", "enum": ["file", "web", "manual"]}, "tags": {"type": "array", "items": {"type": "string"}}}, "required": ["content", "title"]}},
        {"name": "WikiQuery", "description": "Ask a question against the wiki knowledge base. Searches for relevant pages and synthesizes an answer.",
         "input_schema": {"type": "object", "properties": {"question": {"type": "string"}, "save_as_page": {"type": "boolean", "description": "Save the answer as a wiki page"}}, "required": ["question"]}},
        {"name": "WikiLint", "description": "Run a health check on the wiki: find orphan pages, broken references, stale content, missing pages.",
         "input_schema": {"type": "object", "properties": {}, "required": []}},
        # ── Web tools ──────────────────────────────────────────────
        {"name": "WebFetch", "description": "Fetch a web page URL and return its text content. Use for reading articles, documentation, APIs.",
         "input_schema": {"type": "object", "properties": {"url": {"type": "string", "description": "URL to fetch"}}, "required": ["url"]}},
        {"name": "WebSearch", "description": "Search the web and return results. Use when you need current information not in your training data.",
         "input_schema": {"type": "object", "properties": {"query": {"type": "string", "description": "Search query"}}, "required": ["query"]}},
        # ── CrewAI ─────────────────────────────────────────────────
        {"name": "CrewAI", "description": "Run a multi-agent CrewAI crew. Use a saved template by name, or provide inline crew_config.",
         "input_schema": {"type": "object", "properties": {"crew_name": {"type": "string", "description": "Name of saved crew template"}, "crew_config": {"type": "object", "description": "Inline crew config (agents + tasks)"}, "inputs": {"type": "object", "description": "Input variables for the crew"}}}},
        # ── Knowledge Graph ────────────────────────────────────────
        {"name": "KGQuery", "description": "Query the knowledge graph for entity relationships. Supports temporal filtering with as_of date.",
         "input_schema": {"type": "object", "properties": {"entity": {"type": "string", "description": "Entity name to query"}, "as_of": {"type": "string", "description": "Date filter (YYYY-MM-DD)"}, "direction": {"type": "string", "enum": ["in", "out", "both"]}}, "required": ["entity"]}},
        {"name": "KGAdd", "description": "Add a fact triple to the knowledge graph (subject, predicate, object).",
         "input_schema": {"type": "object", "properties": {"subject": {"type": "string"}, "predicate": {"type": "string"}, "object": {"type": "string"}, "valid_from": {"type": "string"}, "confidence": {"type": "number"}}, "required": ["subject", "predicate", "object"]}},
        {"name": "KGTimeline", "description": "Get chronological timeline of facts about an entity.",
         "input_schema": {"type": "object", "properties": {"entity": {"type": "string", "description": "Entity name (optional — omit for all)"}}}},
        # ── Skills ─────────────────────────────────────────────────
        {"name": "RunSkill", "description": "Execute a bundled skill by name: commit, review, debug, wiki-init, wiki-lint, remember, simplify, ultraplan, etc.",
         "input_schema": {"type": "object", "properties": {"skill_name": {"type": "string", "description": "Skill name (e.g., commit, review, debug, wiki-init)"}, "arguments": {"type": "object", "description": "Arguments for the skill"}}, "required": ["skill_name"]}},
    ]

    # ── Tool execution (Python-side) ───────────────────────────────

    async def _execute_tool(self, name: str, input_data: dict[str, Any]) -> str:
        """Execute a tool and return the result as text."""
        try:
            if name == "Bash":
                return await self._tool_bash(input_data)
            elif name == "FileRead":
                return self._tool_file_read(input_data)
            elif name == "FileWrite":
                return self._tool_file_write(input_data)
            elif name == "FileEdit":
                return self._tool_file_edit(input_data)
            elif name == "Glob":
                return self._tool_glob(input_data)
            elif name == "Grep":
                return self._tool_grep(input_data)
            elif name == "MemoryRecall":
                return self._tool_memory_recall(input_data)
            elif name == "MemorySave":
                return self._tool_memory_save(input_data)
            elif name == "WikiIngest":
                return await self._tool_wiki_ingest(input_data)
            elif name == "WikiQuery":
                return await self._tool_wiki_query(input_data)
            elif name == "WikiLint":
                return await self._tool_wiki_lint(input_data)
            elif name == "WebFetch":
                return await self._tool_web_fetch(input_data)
            elif name == "WebSearch":
                return await self._tool_web_search(input_data)
            elif name == "CrewAI":
                return await self._tool_crewai(input_data)
            elif name == "KGQuery":
                return self._tool_kg_query(input_data)
            elif name == "KGAdd":
                return self._tool_kg_add(input_data)
            elif name == "KGTimeline":
                return self._tool_kg_timeline(input_data)
            elif name == "RunSkill":
                return await self._tool_run_skill(input_data)
            else:
                return f"Tool '{name}' is not available in the GUI."
        except Exception as e:
            return f"Tool error: {e}"

    async def _tool_bash(self, inp: dict[str, Any]) -> str:
        cmd = inp.get("command", "")
        if not cmd:
            return "Error: no command provided"
        try:
            proc = await asyncio.create_subprocess_shell(
                cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.STDOUT,
                cwd=str(self.project_dir),
                env={**os.environ, "TERM": "dumb", "NO_COLOR": "1"},
            )
            stdout, _ = await asyncio.wait_for(proc.communicate(), timeout=30)
            output = stdout.decode("utf-8", errors="replace")
            if len(output) > 20000:
                output = output[:20000] + "\n... [truncated]"
            if proc.returncode != 0:
                output += f"\n[exit code: {proc.returncode}]"
            return output or "(no output)"
        except asyncio.TimeoutError:
            return "[command timed out after 30s]"

    def _tool_file_read(self, inp: dict[str, Any]) -> str:
        file_path = inp.get("file_path", "")
        p = Path(file_path)
        if not p.is_file():
            return f"Error: file not found: {file_path}"
        try:
            lines = p.read_text(encoding="utf-8", errors="replace").splitlines()
            offset = int(inp.get("offset", 0))
            limit = int(inp.get("limit", 2000))
            selected = lines[offset:offset + limit]
            numbered = [f"{i + offset + 1}\t{line}" for i, line in enumerate(selected)]
            return "\n".join(numbered) or "(empty file)"
        except Exception as e:
            return f"Error reading file: {e}"

    def _tool_file_write(self, inp: dict[str, Any]) -> str:
        file_path = inp.get("file_path", "")
        content = inp.get("content", "")
        p = Path(file_path)
        try:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_text(content, encoding="utf-8")
            return f"File written: {file_path} ({len(content)} bytes)"
        except Exception as e:
            return f"Error writing file: {e}"

    def _tool_file_edit(self, inp: dict[str, Any]) -> str:
        file_path = inp.get("file_path", "")
        old_string = inp.get("old_string", "")
        new_string = inp.get("new_string", "")
        p = Path(file_path)
        if not p.is_file():
            return f"Error: file not found: {file_path}"
        try:
            text = p.read_text(encoding="utf-8")
            if old_string not in text:
                return f"Error: old_string not found in {file_path}"
            count = text.count(old_string)
            if count > 1:
                return f"Error: old_string matches {count} times (must be unique)"
            new_text = text.replace(old_string, new_string, 1)
            p.write_text(new_text, encoding="utf-8")
            return f"File edited: {file_path}"
        except Exception as e:
            return f"Error editing file: {e}"

    def _tool_glob(self, inp: dict[str, Any]) -> str:
        import glob as globmod
        pattern = inp.get("pattern", "")
        search_path = inp.get("path", str(self.project_dir))
        full_pattern = str(Path(search_path) / pattern)
        try:
            matches = sorted(globmod.glob(full_pattern, recursive=True))[:100]
            if not matches:
                return f"No files matched: {pattern}"
            return "\n".join(matches)
        except Exception as e:
            return f"Glob error: {e}"

    def _tool_grep(self, inp: dict[str, Any]) -> str:
        import re as remod
        pattern = inp.get("pattern", "")
        search_path = inp.get("path", str(self.project_dir))
        file_glob = inp.get("glob", "")
        try:
            regex = remod.compile(pattern, remod.IGNORECASE)
        except remod.error as e:
            return f"Invalid regex: {e}"
        results = []
        search_p = Path(search_path)
        if search_p.is_file():
            files = [search_p]
        else:
            glob_pat = file_glob or "**/*"
            files = sorted(search_p.glob(glob_pat))[:500]
        for fp in files:
            if not fp.is_file() or fp.suffix.lower() not in self.TEXT_EXTS:
                continue
            try:
                for i, line in enumerate(fp.read_text(encoding="utf-8", errors="replace").splitlines(), 1):
                    if regex.search(line):
                        results.append(f"{fp}:{i}:{line.rstrip()}")
                        if len(results) >= 50:
                            break
            except Exception:
                continue
            if len(results) >= 50:
                break
        return "\n".join(results) if results else f"No matches for: {pattern}"

    # ── Memory & Wiki tools ────────────────────────────────────────

    def _tool_memory_recall(self, inp: dict[str, Any]) -> str:
        """L3 deep search across memory + wiki via MemoryService."""
        query = inp.get("query", "")
        limit = int(inp.get("limit", 5))
        try:
            store = self.ipc.memory_service.store
            result = store.recall(query, limit=limit)
            if not result.memories:
                return f"No memories found for: {query}"
            parts = []
            for m in result.memories:
                parts.append(
                    f"[{m.metadata.memory_type}] {m.metadata.name}\n"
                    f"  {m.metadata.description}\n"
                    f"  Tags: {', '.join(m.metadata.tags) if m.metadata.tags else 'none'}\n"
                    f"  {m.body[:500]}"
                )
            return "\n---\n".join(parts)
        except Exception as e:
            return f"Memory recall error: {e}"

    def _tool_memory_save(self, inp: dict[str, Any]) -> str:
        """Save to persistent memory via MemoryService."""
        try:
            store = self.ipc.memory_service.store
            record = store.save_memory(
                title=inp.get("title", "Untitled"),
                body=inp.get("body", ""),
                memory_type=inp.get("memory_type", "project"),
                tags=inp.get("tags"),
                description=inp.get("body", "")[:80],
            )
            return f"Saved memory: {record.metadata.slug} (tier: {record.metadata.tier})"
        except Exception as e:
            return f"Memory save error: {e}"

    async def _tool_wiki_ingest(self, inp: dict[str, Any]) -> str:
        """Ingest content via WikiService (from IpcServer)."""
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(
                request_id=str(uuid.uuid4()),
                action="wiki_ingest",
                payload={
                    "content": inp.get("content", ""),
                    "title": inp.get("title", "Untitled"),
                    "source_type": inp.get("source_type", "manual"),
                    "tags": inp.get("tags", []),
                },
            )
            resp = await self.ipc.wiki_service.ingest(req)
            if resp.ok:
                return f"Ingested: {resp.payload.get('pages_created', 0)} pages created, {resp.payload.get('pages_updated', 0)} updated."
            return f"Ingest failed: {resp.error}"
        except Exception as e:
            return f"WikiIngest error: {e}"

    async def _tool_wiki_query(self, inp: dict[str, Any]) -> str:
        """Query wiki via WikiService (from IpcServer)."""
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(
                request_id=str(uuid.uuid4()),
                action="wiki_query",
                payload={
                    "question": inp.get("question", ""),
                    "save_as_page": inp.get("save_as_page", False),
                },
            )
            resp = await self.ipc.wiki_service.query(req)
            if resp.ok:
                return resp.payload.get("answer", "(no answer)")
            return f"WikiQuery failed: {resp.error}"
        except Exception as e:
            return f"WikiQuery error: {e}"

    async def _tool_wiki_lint(self, inp: dict[str, Any]) -> str:
        """Wiki health check via WikiService (from IpcServer)."""
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(
                request_id=str(uuid.uuid4()),
                action="wiki_lint",
                payload={},
            )
            resp = await self.ipc.wiki_service.lint(req)
            if resp.ok:
                return resp.payload.get("report", "(no report)")
            return f"WikiLint failed: {resp.error}"
        except Exception as e:
            return f"WikiLint error: {e}"

    # ── Web tools ──────────────────────────────────────────────────

    async def _tool_web_fetch(self, inp: dict[str, Any]) -> str:
        """Fetch a URL and return text content."""
        url = inp.get("url", "")
        if not url:
            return "Error: no URL provided"
        try:
            import httpx
            async with httpx.AsyncClient(follow_redirects=True, timeout=15) as client:
                resp = await client.get(url, headers={"User-Agent": "CentaurPsicode/1.0"})
                resp.raise_for_status()
                text = resp.text
                # Strip HTML tags for readability
                import re
                text = re.sub(r'<script[^>]*>.*?</script>', '', text, flags=re.S)
                text = re.sub(r'<style[^>]*>.*?</style>', '', text, flags=re.S)
                text = re.sub(r'<[^>]+>', ' ', text)
                text = re.sub(r'\s+', ' ', text).strip()
                if len(text) > 15000:
                    text = text[:15000] + "\n... [truncated]"
                return text or "(empty page)"
        except Exception as e:
            return f"WebFetch error: {e}"

    async def _tool_web_search(self, inp: dict[str, Any]) -> str:
        """Search the web (via DuckDuckGo HTML — no API key needed)."""
        query = inp.get("query", "")
        if not query:
            return "Error: no query provided"
        try:
            import httpx
            async with httpx.AsyncClient(follow_redirects=True, timeout=10) as client:
                resp = await client.get(
                    "https://html.duckduckgo.com/html/",
                    params={"q": query},
                    headers={"User-Agent": "CentaurPsicode/1.0"},
                )
                import re
                results = re.findall(
                    r'<a rel="nofollow" class="result__a" href="([^"]+)"[^>]*>(.*?)</a>.*?'
                    r'<a class="result__snippet"[^>]*>(.*?)</a>',
                    resp.text, re.S,
                )
                if not results:
                    return f"No search results for: {query}"
                lines = []
                for url, title, snippet in results[:8]:
                    title = re.sub(r'<[^>]+>', '', title).strip()
                    snippet = re.sub(r'<[^>]+>', '', snippet).strip()
                    lines.append(f"- [{title}]({url})\n  {snippet}")
                return "\n\n".join(lines)
        except Exception as e:
            return f"WebSearch error: {e}"

    # ── CrewAI tool ────────────────────────────────────────────────

    async def _tool_crewai(self, inp: dict[str, Any]) -> str:
        """Run a CrewAI crew via IpcServer's CrewAIService."""
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(
                request_id=str(uuid.uuid4()),
                action="crewai_run",
                payload={
                    "crew_name": inp.get("crew_name", ""),
                    "crew_config": inp.get("crew_config", {}),
                    "inputs": inp.get("inputs", {}),
                },
            )
            resp = await self.ipc.crewai_service.handle(req)
            if resp.ok:
                return resp.payload.get("result", "(no result)")
            return f"CrewAI error: {resp.error}"
        except Exception as e:
            return f"CrewAI error: {e}"

    # ── Knowledge Graph tools ────────────────────────────────────

    def _tool_kg_query(self, inp: dict[str, Any]) -> str:
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(request_id=str(uuid.uuid4()), action="kg_query", payload=inp)
            resp = self.ipc._handle_kg(req)
            if resp.ok:
                rels = resp.payload.get("relationships", [])
                if not rels:
                    return f"No relationships found for: {inp.get('entity', '')}"
                lines = []
                for r in rels[:20]:
                    lines.append(f"{r['subject']} --{r['predicate']}--> {r['object']} (from: {r.get('valid_from', '?')}, current: {r.get('current', '?')})")
                return "\n".join(lines)
            return f"KG query error: {resp.error}"
        except Exception as e:
            return f"KGQuery error: {e}"

    def _tool_kg_add(self, inp: dict[str, Any]) -> str:
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(request_id=str(uuid.uuid4()), action="kg_add", payload=inp)
            resp = self.ipc._handle_kg(req)
            if resp.ok:
                return f"Added triple: {inp.get('subject', '')} --{inp.get('predicate', '')}--> {inp.get('object', '')} (id: {resp.payload.get('triple_id', '')})"
            return f"KG add error: {resp.error}"
        except Exception as e:
            return f"KGAdd error: {e}"

    def _tool_kg_timeline(self, inp: dict[str, Any]) -> str:
        try:
            from agent_brain.ipc_types import MemoryRequest
            req = MemoryRequest(request_id=str(uuid.uuid4()), action="kg_timeline", payload=inp)
            resp = self.ipc._handle_kg(req)
            if resp.ok:
                tl = resp.payload.get("timeline", [])
                if not tl:
                    return "No timeline entries found."
                lines = []
                for entry in tl[:30]:
                    lines.append(f"[{entry.get('valid_from', '?')}] {entry['subject']} {entry['predicate']} {entry['object']}")
                return "\n".join(lines)
            return f"KG timeline error: {resp.error}"
        except Exception as e:
            return f"KGTimeline error: {e}"

    # ── Skill execution ────────────────────────────────────────────

    async def _tool_run_skill(self, inp: dict[str, Any]) -> str:
        """Execute a bundled skill via SkillService."""
        try:
            from agent_brain.ipc_types import SkillRequest
            skill_name = inp.get("skill_name", "")
            if not skill_name:
                return "Error: skill_name is required"
            req = SkillRequest(
                request_id=str(uuid.uuid4()),
                skill_name=skill_name,
                arguments=inp.get("arguments", {}),
            )
            resp = await self.ipc.skill_handler(req)
            return resp.content if hasattr(resp, "content") else str(resp)
        except Exception as e:
            return f"Skill error: {e}"

    # ── Agentic Chat Loop (LLM → tool → result → LLM → ...) ──────

    async def _handle_chat(self, websocket: Any, msg: dict[str, Any]) -> None:
        """Full agentic loop: call LLM, execute tools, feed results back, repeat."""
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
        if msg.get("model"):
            session.model = msg["model"]
        if msg.get("provider"):
            session.provider = msg["provider"]

        session.add_user_message(content)

        # Build system prompt with full context
        system_prompt = self._build_system_prompt(content)

        # ── Agentic loop: up to 15 iterations ──
        MAX_TOOL_ROUNDS = 15
        total_usage: dict[str, int] = {}

        try:
            for round_num in range(MAX_TOOL_ROUNDS):
                request_id = str(uuid.uuid4())
                request = StreamRequest(
                    request_id=request_id,
                    model=session.model,
                    messages=session.messages,
                    tools=self.TOOL_DEFINITIONS,
                    system_prompt=system_prompt,
                    max_output_tokens=8192,
                    provider=session.provider,
                )

                # Stream one LLM turn
                accumulated_text = ""
                content_blocks: list[dict[str, Any]] = []
                pending_tool_calls: list[dict[str, Any]] = []
                stop_reason = "end_turn"

                async for event in self.backend.stream_message(request):
                    event_type = event.get("type", "")

                    if event_type == "text_delta":
                        delta = event.get("delta", "")
                        accumulated_text += delta
                        await websocket.send(json.dumps({
                            "type": "text_delta", "session_id": sid, "delta": delta,
                        }))

                    elif event_type == "tool_use":
                        tool_call = {
                            "id": event.get("tool_call_id", ""),
                            "name": event.get("name", ""),
                            "input": event.get("input", {}),
                        }
                        pending_tool_calls.append(tool_call)
                        content_blocks.append({"type": "tool_use", **tool_call})
                        await websocket.send(json.dumps({
                            "type": "tool_use", "session_id": sid,
                            "tool_call_id": tool_call["id"],
                            "name": tool_call["name"],
                            "input": tool_call["input"],
                        }))

                    elif event_type == "message_done":
                        usage = event.get("usage", {})
                        stop_reason = event.get("stop_reason", "end_turn")
                        for k, v in usage.items():
                            total_usage[k] = total_usage.get(k, 0) + (v if isinstance(v, int) else 0)
                        break

                # Build assistant message content blocks
                if accumulated_text:
                    content_blocks.insert(0, {"type": "text", "text": accumulated_text})
                if content_blocks:
                    session.add_assistant_message(content_blocks)

                # If no tool calls, we're done
                if not pending_tool_calls:
                    await websocket.send(json.dumps({
                        "type": "message_done", "session_id": sid,
                        "usage": total_usage, "stop_reason": stop_reason,
                    }))
                    # M1: Extract memories from this turn (fire-and-forget)
                    if accumulated_text and content:
                        asyncio.create_task(self._extract_memories_from_turn(content, accumulated_text))
                    # SM4: Maybe trigger dream consolidation
                    self._turn_count += 1
                    asyncio.create_task(self._maybe_dream())
                    break

                # ── Execute tools and feed results back ──
                tool_results_content: list[dict[str, Any]] = []
                for tc in pending_tool_calls:
                    # Notify browser that tool is executing
                    await websocket.send(json.dumps({
                        "type": "tool_executing", "session_id": sid,
                        "tool_call_id": tc["id"], "name": tc["name"],
                    }))

                    # Execute the tool
                    result_text = await self._execute_tool(tc["name"], tc["input"])

                    # Notify browser of tool result
                    await websocket.send(json.dumps({
                        "type": "tool_result", "session_id": sid,
                        "tool_call_id": tc["id"], "name": tc["name"],
                        "output": result_text[:2000],  # Truncate for display
                    }))

                    tool_results_content.append({
                        "type": "tool_result",
                        "tool_use_id": tc["id"],
                        "content": result_text,
                    })

                # Add tool results as a user message (Anthropic format)
                session.messages.append({
                    "role": "user",
                    "content": tool_results_content,
                })

                # Loop continues — LLM will see tool results and respond

            else:
                # Max rounds reached
                await websocket.send(json.dumps({
                    "type": "text_delta", "session_id": sid,
                    "delta": "\n\n[Reached maximum tool execution rounds (15). Stopping.]",
                }))
                await websocket.send(json.dumps({
                    "type": "message_done", "session_id": sid,
                    "usage": total_usage, "stop_reason": "max_rounds",
                }))

        except Exception as e:
            error_msg = str(e)
            logger.error("Chat stream error: %s", error_msg)
            await websocket.send(json.dumps({
                "type": "error", "session_id": sid, "message": error_msg,
            }))

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
        """Read a text file or PDF, return content + language."""
        p = Path(file_path).resolve()
        if not p.is_file():
            return {"error": f"Not a file: {file_path}"}

        ext = p.suffix.lower()

        # Handle PDF files
        if ext == ".pdf":
            return self._read_pdf(p)

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

    @staticmethod
    def _read_pdf(p: Path) -> dict[str, Any]:
        """Extract text from a PDF file. Tries multiple methods."""
        text = ""

        # Method 1: PyPDF2 / pypdf
        try:
            import pypdf
            reader = pypdf.PdfReader(str(p))
            pages = []
            for i, page in enumerate(reader.pages):
                page_text = page.extract_text() or ""
                if page_text.strip():
                    pages.append(f"--- Page {i+1} ---\n{page_text}")
            text = "\n\n".join(pages)
            if text.strip():
                return {
                    "path": str(p),
                    "name": p.name,
                    "content": text,
                    "language": "plaintext",
                    "size": len(text),
                    "pages": len(reader.pages),
                    "method": "pypdf",
                }
        except ImportError:
            pass
        except Exception:
            pass

        # Method 2: pdfminer.six
        try:
            from pdfminer.high_level import extract_text as pdfminer_extract
            text = pdfminer_extract(str(p))
            if text.strip():
                return {
                    "path": str(p),
                    "name": p.name,
                    "content": text,
                    "language": "plaintext",
                    "size": len(text),
                    "method": "pdfminer",
                }
        except ImportError:
            pass
        except Exception:
            pass

        # Method 3: Basic fallback — tell user to install a PDF library
        return {
            "error": (
                f"Cannot read PDF: {p.name}\n"
                "Install a PDF library:\n"
                "  pip install pypdf\n"
                "  OR: pip install pdfminer.six"
            ),
            "path": str(p),
            "name": p.name,
        }

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
                        "can_ingest": f.suffix.lower() in {".md", ".txt", ".html", ".json", ".yaml", ".yml", ".pdf"},
                        "can_view": f.suffix.lower() in self.TEXT_EXTS or f.suffix.lower() == ".pdf",
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
