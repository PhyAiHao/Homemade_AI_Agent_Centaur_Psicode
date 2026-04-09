# State Ownership Contract

This document defines which runtime owns which state. Violations of these
boundaries lead to race conditions, stale reads, and "who is the source of
truth?" bugs.

## Principle

> **One owner per state domain. The other side asks via IPC.**

---

## Rust (agent-core) Owns: Runtime State

All in-memory, session-scoped state lives in `Arc<RwLock<AppState>>`:

| State | Struct | Owner |
|-------|--------|-------|
| Tasks (CRUD, status, deps) | `Task` | Rust `AppState.tasks` |
| Teams (create, members, roles) | `TeamState` | Rust `AppState.team` |
| Worktrees (enter/exit, CWD) | `WorktreeState` | Rust `AppState.worktree` |
| Plan mode (on/off) | `bool` | Rust `AppState.plan_mode` |
| Cron jobs | `CronJob` | Rust `AppState.cron_jobs` |
| Agent mailboxes | `HashMap<String, Vec<String>>` | Rust `AppState.agent_mailboxes` |
| Current CWD / project root | `PathBuf` | Rust `AppState.cwd` |
| Session ID | `String` | Rust `AppState.session_id` |
| File state cache | `FileStateCache` | Rust |
| Conversation history | `Vec<ConversationMessage>` | Rust query loop |
| Cost tracking | `CostTracker` | Rust query loop |
| Permission decisions | `PermissionGate` | Rust |

**Python must NOT cache copies of these.** If Python needs task status for
memory extraction, Rust includes it in the IPC request payload.

## Python (agent-brain) Owns: Persistent State

All durable, cross-session state lives on disk managed by Python services:

| State | Service | Storage |
|-------|---------|---------|
| Memory files (*.md) | `MemoryStore` | `~/.agent/memory/` |
| MEMORY.md index | `MemoryStore` | `~/.agent/memory/MEMORY.md` |
| Team memory | `TeamMemorySyncManager` | `~/.agent/memory/team/` |
| Session memory | `SessionMemoryManager` | `~/.agent/memory/session.json` |
| Session transcripts | `SessionMemoryManager` | `~/.agent/sessions/*.jsonl` |
| Analytics/cost history | `AnalyticsService` | internal |
| Skills definitions | `SkillService` | YAML files |
| Plugin manifests | `PluginLoader` | JSON manifests |
| Output styles | `OutputStyleService` | YAML files |

**Rust must NOT write memory files directly.** Rust reads `.consolidate-lock`
for dream gating (mtime + PID only), but all memory content goes through IPC.

## Shared (read-only by both)

| State | Written By | Read By |
|-------|-----------|---------|
| `.consolidate-lock` mtime | Rust (dream lock) | Rust (dream gate) |
| `AGENT_IPC_SOCKET` path | Environment | Both |
| `.env` file | User | Both (at startup) |
| `CLAUDE.md` files | User | Rust (system prompt) |
| `~/.agent/config.json` | Rust (config tool) | Rust (startup) |

## IPC Contract

When Python needs runtime info, Rust includes it in the request:
- `MemoryRequest.payload.task_status` — current task states (for extraction context)
- `MemoryRequest.payload.session_id` — session identifier

When Rust needs persistent info, it asks Python via IPC:
- `MemoryRequest(action="recall")` — query memory store
- `MemoryRequest(action="session_get")` — read session memory
- `MemoryRequest(action="render_prompt")` — get memory system prompt
