//! Tool system — the `Tool` trait and all tool implementations.
//!
//! Mirrors `src/Tool.ts`, `src/tools/`, and `src/services/tools/`.
#![allow(dead_code)]

pub mod execution;
pub mod hooks;
pub mod orchestration;
pub mod streaming_executor;

// Tool implementations (S5A, S6A, S7A)
pub mod file_read;
pub mod file_write;
pub mod file_edit;
pub mod glob;
pub mod grep;
pub mod bash;
pub mod powershell;
pub mod agent;
pub mod sleep;
pub mod task_create;
pub mod task_get;
pub mod task_list;
pub mod task_output;
pub mod task_stop;
pub mod task_update;
pub mod team_create;
pub mod team_delete;
pub mod worktree_enter;
pub mod worktree_exit;
pub mod plan_mode_enter;
pub mod plan_mode_exit;
pub mod cron_create;
pub mod cron_schedule;
pub mod remote_trigger;
pub mod send_message;
pub mod ask_user;
pub mod brief;
pub mod config_tool;
pub mod repl;
pub mod synthetic_output;
pub mod todo_write;
pub mod tool_search;
pub mod skill_tool;
pub mod memory_recall;
pub mod crewai_tool;
pub mod wiki_ingest;
pub mod wiki_query;

// IPC proxy tools (T8) — forward to agent-integrations TypeScript layer
pub mod mcp_tool;
pub mod mcp_auth_tool;
pub mod mcp_list_resources;
pub mod mcp_read_resource;
pub mod lsp_tool;
pub mod web_fetch_tool;
pub mod web_search_tool;
pub mod notebook_edit_tool;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::permissions::gate::{GateDecision, PermissionGate};
use crate::state::SharedState;

/// The core trait every tool must implement.
///
/// Mirrors the Tool type defined in `src/Tool.ts`.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name used in API tool definitions and permission rules.
    fn name(&self) -> &'static str;

    /// Human-readable description shown in /help and the API tool list.
    fn description(&self) -> &str;

    /// JSON Schema for the tool's input object.
    fn input_schema(&self) -> Value;

    /// Execute the tool with the given input.
    /// The `output_tx` channel receives streaming output lines as they are produced.
    async fn execute(
        &self,
        input: Value,
        output_tx: tokio::sync::mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult>;

    /// Whether this tool requires explicit user permission in Default mode.
    /// Read-only tools (FileRead, Glob, Grep) return false.
    fn requires_permission(&self) -> bool { true }

    /// Whether this tool's output should be shown inline in the TUI.
    fn show_inline_output(&self) -> bool { true }
}

/// A single chunk of streaming output from a tool (shown in TUI as it arrives).
#[derive(Debug, Clone)]
pub struct ToolOutput {
    pub text:     String,
    pub is_error: bool,
}

/// Final result returned after tool execution completes.
#[derive(Debug, Clone)]
pub struct ToolResult {
    /// The full output string returned to Claude as the tool_result content.
    pub content:  String,
    pub is_error: bool,
    /// Optional structured data (used by SyntheticOutputTool).
    pub metadata: Option<Value>,
}

impl ToolResult {
    pub fn ok(content: impl Into<String>) -> Self {
        ToolResult { content: content.into(), is_error: false, metadata: None }
    }

    pub fn error(msg: impl Into<String>) -> Self {
        ToolResult { content: msg.into(), is_error: true, metadata: None }
    }
}

/// Registry of all available tools.
pub struct ToolRegistry {
    tools: Vec<Arc<dyn Tool>>,
}

