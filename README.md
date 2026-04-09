# Centaur Psicode

> A polyglot AI agent CLI — Rust core, Python intelligence, TypeScript integrations.

Built as a full rewrite of the Claude Code architecture with many improvements:

---

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  agent-core/  (Rust)                                        │
│  ─ CLI entry point (clap)                                   │
│  ─ Tool-use loop + streaming query engine                   │
│  ─ 45 tools (file, bash, git, agent, task, cron...)         │
│  ─ Permission system                                        │
│  ─ Terminal UI (Ratatui — full Ink replacement)             │
│  ─ Vim mode, keybindings                                    │
│  ─ Remote sessions, server, proxy                           │
└────────────────────┬────────────────────────────────────────┘
                     │ Unix socket + msgpack (ipc_schema.json)
┌────────────────────▼────────────────────────────────────────┐
│  agent-brain/  (Python)                                     │
│  ─ Anthropic API client (streaming)                         │
│  ─ Local LLM backend (Ollama)                               │
│  ─ Memory system (extract, persist, inject)                 │
│  ─ Context compression (auto, snip, micro)                  │
│  ─ Skills system (11 bundled + user-defined)                │
│  ─ Plugin system                                            │
│  ─ Voice STT                                                │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│  agent-integrations/  (TypeScript)                          │
│  ─ VS Code / JetBrains IDE bridge                           │
│  ─ Model Context Protocol (MCP) server                      │
│  ─ Language Server Protocol (LSP) client                    │
│  ─ Web fetch + search tools                                 │
│  ─ OAuth 2.0 authentication                                 │
└─────────────────────────────────────────────────────────────┘
```

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs) 1.80+
- Python 3.11+
- Node.js 20+ (for IDE integrations)

### Install

```bash
# Clone and build
git clone https://github.com/your-org/centaur-psicode
cd centaur-psicode/ai-agent

# Copy environment config
cp .env.example .env
# Edit .env and add your ANTHROPIC_API_KEY

# Build everything
make build

# Install binary
make install
```

### Run

```bash
# Start the Python IPC server (in background)
make dev-python &

# Launch the agent
agent
```

### Use local LLM (no API key needed)

```bash
# Install Ollama: https://ollama.com
ollama pull llama3.3:70b

# Set in .env:
# OLLAMA_BASE_URL=http://localhost:11434
# LOCAL_MODEL=llama3.3:70b

agent
```

---

## Commands

| Command | Description |
|---|---|
| `/help` | Show all commands |
| `/model` | Switch model |
| `/compact` | Compress conversation history |
| `/memory` | Manage persistent memory |
| `/skills` | List and run skills |
| `/diff` | Show file changes |
| `/tasks` | View background tasks |
| `/vim` | Toggle vim mode |
| `/plan` | Enter plan mode |
| `/cost` | Show token usage |
| `/doctor` | Diagnose environment |
| `/login` | Set API key |
| `/clear` | Clear conversation |

---

## Development

```bash
make test        # Run all tests
make lint        # Lint all three layers
make fmt         # Format all code
make doctor      # Check environment
make clean       # Clean build artifacts
```

---

## Project Structure

```
ai-agent/
├── AGENT_STATUS.md          # Two-agent coordination log
├── ipc_schema.json          # Rust↔Python message protocol
├── .env.example
├── Makefile
├── agent-core/              # Rust — engine, TUI, tools
├── agent-brain/             # Python — AI, memory, skills
└── agent-integrations/      # TypeScript — IDE, MCP, LSP
```

---

## License

MIT
