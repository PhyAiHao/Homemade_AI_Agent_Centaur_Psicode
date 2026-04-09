//! QueryEngine — the main agentic loop manager.
//!
//! Mirrors `src/QueryEngine.ts`. Manages the session lifetime, builds
//! system prompts, maintains conversation history, and delegates to
//! the query loop for the streaming tool-use cycle.

use anyhow::Result;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::info;

use crate::cost_tracker::CostTracker;
use crate::ipc::IpcClient;
use crate::permissions::gate::PermissionGate;
use crate::session_history::SessionTranscript;
use crate::state::SharedState;
use crate::tools::ToolRegistry;

use crate::query::message::ConversationMessage;
use crate::query::query_loop::{self, QueryEvent, QueryLoopConfig};
use crate::query::stop_hooks::StopReason;

/// Configuration for a new QueryEngine session.
#[derive(Debug, Clone)]
pub struct QueryEngineConfig {
    /// Working directory.
    pub cwd: PathBuf,
    /// Model to use.
    pub model: String,
    /// Custom system prompt addition.
    pub custom_system_prompt: Option<String>,
    /// Max turns per conversation.
    pub max_turns: u32,
    /// Max USD budget.
    pub max_budget_usd: f64,
    /// Max output tokens per API call.
    pub max_output_tokens: u32,
    /// Context window size (for token budget).
    pub context_window: u64,
    /// Fallback model on repeated 529 (overloaded) errors.
    pub fallback_model: Option<String>,
    /// Enable streaming tool execution (concurrent read-only tools).
    pub streaming_tool_execution: bool,
    /// Optional structured output JSON schema.
    pub output_format: Option<serde_json::Value>,
    /// Enable prompt caching (cache_control markers).
    pub prompt_caching: bool,
    /// LLM provider: "first_party" (Anthropic), "openai", "gemini", "ollama".
    pub provider: String,
    /// API key to pass through to the Python brain for LLM calls.
    /// If None, the brain uses its own environment-loaded key.
    pub api_key: Option<String>,
}

impl Default for QueryEngineConfig {
    fn default() -> Self {
        QueryEngineConfig {
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            model: "claude-sonnet-4-6".to_string(),
            custom_system_prompt: None,
            max_turns: 30,
            max_budget_usd: 50.0,
            max_output_tokens: 16_384,
            context_window: 200_000,
            fallback_model: None,
            streaming_tool_execution: true,
            output_format: None,
            prompt_caching: true,
            provider: "first_party".to_string(),
            api_key: None,
        }
    }
}

/// Result of a full query session.
#[derive(Debug)]
pub struct QueryResult {
    /// Final assistant text (concatenated from all turns).
    pub response_text: String,
    /// Why the loop stopped.
    pub stop_reason: StopReason,
    /// Cumulative cost summary.
    pub cost_summary: String,
    /// Session ID.
    pub session_id: String,
}

/// The query engine — manages a single conversational session.
pub struct QueryEngine {
    config: QueryEngineConfig,
    /// Cumulative conversation messages.
    messages: Vec<ConversationMessage>,
    /// IPC client for talking to the Python brain.
    ipc: IpcClient,
    /// Tool registry.
    registry: ToolRegistry,
    /// Permission gate.
    gate: PermissionGate,
    /// Cost tracker.
    cost_tracker: CostTracker,
    /// Session transcript recorder.
    transcript: SessionTranscript,
    /// Shared application state.
    state: SharedState,
    /// Abort handle for cooperative cancellation.
    abort_handle: crate::query::abort::AbortHandle,
}

impl QueryEngine {
    /// Create a new QueryEngine session.
    pub async fn new(
        config: QueryEngineConfig,
        ipc: IpcClient,
        state: SharedState,
        gate: PermissionGate,
    ) -> Result<Self> {
        let registry = ToolRegistry::default_registry(state.clone());
        Self::with_registry(config, ipc, state, gate, registry).await
    }