impl ToolRegistry {
    /// Build the default registry with all built-in tools.
    ///
    /// Tools that need shared state (task, team, worktree, plan mode, cron, messaging)
    /// receive a clone of the `SharedState` handle.
    pub fn default_registry(state: SharedState) -> Self {
        let tools: Vec<Arc<dyn Tool>> = vec![
            // ── File tools (S5A / T2-T4) ───────────────────────────────
            Arc::new(file_read::FileReadTool),
            Arc::new(file_write::FileWriteTool),
            Arc::new(file_edit::FileEditTool),
            Arc::new(glob::GlobTool),
            Arc::new(grep::GrepTool),

            // ── Shell tools (S6A / T5) ─────────────────────────────────
            Arc::new(bash::BashTool::new()),
            Arc::new(powershell::PowerShellTool),

            // ── Agent + Sleep (S7A / T7) ───────────────────────────────
            Arc::new(agent::AgentTool),
            Arc::new(sleep::SleepTool),

            // ── Task management ────────────────────────────────────────
            Arc::new(task_create::TaskCreateTool   { state: state.clone() }),
            Arc::new(task_get::TaskGetTool         { state: state.clone() }),
            Arc::new(task_list::TaskListTool        { state: state.clone() }),
            Arc::new(task_output::TaskOutputTool    { state: state.clone() }),
            Arc::new(task_stop::TaskStopTool        { state: state.clone() }),
            Arc::new(task_update::TaskUpdateTool    { state: state.clone() }),

            // ── Team management ────────────────────────────────────────
            Arc::new(team_create::TeamCreateTool   { state: state.clone() }),
            Arc::new(team_delete::TeamDeleteTool   { state: state.clone() }),

            // ── Worktree ───────────────────────────────────────────────
            Arc::new(worktree_enter::EnterWorktreeTool { state: state.clone() }),
            Arc::new(worktree_exit::ExitWorktreeTool   { state: state.clone() }),

            // ── Plan mode ──────────────────────────────────────────────
            Arc::new(plan_mode_enter::EnterPlanModeTool { state: state.clone() }),
            Arc::new(plan_mode_exit::ExitPlanModeTool   { state: state.clone() }),

            // ── Cron ───────────────────────────────────────────────────
            Arc::new(cron_create::CronCreateTool     { state: state.clone() }),
            Arc::new(cron_schedule::CronScheduleTool { state: state.clone() }),

            // ── Communication ──────────────────────────────────────────
            Arc::new(remote_trigger::RemoteTriggerTool),
            Arc::new(send_message::SendMessageTool { state: state.clone() }),
            Arc::new(ask_user::AskUserQuestionTool),
            Arc::new(brief::BriefTool),

            // ── Utility ────────────────────────────────────────────────
            Arc::new(config_tool::ConfigTool),
            Arc::new(repl::ReplTool),
            Arc::new(synthetic_output::SyntheticOutputTool),
            Arc::new(todo_write::TodoWriteTool),
            Arc::new(tool_search::ToolSearchTool),
            Arc::new(skill_tool::SkillTool),
            Arc::new(memory_recall::MemoryRecallTool),
            Arc::new(crewai_tool::CrewAITool),
            Arc::new(wiki_ingest::WikiIngestTool),
            Arc::new(wiki_query::WikiQueryTool),

            // ── IPC proxy tools (T8) ───────────────────────────────────
            Arc::new(mcp_tool::McpTool),
            Arc::new(mcp_auth_tool::McpAuthTool),
            Arc::new(mcp_list_resources::ListMcpResourcesTool),
            Arc::new(mcp_read_resource::ReadMcpResourceTool),
            Arc::new(lsp_tool::LspTool),
            Arc::new(web_fetch_tool::WebFetchTool),
            Arc::new(web_search_tool::WebSearchTool),
            Arc::new(notebook_edit_tool::NotebookEditTool),
        ];
        ToolRegistry { tools }
    }

    /// Create an empty registry.
    pub fn empty() -> Self {
        ToolRegistry { tools: Vec::new() }
    }

    /// Create a registry from an explicit tool list.
    pub fn from_tools(tools: Vec<Arc<dyn Tool>>) -> Self {
        ToolRegistry { tools }
    }

    /// Create a clone of the registry for concurrent tool execution.
    /// Since all tools are behind Arc, this is cheap — just clones the Arc pointers.
    pub fn clone_for_concurrent(&self) -> Self {
        ToolRegistry { tools: self.tools.clone() }
    }

    // ─── Dynamic composition (Principle 4) ──────────────────────────────

    /// Add a tool to the registry at runtime.
    /// Used for MCP tool discovery and custom agent tool sets.
    /// Returns false if a tool with the same name already exists.
    pub fn add_tool(&mut self, tool: Arc<dyn Tool>) -> bool {
        if self.tools.iter().any(|t| t.name() == tool.name()) {
            return false; // duplicate
        }
        self.tools.push(tool);
        true
    }

    /// Remove a tool by name. Returns true if found and removed.
    pub fn remove_tool(&mut self, name: &str) -> bool {
        let before = self.tools.len();
        self.tools.retain(|t| t.name() != name);
        self.tools.len() < before
    }

    /// Create a filtered copy of the registry containing only the named tools.
    /// Used for sub-agent tool restriction (e.g., read-only agent gets only
    /// FileRead, Grep, Glob).
    pub fn filter(&self, allowed_names: &[&str]) -> Self {
        ToolRegistry {
            tools: self.tools.iter()
                .filter(|t| allowed_names.contains(&t.name()))
                .cloned()
                .collect(),
        }
    }

