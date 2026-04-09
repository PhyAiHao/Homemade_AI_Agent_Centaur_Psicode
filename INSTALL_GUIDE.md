# Centaur Psicode — Complete Installation & Usage Guide

## Table of Contents

1. [System Requirements](#1-system-requirements)
2. [Install Prerequisites](#2-install-prerequisites)
3. [Set Up the Project](#3-set-up-the-project)
4. [Configure API Keys](#4-configure-api-keys)
5. [Build the Agent](#5-build-the-agent)
6. [Start the GUI](#6-start-the-gui)
7. [Using the GUI](#7-using-the-gui)
8. [Using Voice Input](#8-using-voice-input)
9. [Using the Document Library](#9-using-the-document-library)
10. [Using CrewAI (Multi-Agent)](#10-using-crewai-multi-agent)
11. [Using Media Tools](#11-using-media-tools-tts-images-video)
12. [Using the Memory System](#12-using-the-memory-system-llm-wiki)
13. [Running the CLI Agent](#13-running-the-cli-agent-advanced)
14. [Optional Packages](#14-optional-packages)
15. [Troubleshooting](#15-troubleshooting)

---

## 1. System Requirements

| Requirement | Minimum | Recommended |
|-------------|---------|-------------|
| **macOS** | 12 Monterey | 14 Sonoma+ |
| **Python** | 3.10 | 3.10 (pinned in Makefile) |
| **Rust** | 1.70+ | latest stable |
| **Node.js** | 18 | 20+ |
| **RAM** | 8 GB | 16 GB+ |
| **Disk** | 2 GB | 5 GB+ (with local models) |

---

## 2. Install Prerequisites

### Python 3.10

```bash
# Install via Homebrew (recommended for macOS)
brew install python@3.10

# Verify
/opt/homebrew/opt/python@3.10/libexec/bin/python3 --version
# Should show: Python 3.10.x
```

### Rust

```bash
# Install Rust toolchain
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Restart terminal, then verify
rustc --version
cargo --version
```

### Node.js (for VS Code extension and MCP integrations)

```bash
brew install node

# Verify
node --version   # Should be 18+
npm --version
```

### Ollama (free local AI — no API key needed)

```bash
# Install
brew install ollama

# Pull a model (pick one)
ollama pull gemma4:31b        # 19 GB — best quality, needs 32GB RAM
ollama pull llama3.1:latest   # 4.9 GB — good balance
ollama pull llama3:latest     # 4.7 GB — lightweight

# Start the Ollama server (keep this running)
ollama serve
```

### FFmpeg (optional — for video composition)

```bash
brew install ffmpeg
```

---

## 3. Set Up the Project

```bash
# Navigate to the project
cd "/Users/phyaihao/Desktop/new ideas related paper/AI model and Intellegence/Claude_Code/Centaur_Psicode/Homemade_AI_Agent_v1"

# Create environment file
touch .env
```

---

## 4. Configure API Keys

Edit the `.env` file in the project root. You need **at least one** LLM provider:

```bash
# ═══════════════════════════════════════════════
# LLM PROVIDERS (choose at least one)
# ═══════════════════════════════════════════════

# Anthropic (Claude) — https://console.anthropic.com
ANTHROPIC_API_KEY=sk-ant-your-key-here

# OpenAI (GPT-4o, DALL-E, TTS) — https://platform.openai.com
OPENAI_API_KEY=sk-your-key-here

# Google Gemini — https://aistudio.google.com
GEMINI_API_KEY=AIzaSy-your-key-here

# Ollama needs NO key — just run "ollama serve"

# ═══════════════════════════════════════════════
# MODEL SELECTION (optional)
# ═══════════════════════════════════════════════

CLAUDE_MODEL=claude-sonnet-4-6
AGENT_PROVIDER=first_party

# For Ollama:
# CLAUDE_MODEL=gemma4:31b
# AGENT_PROVIDER=ollama

# ═══════════════════════════════════════════════
# MEDIA TOOLS (optional)
# ═══════════════════════════════════════════════

# ElevenLabs TTS — https://elevenlabs.io
# ELEVENLABS_API_KEY=your-key

# Stability AI (images) — https://platform.stability.ai
# STABILITY_API_KEY=sk-your-key

# Runway (video) — https://runwayml.com
# RUNWAY_API_KEY=your-key

# Kling AI (video) — https://kling.ai
# KLING_API_KEY=your-key
```

### If you want free-only (no API keys):

Just use Ollama — set these in `.env`:

```bash
CLAUDE_MODEL=gemma4:31b
AGENT_PROVIDER=ollama
```

---

## 5. Build the Agent

```bash
cd "/Users/phyaihao/Desktop/new ideas related paper/AI model and Intellegence/Claude_Code/Centaur_Psicode/Homemade_AI_Agent_v1"

# Install Python dependencies
cd agent-brain && pip install -e . && cd ..

# Build Rust binary
cd agent-core && cargo build --release && cd ..

# (Optional) Build TypeScript
cd agent-integrations && npm install && npm run build && cd ..

# (Optional) Install CrewAI for multi-agent features
pip install crewai

# (Optional) Install free TTS
pip install edge-tts

# (Optional) Install vector search
pip install chromadb
```

Or use the Makefile shortcut:

```bash
make build    # Builds everything
make deps     # Installs missing Python packages
```

---

## 6. Start the GUI

```bash
cd "/Users/phyaihao/Desktop/new ideas related paper/AI model and Intellegence/Claude_Code/Centaur_Psicode/Homemade_AI_Agent_v1"

python gui/server.py
```

You should see:

```
  Centaur Psicode Web GUI
  ─────────────────────────────────
  GUI:       http://127.0.0.1:8420
  WebSocket: ws://127.0.0.1:8421
  Providers: Anthropic, Ollama (local)
  Model:     claude-sonnet-4-6

  Open http://127.0.0.1:8420 in your browser.

  Press Ctrl+C to stop.
```

Open **http://127.0.0.1:8420** in Chrome or Safari.

---

## 7. Using the GUI

### Landing Page

The landing page shows what Centaur Psicode can do. Click **"Open Chat"** to enter the IDE.

### IDE Layout

The IDE has 3 main panels, all resizable by dragging the dividers:

```
+------------------+----------------------------+------------------+
|   LEFT PANEL     |   CENTER PANEL             |   RIGHT PANEL    |
|                  |                            |                  |
|   [Explorer]     |   [file.py] [main.rs] ×   |   AI CHAT        |
|   [Docs]         |   1│ def hello():          |                  |
|   [Crews]        |   2│     print("hi")       |   🎤 [Message] ↑ |
|                  |                            |                  |
|   📁 agent-core  +----------------------------+                  |
|   📁 agent-brain |   TERMINAL                 |                  |
|   📁 gui         |   ~/project $ ls           |                  |
|   📄 Makefile    |   agent-core/ agent-brain/ |                  |
+------------------+----------------------------+------------------+
```

### Left Panel — 3 Tabs

| Tab | What It Does |
|-----|-------------|
| **Explorer** | Browse files on your computer. Quick-pick buttons: Project, Home, Desktop, Documents, Downloads. Click files to open in editor. |
| **Docs** | Document library. Upload papers, articles, transcripts. Click "Ingest" to add to the wiki knowledge base. |
| **Crews** | CrewAI multi-agent builder. Create and customize AI agent teams. |

### Center Panel

| Area | What It Does |
|------|-------------|
| **Editor** (top) | Tabbed file viewer with line numbers. Click files in Explorer to open them. |
| **Terminal** (bottom) | Real terminal. Type commands, see output. Supports `cd`, history (arrow keys). |

### Right Panel — AI Chat

- Type a message and press **Enter** or click **↑** to send
- Click **🎤** for voice input
- Click **+** for new conversation
- Click **⚙** (top-right) to open Settings

### Settings (⚙ gear icon)

The settings drawer lets you configure:
- **LLM Provider** — Anthropic, OpenAI, Gemini, or Ollama
- **API Key** — enter and save (stored in `.env`)
- **Model** — pick from dropdown (auto-populated per provider)
- **Voice & TTS** — choose TTS provider, enter ElevenLabs key
- **Image Generation** — DALL-E or Stability AI
- **Video Generation** — Runway or Kling

Green dots = installed/configured. Red dots = not yet set up.

---

## 8. Using Voice Input

### Setup

1. Open the GUI in **Chrome** or **Safari** (Firefox doesn't support Web Speech API)
2. macOS: **System Settings → Privacy & Security → Microphone** → allow your browser
3. When you first click 🎤, the browser will ask for microphone permission → click **Allow**

### Usage

1. Click **🎤** (microphone button) next to the text input
2. The button turns **red and pulses** while recording
3. **Speak naturally** — words appear in real-time
4. **Speech repair** automatically removes:
   - Filler words: "um", "uh", "hmm", "like", "you know", "basically"
   - Stutters: "the the" → "the"
   - False starts: "go to the — I mean visit the" → "visit the"
5. **Stop speaking for 3 seconds** → message auto-sends
6. Or click 🎤 again to stop, then edit and send manually

---

## 9. Using the Document Library

### Adding Documents

**Method 1: GUI Upload**
1. Click **Docs** tab in the left panel
2. Select a category (Papers, Articles, Transcripts, Downloads)
3. Click **↑ Upload File** → pick files from your computer

**Method 2: Drag into folder**
Copy files directly into:
```
Homemade_AI_Agent_v1/documents/
  papers/        ← Research papers
  articles/      ← Blog posts, web articles
  transcripts/   ← Meeting notes, chat exports
  downloads/     ← AI-downloaded content
```

**Method 3: Ask the AI**
In the chat: *"Download the article at https://example.com and save it"*

### Actions on Documents

| Button | What It Does |
|--------|-------------|
| **View** | Opens in the editor panel |
| **Ingest** | Sends to AI → extracts into wiki pages with cross-references |
| **✕** | Deletes the file |

### Ingesting into the Wiki

When you click **Ingest** on a document:
1. The AI reads the full document
2. Creates a **summary page** in the wiki
3. Creates **entity pages** (people, tools, projects mentioned)
4. Creates **concept pages** (patterns, methodologies)
5. Adds **cross-references** between pages with `[[slug]]` links
6. Stores **raw verbatim chunks** for accurate search
7. Logs the ingestion in `wiki/log.md`

---

## 10. Using CrewAI (Multi-Agent)

### Install

```bash
pip install crewai
```

### Using Pre-Built Crews

1. Click **Crews** tab in the left panel
2. Click a saved crew (e.g., `code_review`, `research_report`)
3. Review the agents and tasks
4. Click **Save & Run**

Or in the chat: *"Run the code_review crew on the auth module"*

### Building Custom Crews

1. Click **Crews** tab → scroll to **Build New Crew**
2. Set **Crew Name** and **Description**
3. Choose **Process**: Sequential (one after another) or Hierarchical (manager delegates)
4. Use **+/-** buttons to set **Number of Agents** (1-8)
5. For each agent, fill in:
   - **Name**: e.g., "researcher"
   - **Role**: e.g., "Research Analyst"
   - **Goal**: what this agent aims to achieve
   - **Backstory**: expertise and personality (shapes reasoning)
   - **Model**: which AI model (can be different per agent!)
   - **Allow delegation**: can this agent ask others for help?
6. Use **+/-** to set **Number of Tasks** (1-12)
7. For each task:
   - **Description**: what to do
   - **Expected output**: what the deliverable looks like
   - **Agent**: which agent handles this task
   - **Context**: which earlier tasks feed into this one (e.g., "0,1")
8. Click **Save Crew** to save as template, or **Save & Run** to execute

### Pre-Built Crew Templates

| Crew | Agents | Tasks | Use Case |
|------|--------|-------|----------|
| `code_review` | Security Auditor, Code Reviewer, Report Writer | 3 sequential | Code quality and security review |
| `research_report` | Researcher, Technical Writer | 2 sequential | Research and report on any topic |

---

## 11. Using Media Tools (TTS, Images, Video)

### Text-to-Speech

```
Chat: "Read this aloud: Hello, I am Centaur Psicode."
Chat: "Convert this text to speech and save as audio.mp3"
```

| Provider | Setup | Quality |
|----------|-------|---------|
| Edge TTS (free) | `pip install edge-tts` | Good |
| OpenAI TTS | Set `OPENAI_API_KEY` | Great |
| ElevenLabs | `pip install elevenlabs` + key | Best |

### Image Generation

```
Chat: "Generate an image of a futuristic AI brain"
Chat: "Create a logo for Centaur Psicode"
```

Requires `OPENAI_API_KEY` (for DALL-E 3) or `STABILITY_API_KEY`.

### Video Generation

```
Chat: "Generate a 10-second video of a neural network"
```

Requires `RUNWAY_API_KEY` or `KLING_API_KEY`.

### Audio + Image → Video (free, local)

```bash
brew install ffmpeg
```

```
Chat: "Combine this image and audio into a video"
```

### Configure in GUI

Click **⚙ Settings** → scroll down to **Voice & TTS**, **Image Generation**, **Video Generation** sections. Enter API keys and choose providers.

---

## 12. Using the Memory System (LLM Wiki)

The memory system is automatic — it works in the background:

### How It Works

| Layer | Tokens | When | What |
|-------|--------|------|------|
| **L0 Identity** | ~100 | Always | Who you are, core constraints |
| **L1 Essentials** | ~800 | Always | Top-15 memories by importance |
| **L2 On-Demand** | ~2K | Topic match | Relevant context for your question |
| **L3 Deep Search** | Unlimited | On request | Full hybrid search |

### Automatic Features

- **Memory extraction**: After each conversation turn, durable knowledge is saved
- **Dream consolidation**: Every 12 hours, reviews transcripts and organizes memory
- **Cross-references**: Pages link to each other via `[[slug]]` syntax
- **Importance scoring**: Memories ranked by type, recency, access, backlinks

### Manual Commands (in chat)

```
"Remember that I prefer Python over Java"          → saves user preference
"What do we know about the auth system?"            → searches wiki
"Ingest this document into the wiki"                → WikiIngest
"Check wiki health"                                 → WikiLint
```

### Where Data Lives

```
~/.agent/memory/
  core/              ← Always in system prompt (max 10 files)
  archive/           ← Searchable on demand (unlimited)
  vectors/           ← ChromaDB embeddings (semantic search)
  wiki/log.md        ← Ingestion/query history
  knowledge_graph.sqlite3  ← Entity relationships
```

---

## 13. Running the CLI Agent (Advanced)

Besides the GUI, you can run the agent in the terminal:

```bash
cd "/Users/phyaihao/Desktop/new ideas related paper/AI model and Intellegence/Claude_Code/Centaur_Psicode/Homemade_AI_Agent_v1"

# Start both Rust CLI + Python brain
make dev

# Or start components separately:
make dev-python   # Python brain only (IPC server)
make dev-rust     # Rust CLI only (connects to Python brain)
make dev-gui      # Web GUI only (standalone, no IPC needed)
```

---

## 14. Optional Packages

### Recommended

| Package | Command | What It Enables |
|---------|---------|----------------|
| **CrewAI** | `pip install crewai` | Multi-agent teams |
| **Edge TTS** | `pip install edge-tts` | Free text-to-speech |
| **ChromaDB** | `pip install chromadb` | Semantic vector search |
| **FFmpeg** | `brew install ffmpeg` | Video composition |

### Premium (need API keys)

| Package | Command | What It Enables |
|---------|---------|----------------|
| **ElevenLabs** | `pip install elevenlabs` | Premium TTS voices |
| **OpenAI** | `pip install openai` | DALL-E images + OpenAI TTS |

### All at once

```bash
pip install crewai edge-tts chromadb elevenlabs openai
brew install ffmpeg
```

---

## 15. Troubleshooting

| Problem | Solution |
|---------|---------|
| `ModuleNotFoundError: agent_brain` | Run from the project root, or `cd agent-brain && pip install -e .` |
| `ModuleNotFoundError: websockets` | `pip install websockets` |
| `ModuleNotFoundError: msgpack` | `pip install msgpack` |
| GUI loads but chat says "Not connected" | Server not running. Check terminal for errors. |
| "No API key configured" | Open Settings (⚙), enter your API key, click Save |
| "model not found" error | Wrong model name. Open Settings, pick from dropdown. |
| Voice says "not supported" | Use Chrome or Safari (not Firefox) |
| Voice says "microphone denied" | macOS: System Settings → Privacy → Microphone → allow browser |
| `cargo build` fails | Run `rustup update` to update Rust toolchain |
| Ollama "connection refused" | Run `ollama serve` in a separate terminal |
| Terminal empty / no output | Restart the server (`Ctrl+C`, then `python gui/server.py`) |

### Quick Health Check

```bash
cd "/Users/phyaihao/Desktop/new ideas related paper/AI model and Intellegence/Claude_Code/Centaur_Psicode/Homemade_AI_Agent_v1"

# Check Python
python3 --version

# Check Rust
cargo --version

# Check Python deps
python3 -c "import anthropic, msgpack, websockets, pydantic; print('Core deps OK')"

# Check optional deps
python3 -c "import crewai; print('CrewAI OK')" 2>/dev/null || echo "CrewAI: not installed"
python3 -c "import edge_tts; print('Edge TTS OK')" 2>/dev/null || echo "Edge TTS: not installed"
python3 -c "import chromadb; print('ChromaDB OK')" 2>/dev/null || echo "ChromaDB: not installed"

# Check Ollama
ollama list 2>/dev/null || echo "Ollama: not running (run 'ollama serve')"

# Check FFmpeg
ffmpeg -version 2>/dev/null | head -1 || echo "FFmpeg: not installed (brew install ffmpeg)"
```

---

## Quick Start Summary

```bash
# 1. Install prerequisites
brew install python@3.10 ollama
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# 2. Pull a free local model
ollama pull gemma4:31b && ollama serve

# 3. Install Python dependencies
cd agent-brain && pip install -e . && cd ..
pip install crewai edge-tts chromadb

# 4. Create .env with your keys (or just use Ollama)
echo "CLAUDE_MODEL=gemma4:31b" > .env
echo "AGENT_PROVIDER=ollama" >> .env

# 5. Start the GUI
python gui/server.py

# 6. Open http://127.0.0.1:8420 in Chrome
```