    /// Create a QueryEngine with a pre-built ToolRegistry (e.g., with MCP tools added).
    pub async fn with_registry(
        config: QueryEngineConfig,
        ipc: IpcClient,
        state: SharedState,
        gate: PermissionGate,
        registry: ToolRegistry,
    ) -> Result<Self> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let transcript = SessionTranscript::new(&session_id).await?;

        info!(session_id = %session_id, model = %config.model, "QueryEngine initialized");

        let abort_handle = crate::query::abort::AbortHandle::new();

        Ok(QueryEngine {
            config,
            messages: Vec::new(),
            ipc,
            registry,
            gate,
            cost_tracker: CostTracker::new(),
            transcript,
            state,
            abort_handle,
        })
    }

    /// Update the model for subsequent queries (called when TUI wizard changes it).
    pub fn set_model(&mut self, model: &str) {
        if self.config.model != model {
            info!(old = %self.config.model, new = %model, "Model changed");
            self.config.model = model.to_string();
        }
    }

    /// Update the provider for subsequent queries (called when TUI wizard changes it).
    pub fn set_provider(&mut self, provider: &str) {
        if self.config.provider != provider {
            info!(old = %self.config.provider, new = %provider, "Provider changed");
            self.config.provider = provider.to_string();
        }
    }

    /// Submit a user message and run the agentic loop to completion.
    ///
    /// Returns a stream of events via the `event_tx` channel and
    /// the final result when the loop completes.
    pub async fn submit_message(
        &mut self,
        user_prompt: &str,
        event_tx: mpsc::Sender<QueryEvent>,
    ) -> Result<QueryResult> {
        // ── Add user message to history ─────────────────────────────────
        let user_msg = ConversationMessage::user_text(user_prompt);
        self.transcript.record_message(&user_msg).await.ok();
        self.messages.push(user_msg);

        // ── Build system prompt ─────────────────────────────────────────
        let system_prompt = self.build_system_prompt().await;

        // ── Configure the query loop ────────────────────────────────────
        let loop_config = QueryLoopConfig {
            model: self.config.model.clone(),
            system_prompt,
            max_turns: self.config.max_turns,
            max_budget_usd: self.config.max_budget_usd,
            max_output_tokens: self.config.max_output_tokens,
            stop_hooks: Vec::new(),
            token_budget: self.config.context_window,
            fallback_model: self.config.fallback_model.clone(),
            streaming_tool_execution: self.config.streaming_tool_execution,
            output_format: self.config.output_format.clone(),
            prompt_caching: self.config.prompt_caching,
            abort_handle: self.abort_handle.clone(),
            provider: self.config.provider.clone(),
            api_key: self.config.api_key.clone(),
        };

        // ── Run the loop ────────────────────────────────────────────────
        let stop_reason = query_loop::run_query_loop(
            loop_config,
            &mut self.messages,
            &mut self.ipc,
            &self.registry,
            &self.gate,
            &mut self.cost_tracker,
            event_tx,
        ).await?;

        // ── Extract final response text ─────────────────────────────────
        let response_text = self.messages.iter()
            .rev()
            .find(|m| m.role == crate::query::message::Role::Assistant)
            .map(|m| m.text_content())
            .unwrap_or_default();

        // ── Record to transcript ────────────────────────────────────────
        if let Some(last_assistant) = self.messages.last() {
            self.transcript.record_message(last_assistant).await.ok();
        }

        // ── Record to prompt history ────────────────────────────────────
        crate::history::add_to_history(&crate::history::HistoryEntry {
            display: user_prompt.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            project: self.config.cwd.to_string_lossy().to_string(),
            session_id: Some(self.transcript.session_id().to_string()),
            pasted_contents: Vec::new(),
        }).await.ok();

        info!(
            session  = %self.session_id(),
            turns    = self.message_count(),
            stop     = ?stop_reason,
            cost     = %self.cost_tracker().summary(),
            "Query session complete"
        );

        Ok(QueryResult {
            response_text,
            stop_reason,
            cost_summary: self.cost_tracker().summary(),
            session_id: self.session_id().to_string(),
        })
    }

    /// Run a single prompt in headless mode (no TUI).
    ///
    /// Used by `agent --bare "prompt"` and the SDK entrypoint.
    pub async fn run_single(
        config: QueryEngineConfig,
        prompt: &str,
        ipc: IpcClient,
        state: SharedState,
        gate: PermissionGate,
    ) -> Result<QueryResult> {
        let (event_tx, mut event_rx) = mpsc::channel::<QueryEvent>(256);

        let mut engine = Self::new(config, ipc, state, gate).await?;

        // Wire Ctrl-C to the engine's abort handle so headless runs are interruptible
        let abort = engine.abort_handle().clone();
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            abort.abort(crate::query::abort::AbortReason::UserInterrupt);
        });

        // Spawn a task to print events in headless mode
        let print_handle = tokio::spawn(async move {
            let mut full_response = String::new();
            while let Some(event) = event_rx.recv().await {
                match event {
                    QueryEvent::TextDelta(text) => {
                        print!("{text}");
                        full_response.push_str(&text);
                    }
                    QueryEvent::ToolStart { name, .. } => {
                        eprintln!("[tool: {name}]");
                    }
                    QueryEvent::Done(reason) => {
                        if reason.is_error() {
                            eprintln!("\n[{}]", reason.display());
                        }
                    }
                    QueryEvent::Error(e) => {
                        eprintln!("\n[Error: {e}]");
                    }
                    _ => {}
                }
            }
            full_response
        });

        let result = engine.submit_message(prompt, event_tx).await?;
        let _ = print_handle.await;

        println!(); // final newline

        // Log the completed session summary using all QueryResult fields
        eprintln!(
            "[session {}] stop={:?}  cost={}",
            result.session_id,
            result.stop_reason,
            result.cost_summary,
        );
        // response_text is available for SDK callers that need the final text
        tracing::debug!(chars = result.response_text.len(), "Response text length");

        Ok(result)
    }

    /// Build the full system prompt from config + context.
    async fn build_system_prompt(&self) -> String {
        let mut parts = Vec::new();

        // Collect rich system and user context from context.rs
        let sys_ctx = crate::context::collect_system_context(&self.config.cwd).await;
        let user_ctx = crate::context::collect_user_context(&self.config.cwd).await;

        // Read plan-mode flag from shared state so the prompt reflects current mode
        let plan_mode = self.state.read().await.plan_mode;
        if plan_mode {
            parts.push("You are currently in PLAN MODE. Describe what you would do but do not execute any tools that modify files or run commands.".to_string());
        }

        // Base identity
        parts.push("You are an AI agent for software engineering tasks.".to_string());

        // System context (cwd, platform, shell, date, git info)
        parts.push(crate::context::format_system_context(&sys_ctx));

        // Model info
        parts.push(format!("Model: {}", self.config.model));

        // Tool list
        let tool_names = self.registry.names();
        parts.push(format!(
            "Available tools: {}",
            tool_names.join(", ")
        ));

        // User context (CLAUDE.md content)
        let user_ctx_str = crate::context::format_user_context(&user_ctx);
        if !user_ctx_str.is_empty() {
            parts.push(user_ctx_str);
        }

        // Custom system prompt
        if let Some(ref custom) = self.config.custom_system_prompt {
            if !custom.is_empty() {
                parts.push(custom.clone());
            }
        }

        // M5: Inject MEMORY.md content into system prompt
        if let Some(memory_prompt) = crate::query::memory_integration::load_memory_prompt(
            &self.config.cwd
        ) {
            parts.push(memory_prompt);
        }

        parts.join("\n\n")
    }

    // ─── Accessors ──────────────────────────────────────────────────────

    pub fn session_id(&self) -> &str {
        self.transcript.session_id()
    }

    pub fn cost_tracker(&self) -> &CostTracker {
        &self.cost_tracker
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the abort handle for this engine.
    /// The TUI uses this to trigger cancellation on Ctrl-C.
    pub fn abort_handle(&self) -> &crate::query::abort::AbortHandle {
        &self.abort_handle
    }
}