    /// Create a filtered copy excluding the named tools.
    pub fn exclude(&self, excluded_names: &[&str]) -> Self {
        ToolRegistry {
            tools: self.tools.iter()
                .filter(|t| !excluded_names.contains(&t.name()))
                .cloned()
                .collect(),
        }
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    /// Whether the registry is empty.
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    // ─── Queries ────────────────────────────────────────────────────────

    /// Find a tool by name.
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Tool>> {
        self.tools.iter().find(|t| t.name() == name)
    }

    /// List all tool names.
    pub fn names(&self) -> Vec<&str> {
        self.tools.iter().map(|t| t.name()).collect()
    }

    /// Export tool definitions for the Anthropic API.
    pub fn api_definitions(&self) -> Vec<Value> {
        self.tools.iter().map(|t| {
            serde_json::json!({
                "name":         t.name(),
                "description":  t.description(),
                "input_schema": t.input_schema(),
            })
        }).collect()
    }

    /// Core tools always present in the prompt (sufficient for most tasks).
    pub const CORE_TOOL_NAMES: &'static [&'static str] = &[
        "FileRead", "FileWrite", "FileEdit", "Glob", "Grep",
        "Bash", "Agent", "AskUserQuestion", "TodoWrite", "ToolSearch",
        "MemoryRecall",
    ];

    /// Extended tools deferred until the model explicitly requests them via ToolSearch.
    /// This reduces system prompt token usage on every turn.
    pub fn core_only(&self) -> Self {
        self.filter(Self::CORE_TOOL_NAMES)
    }

    /// Return a filtered registry based on user message context.
    /// Heuristic: analyze first user message for keywords to determine likely tool needs.
    pub fn filter_for_context(&self, user_message: &str) -> Self {
        let msg = user_message.to_lowercase();

        // Always include core tools
        let mut wanted: Vec<&str> = Self::CORE_TOOL_NAMES.to_vec();

        // Add contextual tools based on keywords
        if msg.contains("commit") || msg.contains("git") || msg.contains("push") || msg.contains("branch") {
            // Git workflow — needs shell primarily (already in core via Bash)
        }
        if msg.contains("test") || msg.contains("debug") || msg.contains("run") {
            // Testing — Bash is in core, nothing extra needed
        }
        if msg.contains("plan") || msg.contains("architect") || msg.contains("design") {
            wanted.extend_from_slice(&["EnterPlanMode", "ExitPlanMode"]);
        }
        if msg.contains("team") || msg.contains("parallel") || msg.contains("swarm") || msg.contains("worker") {
            wanted.extend_from_slice(&["TeamCreate", "TeamDelete", "SendMessage",
                "TaskCreate", "TaskGet", "TaskList", "TaskUpdate", "TaskStop", "TaskOutput"]);
        }
        if msg.contains("worktree") || msg.contains("branch") || msg.contains("isolat") {
            wanted.extend_from_slice(&["EnterWorktree", "ExitWorktree"]);
        }
        if msg.contains("schedule") || msg.contains("cron") || msg.contains("recurring") {
            wanted.extend_from_slice(&["CronCreate", "CronSchedule"]);
        }
        if msg.contains("search") || msg.contains("web") || msg.contains("fetch") || msg.contains("url") {
            wanted.extend_from_slice(&["WebFetchTool", "WebSearchTool"]);
        }
        if msg.contains("notebook") || msg.contains("jupyter") || msg.contains(".ipynb") {
            wanted.extend_from_slice(&["NotebookEditTool"]);
        }
        if msg.contains("skill") {
            wanted.extend_from_slice(&["SkillTool"]);
        }
        if msg.contains("mcp") || msg.contains("server") {
            wanted.extend_from_slice(&["McpTool", "McpAuthTool", "ListMcpResources", "ReadMcpResource"]);
        }

        // ToolSearch is always included so the model can discover deferred tools
        self.filter(&wanted)
    }

    /// Track which tools were actually called. Returns names for usage statistics.
    pub fn record_tool_usage(tool_name: &str, usage_map: &mut std::collections::HashMap<String, u32>) {
        *usage_map.entry(tool_name.to_string()).or_insert(0) += 1;
    }

    /// Execute a tool after checking permissions.
    pub async fn execute_with_permission(
        &self,
        tool_name: &str,
        input: Value,
        gate: &PermissionGate,
        output_tx: tokio::sync::mpsc::Sender<ToolOutput>,
    ) -> Result<ToolResult> {
        let tool = self.get(tool_name)
            .ok_or_else(|| anyhow::anyhow!("Unknown tool: {tool_name}"))?;

        let input_json = input.to_string();

        if tool.requires_permission() {
            match gate.check(tool_name, &input_json) {
                GateDecision::Allow   => {}
                GateDecision::Deny(r) => return Ok(ToolResult::error(format!("Permission denied: {r}"))),
                GateDecision::PlanOnly => {
                    let desc = format!("[Plan mode] Would execute {tool_name} with: {input_json}");
                    let _ = output_tx.send(ToolOutput { text: desc.clone(), is_error: false }).await;
                    return Ok(ToolResult::ok(desc));
                }
            }
        }

        tool.execute(input, output_tx).await
    }
}
