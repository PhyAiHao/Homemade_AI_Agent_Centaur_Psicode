//! Query loop — the core streaming tool-use loop.
//!
//! Mirrors the `queryLoop()` function in `src/query.ts`.
//!
//! The loop cycle:
//!   1. Microcompact old tool results if token budget is getting tight
//!   2. Send messages to the API via IPC (`api_request`) with prompt cache markers
//!   3. Stream `text_delta` and `tool_use` blocks from the API
//!   4. On `tool_use`: execute tools concurrently (safe) or sequentially (unsafe)
//!   5. Inject `tool_result` messages and loop back to step 1
//!   6. Stop when `message_done` arrives with no pending tool calls
//!   7. On API errors: retry with exponential backoff, fallback model on repeated 529s

use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::cost_tracker::{ApiUsage, CostTracker};
use crate::ipc::{IpcClient, IpcMessage};
use crate::permissions::gate::PermissionGate;
use crate::tools::{ToolOutput, ToolRegistry, ToolResult as TResult};

use super::abort::{self, AbortHandle};
use super::compact;
use super::message::{ConversationMessage, ToolResultBlock, ToolUseBlock};
use super::prompt_cache::{self, CacheBreakDetector};
use super::retry::{self};
use super::stop_hooks::{self, StopHook, StopReason};
use super::token_budget::{BudgetDecision, BudgetTracker};
use super::tool_result_budget;

// ─── Events ─────────────────────────────────────────────────────────────────

/// Events emitted by the query loop for consumption by the TUI / caller.
#[derive(Debug, Clone)]
pub enum QueryEvent {
    /// Streaming text delta from the assistant.
    TextDelta(String),
    /// Thinking block content (chain-of-thought).
    ThinkingDelta(String),
    /// Tool execution started.
    ToolStart { id: String, name: String },
    /// Tool streaming output.
    ToolOutput(ToolOutput),
    /// Tool execution completed.
    ToolDone { id: String, result: TResult },
    /// Full assistant message completed.
    AssistantMessage(ConversationMessage),
    /// Cost/usage update.
    UsageUpdate { input_tokens: u64, output_tokens: u64, cost_usd: f64 },
    /// Loop ended.
    Done(StopReason),
    /// Error.
    Error(String),
    /// Retry status (shown in TUI during backoff waits).
    RetryWait { attempt: u32, max: u32, delay_ms: u64, reason: String },
    /// Context was auto-compacted.
    Compacted { messages_before: usize, messages_after: usize },
}

// ─── Config ─────────────────────────────────────────────────────────────────

/// Configuration for the query loop.
#[derive(Debug, Clone)]
pub struct QueryLoopConfig {
    pub model: String,
    pub system_prompt: String,
    pub max_turns: u32,
    pub max_budget_usd: f64,
    pub max_output_tokens: u32,
    pub stop_hooks: Vec<StopHook>,
    pub token_budget: u64,
    /// Fallback model to use after repeated 529 errors.
    pub fallback_model: Option<String>,
    /// Whether to enable streaming tool execution (tools start during streaming).
    pub streaming_tool_execution: bool,
    /// Optional structured output JSON schema.
    pub output_format: Option<Value>,
    /// Whether to enable prompt caching.
    pub prompt_caching: bool,
    /// Abort handle for cooperative cancellation.
    pub abort_handle: AbortHandle,
    /// LLM provider: "first_party", "openai", "gemini", "ollama".
    pub provider: String,
    /// API key to pass through to the Python brain. If None, the brain
    /// uses its own environment-loaded key. Set this when the caller
    /// has a key that the brain process might not have (e.g., IDE bridge).
    pub api_key: Option<String>,
}

/// Maximum consecutive turns where the same tool is called before injecting
/// a "you seem stuck" nudge. Prevents infinite WebSearch/Grep loops.
const MAX_REPEATED_TOOL_TURNS: u32 = 5;

impl Default for QueryLoopConfig {
    fn default() -> Self {
        QueryLoopConfig {
            model: "claude-sonnet-4-6".to_string(),
            system_prompt: String::new(),
            max_turns: 30,
            max_budget_usd: 50.0,
            max_output_tokens: 16_384,
            stop_hooks: Vec::new(),
            token_budget: 200_000,
            fallback_model: None,
            streaming_tool_execution: true,
            output_format: None,
            prompt_caching: true,
            abort_handle: AbortHandle::new(),
            provider: "first_party".to_string(),
            api_key: None,
        }
    }
}




// ─── Auto-compact via IPC ───────────────────────────────────────────────────

/// Request the Python brain to summarize and compact the conversation.
async fn request_compact(
    ipc: &mut IpcClient,
    messages: &[ConversationMessage],
    token_budget: Option<u32>,
) -> Result<Option<(String, Vec<Value>)>> {
    let messages_json: Vec<Value> = messages.iter().map(|m| {
        json!({ "role": m.role, "content": m.content })
    }).collect();

    let request = IpcMessage::CompactRequest(crate::ipc::CompactRequest {
        request_id: IpcClient::new_request_id(),
        messages: messages_json,
        token_budget,
    });

    match ipc.request(request).await {
        Ok(IpcMessage::CompactResponse(resp)) => {
            if resp.summary.is_empty() && resp.messages.is_empty() {
                Ok(None)
            } else {
                Ok(Some((resp.summary, resp.messages)))
            }
        }
        Ok(_) => Ok(None),
        Err(e) => {
            warn!(error = %e, "Compact request failed");
            Ok(None)
        }
    }
}


// ─── Streaming tool execution ───────────────────────────────────────────────

/// Tools that are safe to execute concurrently (read-only).
const CONCURRENCY_SAFE_TOOLS: &[&str] = &[
    "FileRead", "Glob", "Grep", "WebSearch", "WebFetch", "ToolSearch", "TaskGet",
    "TaskList", "TaskOutput", "Brief", "Sleep",
];

fn is_concurrency_safe(tool_name: &str) -> bool {
    CONCURRENCY_SAFE_TOOLS.contains(&tool_name)
}

/// Execute tool calls, using concurrent execution for safe tools.
async fn execute_tools(
    tool_use_blocks: &[ToolUseBlock],
    registry: &ToolRegistry,
    gate: &PermissionGate,
    event_tx: &mpsc::Sender<QueryEvent>,
    streaming: bool,
) -> Vec<ToolResultBlock> {
    let mut results: Vec<ToolResultBlock> = Vec::with_capacity(tool_use_blocks.len());

    if !streaming || tool_use_blocks.len() <= 1 {
        // Sequential execution (original behavior)
        for tool_block in tool_use_blocks {
            let result = execute_single_tool(tool_block, registry, gate, event_tx).await;
            results.push(result);
        }
        return results;
    }

    // Check if ALL tools are concurrency-safe
    let all_safe = tool_use_blocks.iter().all(|t| is_concurrency_safe(&t.name));

    if all_safe {
        // Execute all concurrently
        let mut handles = Vec::new();
        for tool_block in tool_use_blocks {
            let tb = tool_block.clone();
            let tool_id = tool_block.id.clone(); // capture ID before move
            let reg = registry.clone_for_concurrent();
            let g = gate.clone();
            let tx = event_tx.clone();
            handles.push((tool_id, tokio::spawn(async move {
                execute_single_tool(&tb, &reg, &g, &tx).await
            })));
        }
        for (tool_id, handle) in handles {
            match handle.await {
                Ok(result) => results.push(result),
                Err(e) => results.push(ToolResultBlock {
                    tool_use_id: tool_id,
                    content: format!("Tool execution panic: {e}"),
                    is_error: true,
                }),
            }
        }
    } else {
        // Mixed: execute safe tools concurrently, unsafe ones sequentially
        // Group: find contiguous safe blocks vs unsafe
        for tool_block in tool_use_blocks {
            let result = execute_single_tool(tool_block, registry, gate, event_tx).await;
            results.push(result);
        }
    }

    results
}

/// Execute a single tool with event emission.
async fn execute_single_tool(
    tool_block: &ToolUseBlock,
    registry: &ToolRegistry,
    gate: &PermissionGate,
    event_tx: &mpsc::Sender<QueryEvent>,
) -> ToolResultBlock {
    let _ = event_tx.send(QueryEvent::ToolStart {
        id: tool_block.id.clone(),
        name: tool_block.name.clone(),
    }).await;

    let (tool_tx, mut tool_rx) = mpsc::channel::<ToolOutput>(64);

    let event_tx_clone = event_tx.clone();
    let forward_handle = tokio::spawn(async move {
        while let Some(output) = tool_rx.recv().await {
            let _ = event_tx_clone.send(QueryEvent::ToolOutput(output)).await;
        }
    });

    let result = registry.execute_with_permission(
        &tool_block.name,
        tool_block.input.clone(),
        gate,
        tool_tx,
    ).await.unwrap_or_else(|e| {
        TResult::error(format!("Tool execution error: {e}"))
    });

    let _ = forward_handle.await;

    let _ = event_tx.send(QueryEvent::ToolDone {
        id: tool_block.id.clone(),
        result: result.clone(),
    }).await;

    ToolResultBlock {
        tool_use_id: tool_block.id.clone(),
        content: result.content,
        is_error: result.is_error,
    }
}

// ─── Main query loop ────────────────────────────────────────────────────────

/// Run the streaming tool-use query loop.
///
/// This is the heart of the agent — the agentic loop that sends messages
/// to the LLM via IPC, streams responses, executes tools, and loops
/// until completion.
///
/// Features:
/// - Microcompact: clears old tool results when context gets tight
/// - Auto-compact: summarizes conversation via IPC when near context limit
/// - Prompt caching: adds cache_control markers to reduce API cost
/// - Retry/fallback: exponential backoff on errors, fallback model on 529s
/// - Streaming tool execution: concurrent execution of read-only tools
/// - Structured output: pass-through JSON schema for API-level enforcement
pub async fn run_query_loop(
    config: QueryLoopConfig,
    messages: &mut Vec<ConversationMessage>,
    ipc: &mut IpcClient,
    registry: &ToolRegistry,
    gate: &PermissionGate,
    cost_tracker: &mut CostTracker,
    event_tx: mpsc::Sender<QueryEvent>,
) -> Result<StopReason> {
    let mut turn_count: u32 = 0;
    let mut budget_tracker = BudgetTracker::new(config.token_budget);
    let mut current_model = config.model.clone();
    let mut consecutive_529s: u32 = 0;
    let mut compact_tracking = compact::AutoCompactTracking::default();
    let mut max_output_tokens_recovery_count: u32 = 0;
    let mut current_max_output_tokens = config.max_output_tokens;
    let cache_break_detector = CacheBreakDetector::new();
    let mut tool_budget_state = tool_result_budget::ToolResultBudgetState::default();
    let skip_budget_tools: std::collections::HashSet<String> = std::collections::HashSet::new();
    let session_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join(".agent-sessions");
    let mut session_memory_state = super::memory_integration::SessionMemoryState::new();
    let mut memory_prefetch_result: Option<String> = None;
    let mut repeated_tool_counter: std::collections::HashMap<String, u32> = std::collections::HashMap::new();

    // ── Deferred tool loading ──────────────────────────────────────
    // Start with core-only tools in the API request. filter_for_context()
    // adds keyword-matched tools from the first user message. The FULL
    // registry is kept for execution — so if the LLM calls a deferred
    // tool (discovered via ToolSearch), it still works.
    let first_user_msg = messages.iter()
        .find(|m| m.role == super::message::Role::User)
        .map(|m| m.text_content())
        .unwrap_or_default();
    let api_registry = registry.filter_for_context(&first_user_msg);
    info!(
        core = api_registry.len(),
        total = registry.len(),
        deferred = registry.len() - api_registry.len(),
        "Tool loading: {} in API, {} deferred via ToolSearch",
        api_registry.len(), registry.len() - api_registry.len(),
    );

    // Auto-compact threshold — accounts for system prompt size so compaction
    // triggers before the prompt + messages exceed the context window.
    let autocompact_threshold = compact::auto_compact_threshold_with_prompt(
        config.token_budget, config.max_output_tokens,
        config.system_prompt.len(),
    );

    loop {
        turn_count += 1;

        // ── Check turn limit ────────────────────────────────────────────
        if turn_count > config.max_turns {
            let reason = StopReason::MaxTurns(config.max_turns);
            let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
            return Ok(reason);
        }

        // ── Check USD budget ────────────────────────────────────────────
        if cost_tracker.exceeds_budget(config.max_budget_usd) {
            let reason = StopReason::MaxBudget(config.max_budget_usd);
            let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
            return Ok(reason);
        }

        // ── M4: Relevant memory prefetch (once, on first turn) ──────────
        if turn_count == 1 && memory_prefetch_result.is_none() {
            // Get the user's latest message for context-aware recall
            let user_msg = messages.iter().rev()
                .find(|m| m.role == super::message::Role::User)
                .map(|m| m.text_content())
                .unwrap_or_default();

            if !user_msg.is_empty() {
                if let Some(memories) = super::memory_integration::prefetch_relevant_memories(
                    ipc, &user_msg
                ).await {
                    memory_prefetch_result = Some(memories);
                    debug!("Relevant memories prefetched for this turn");
                }
            }
        }

        // ── Microcompact: clear old tool results ────────────────────────
        let estimated_tokens = compact::estimate_tokens(messages);
        if estimated_tokens > autocompact_threshold / 2 {
            compact::microcompact(messages, compact::MICROCOMPACT_KEEP_RECENT);
        }

        // ── M3: Try session-memory compact FIRST (zero LLM calls) ────────
        let mut compacted_this_turn = false;
        if estimated_tokens > autocompact_threshold && compact_tracking.consecutive_failures < 3 {
            info!(tokens = estimated_tokens, threshold = autocompact_threshold,
                  "Token budget tight — trying session memory compact first");

            if let Some((summary, kept)) = super::memory_integration::try_session_memory_compact(
                ipc, messages, (autocompact_threshold / 2) as u32,
            ).await {
                let msgs_before = messages.len();
                messages.clear();
                messages.push(ConversationMessage::user_text(format!(
                    "[Context compacted via session memory. Summary:\n{summary}\n]"
                )));
                for msg_val in kept {
                    if let (Some(role), Some(content)) = (
                        msg_val.get("role").and_then(|r| r.as_str()),
                        msg_val.get("content").cloned(),
                    ) {
                        messages.push(ConversationMessage {
                            role: if role == "assistant" {
                                super::message::Role::Assistant
                            } else {
                                super::message::Role::User
                            },
                            content,
                        });
                    }
                }
                let _ = event_tx.send(QueryEvent::Compacted {
                    messages_before: msgs_before,
                    messages_after: messages.len(),
                }).await;
                info!(before = msgs_before, after = messages.len(),
                      "Session memory compact complete (zero LLM calls)");
                compact_tracking.consecutive_failures = 0;
                compacted_this_turn = true;
            }
        }

        // ── Auto-compact FALLBACK: full LLM summarization ──────────────
        let estimated_tokens_after = compact::estimate_tokens(messages);
        if !compacted_this_turn && estimated_tokens_after > autocompact_threshold
            && compact_tracking.consecutive_failures < 3
        {
            info!(tokens = estimated_tokens_after, threshold = autocompact_threshold,
                  "Session memory compact unavailable — requesting full auto-compact");
            let msgs_before = messages.len();
            match request_compact(ipc, messages, Some(autocompact_threshold as u32 / 2)).await {
                Ok(Some((summary, compacted_msgs))) => {
                    // Replace messages with compacted version
                    messages.clear();
                    // Add boundary marker
                    messages.push(ConversationMessage::user_text(format!(
                        "[Context was auto-compacted. Summary of prior conversation:\n{summary}\n]"
                    )));
                    // Re-add compacted messages from Python
                    for msg_val in compacted_msgs {
                        if let (Some(role), Some(content)) = (
                            msg_val.get("role").and_then(|r| r.as_str()),
                            msg_val.get("content").cloned(),
                        ) {
                            messages.push(ConversationMessage {
                                role: if role == "assistant" {
                                    super::message::Role::Assistant
                                } else {
                                    super::message::Role::User
                                },
                                content,
                            });
                        }
                    }
                    let msgs_after = messages.len();
                    let _ = event_tx.send(QueryEvent::Compacted {
                        messages_before: msgs_before,
                        messages_after: msgs_after,
                    }).await;
                    info!(before = msgs_before, after = msgs_after, "Auto-compact complete");
                    compact_tracking.consecutive_failures = 0;
                }
                Ok(None) => {
                    compact_tracking.consecutive_failures += 1;
                    warn!(failures = compact_tracking.consecutive_failures, "Auto-compact returned no result");
                }
                Err(e) => {
                    compact_tracking.consecutive_failures += 1;
                    warn!(error = %e, failures = compact_tracking.consecutive_failures, "Auto-compact failed");
                }
            }
        }

        // ── Normalize messages (thinking blocks, empty content) ────────
        super::message::normalize_messages_for_api(messages);

        // ── Build API request (only send filtered tools to save tokens) ──
        let tools_json = api_registry.api_definitions();
        let mut messages_json: Vec<Value> = messages.iter().map(|m| {
            json!({
                "role": m.role,
                "content": m.content,
            })
        }).collect();

        // ── M4: Inject prefetched memories as context ────────────────────
        if let Some(ref memories) = memory_prefetch_result {
            // Inject as the first user message (before the conversation)
            if turn_count == 1 {
                messages_json.insert(0, json!({
                    "role": "user",
                    "content": [{
                        "type": "text",
                        "text": format!("[Relevant memories from previous conversations:\n{memories}\n]")
                    }]
                }));
                // Add an assistant acknowledgment to maintain alternation
                messages_json.insert(1, json!({
                    "role": "assistant",
                    "content": [{ "type": "text", "text": "I'll keep these memories in mind." }]
                }));
            }
        }

        // ── Prompt caching: add cache_control markers ───────────────────
        let cache_config = prompt_cache::PromptCacheConfig::default();
        if config.prompt_caching && !messages_json.is_empty() {
            prompt_cache::add_message_cache_markers(&mut messages_json, &cache_config);
        }

        // ── Build system prompt with cache_control ──────────────────────
        let system_prompt_value = if config.prompt_caching {
            Some(prompt_cache::build_system_prompt_blocks(&config.system_prompt, &cache_config))
        } else {
            Some(Value::String(config.system_prompt.clone()))
        };

        // ── Structured output: add output_format + beta header ──────────
        let mut betas: Vec<String> = vec![];
        let mut metadata: HashMap<String, Value> = HashMap::new();
        if let Some(ref output_format) = config.output_format {
            betas.push("structured-outputs-2025-01-24".to_string());
            metadata.insert("output_format".to_string(), output_format.clone());
        }

        debug!(turn = turn_count, model = %current_model, "Sending API request via IPC");

        // ── Send to IPC and stream response (with retry) ────────────────
        let api_start = std::time::Instant::now();
        let request_id = IpcClient::new_request_id();

        let mut retry_attempt: u32 = 0;
        // Declare here; every branch of 'retry initialises these before reading.
        let (mut assistant_text, mut tool_use_blocks, mut raw_content_blocks, mut api_usage);
        let mut stream_stop_reason: Option<String>;

        'retry: loop {
            assistant_text = String::new();
            tool_use_blocks = Vec::new();
            raw_content_blocks = Vec::new();
            stream_stop_reason = None;
            api_usage = ApiUsage::default();

            // Record prompt state for cache break detection
            let cache_snapshot = prompt_cache::PromptStateSnapshot::new(
                &config.system_prompt, &tools_json, &current_model, false,
            );
            cache_break_detector.record_state(cache_snapshot);

            let stream_result = ipc.send_streaming(IpcMessage::ApiRequest(crate::ipc::ApiRequest {
                request_id: request_id.clone(),
                model: current_model.clone(),
                messages: messages_json.clone(),
                tools: tools_json.clone(),
                system_prompt: system_prompt_value.clone(),
                max_output_tokens: Some(current_max_output_tokens),
                metadata: metadata.clone(),
                tool_choice: None,
                thinking: None,
                betas: betas.clone(),
                provider: config.provider.clone(),
                api_key: config.api_key.clone(),
                base_url: None,
                fast_mode: false,
            })).await;

            let mut stream = match stream_result {
                Ok(s) => s,
                Err(e) => {
                    retry_attempt += 1;
                    if retry_attempt > retry::DEFAULT_MAX_RETRIES {
                        let reason = StopReason::ApiError(format!(
                            "IPC connection failed after {} retries: {e}", retry::DEFAULT_MAX_RETRIES
                        ));
                        let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                        return Ok(reason);
                    }
                    let delay = retry::compute_delay(retry_attempt, false);
                    let _ = event_tx.send(QueryEvent::RetryWait {
                        attempt: retry_attempt, max: retry::DEFAULT_MAX_RETRIES,
                        delay_ms: delay.as_millis() as u64,
                        reason: format!("IPC error: {e}"),
                    }).await;
                    tokio::time::sleep(delay).await;
                    continue 'retry;
                }
            };

            // Process streaming messages (with abort check)
            let mut stream_error: Option<String> = None;

            loop {
                let msg_result = tokio::select! {
                    msg = stream.recv() => {
                        match msg {
                            Some(m) => m,
                            None => break, // stream closed
                        }
                    }
                    _ = config.abort_handle.cancelled() => {
                        stream_error = Some("Aborted by user".to_string());
                        break;
                    }
                };
                match msg_result {
                    Ok(IpcMessage::TextDelta(td)) => {
                        assistant_text.push_str(&td.delta);
                        let _ = event_tx.send(QueryEvent::TextDelta(td.delta)).await;
                    }
                    Ok(IpcMessage::ToolUse(tu)) => {
                        debug!(tool = %tu.name, id = %tu.tool_call_id, "Received tool_use");
                        tool_use_blocks.push(ToolUseBlock {
                            id: tu.tool_call_id.clone(),
                            name: tu.name.clone(),
                            input: tu.input.clone(),
                        });
                        raw_content_blocks.push(json!({
                            "type": "tool_use",
                            "id": tu.tool_call_id,
                            "name": tu.name,
                            "input": tu.input,
                        }));
                    }
                    Ok(IpcMessage::MessageDone(md)) => {
                        api_usage.input_tokens = md.usage.get("input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0);
                        api_usage.output_tokens = md.usage.get("output_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0);
                        api_usage.cache_read_input_tokens = md.usage.get("cache_read_input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0);
                        api_usage.cache_creation_input_tokens = md.usage.get("cache_creation_input_tokens")
                            .and_then(|v| v.as_u64()).unwrap_or(0);
                        stream_stop_reason = md.stop_reason.clone();
                        break;
                    }
                    Ok(_) => { /* ignore other message types */ }
                    Err(e) => {
                        stream_error = Some(e.to_string());
                        break;
                    }
                }
            }

            // ── Cache break detection ───────────────────────────────────
            if api_usage.cache_read_input_tokens > 0 || api_usage.cache_creation_input_tokens > 0 {
                if let Some(cause) = cache_break_detector.check_response(
                    api_usage.cache_read_input_tokens,
                    api_usage.cache_creation_input_tokens,
                    &prompt_cache::PromptStateSnapshot::new(&config.system_prompt, &tools_json, &current_model, false),
                ) {
                    warn!("Cache break detected: {cause}");
                }
            }

            // ── Handle errors with retry/fallback ───────────────────────
            if let Some(ref err_msg) = stream_error {
                // Check for prompt_too_long → reactive compact
                if matches!(retry::classify_error(err_msg, &stream_stop_reason), retry::RetryDecision::ContextOverflow{..}) && compact_tracking.consecutive_failures < 3 {
                    warn!("Prompt too long — attempting reactive compact");
                    if let Ok(Some((summary, compacted))) = request_compact(
                        ipc, messages, Some(autocompact_threshold as u32 / 2)
                    ).await {
                        let msgs_before = messages.len();
                        messages.clear();
                        messages.push(ConversationMessage::user_text(format!(
                            "[Context reactively compacted. Summary:\n{summary}\n]"
                        )));
                        for msg_val in compacted {
                            if let (Some(role), Some(content)) = (
                                msg_val.get("role").and_then(|r| r.as_str()),
                                msg_val.get("content").cloned(),
                            ) {
                                messages.push(ConversationMessage {
                                    role: if role == "assistant" {
                                        super::message::Role::Assistant
                                    } else {
                                        super::message::Role::User
                                    },
                                    content,
                                });
                            }
                        }
                        let _ = event_tx.send(QueryEvent::Compacted {
                            messages_before: msgs_before,
                            messages_after: messages.len(),
                        }).await;
                        // Rebuild messages_json from new messages
                        messages_json = messages.iter().map(|m| {
                            json!({ "role": m.role, "content": m.content })
                        }).collect();
                        if config.prompt_caching && !messages_json.is_empty() {
                            prompt_cache::add_message_cache_markers(&mut messages_json, &prompt_cache::PromptCacheConfig::default());
                        }
                        retry_attempt += 1;
                        continue 'retry;
                    }
                }

                // ── 401 OAuth refresh attempt ────────────────────────────
                if (err_msg.contains("401") || err_msg.to_lowercase().contains("unauthorized"))
                    && retry_attempt == 0
                {
                    if let Ok(Some(tokens)) = crate::auth::get_oauth_tokens() {
                        if let Some(ref refresh) = tokens.refresh_token {
                            warn!("401 error detected — attempting OAuth token refresh");
                            match crate::auth::refresh_oauth_token(
                                "https://console.anthropic.com/v1/oauth/token",
                                "centaur-psicode",
                                refresh,
                            ).await {
                                Ok(_) => {
                                    retry_attempt += 1;
                                    continue 'retry;
                                }
                                Err(e) => {
                                    // Offline or refresh failed — don't retry, fall through
                                    warn!("OAuth refresh failed (offline?): {e}");
                                }
                            }
                        }
                    }
                }

                if retry::classify_error(err_msg, &stream_stop_reason) != retry::RetryDecision::Fatal {
                    // Track 529 for fallback
                    if matches!(retry::classify_error(err_msg, &stream_stop_reason), retry::RetryDecision::Overloaded) {
                        consecutive_529s += 1;
                        if consecutive_529s >= retry::MAX_529_BEFORE_FALLBACK {
                            if let Some(ref fallback) = config.fallback_model {
                                warn!(
                                    consecutive_529s,
                                    fallback = %fallback,
                                    "Switching to fallback model after repeated overloaded errors"
                                );
                                current_model = fallback.clone();
                                consecutive_529s = 0;
                                // Strip thinking blocks — signatures are model-bound
                                super::message::strip_all_thinking(messages);
                                continue 'retry;
                            }
                        }
                    }

                    retry_attempt += 1;
                    if retry_attempt > retry::DEFAULT_MAX_RETRIES {
                        let reason = StopReason::ApiError(format!(
                            "API failed after {} retries: {err_msg}", retry::DEFAULT_MAX_RETRIES
                        ));
                        let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                        return Ok(reason);
                    }
                    let delay = retry::compute_delay(retry_attempt, false);
                    let _ = event_tx.send(QueryEvent::RetryWait {
                        attempt: retry_attempt, max: retry::DEFAULT_MAX_RETRIES,
                        delay_ms: delay.as_millis() as u64,
                        reason: err_msg.clone(),
                    }).await;
                    warn!(attempt = retry_attempt, delay_ms = delay.as_millis(), "Retrying after error");
                    tokio::time::sleep(delay).await;
                    continue 'retry;
                }

                // Non-retryable error — inform TUI then signal loop done
                let reason = StopReason::ApiError(err_msg.clone());
                let _ = event_tx.send(QueryEvent::Error(err_msg.clone())).await;
                let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                return Ok(reason);
            }

            // Success — reset retry/529 counters
            consecutive_529s = 0;
            break 'retry;
        }

        let api_duration_ms = api_start.elapsed().as_millis() as u64;

        // ── Record cost ─────────────────────────────────────────────────
        cost_tracker.record(&current_model, api_usage.clone(), api_duration_ms);
        let _ = event_tx.send(QueryEvent::UsageUpdate {
            input_tokens: cost_tracker.total_input_tokens(),
            output_tokens: cost_tracker.total_output_tokens(),
            cost_usd: cost_tracker.total_cost(),
        }).await;

        // ── M2: Session memory update (post-sampling hook) ──────────────
        // After each model response, update session memory if thresholds met.
        let current_total_tokens = cost_tracker.total_input_tokens() + cost_tracker.total_output_tokens();
        let has_tool_calls_this_turn = !tool_use_blocks.is_empty();
        if session_memory_state.should_extract(current_total_tokens, has_tool_calls_this_turn) {
            debug!("Session memory update threshold met — updating");
            // Fire-and-forget: don't block the main loop
            let msgs = messages.clone();
            let socket = std::env::var("AGENT_IPC_SOCKET")
                .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
            session_memory_state.extraction_in_progress = true;
            let tokens_snapshot = current_total_tokens;
            tokio::spawn(async move {
                if let Ok(mut mem_ipc) = crate::ipc::IpcClient::connect_to(&socket).await {
                    super::memory_integration::update_session_memory(&mut mem_ipc, &msgs).await;
                }
            });
            session_memory_state.record_extraction(tokens_snapshot);
        }

        // ── Build assistant message ─────────────────────────────────────
        if !assistant_text.is_empty() {
            raw_content_blocks.insert(0, json!({
                "type": "text",
                "text": assistant_text,
            }));
        }

        let assistant_msg = ConversationMessage {
            role: super::message::Role::Assistant,
            content: Value::Array(raw_content_blocks),
        };

        // Emit ThinkingDelta events for any chain-of-thought blocks the model produced.
        // Extended thinking content arrives as {"type":"thinking","thinking":"..."} blocks.
        if let Value::Array(ref blocks) = assistant_msg.content {
            for block in blocks {
                if block.get("type").and_then(|t| t.as_str()) == Some("thinking") {
                    if let Some(text) = block.get("thinking").and_then(|t| t.as_str()) {
                        let _ = event_tx.send(QueryEvent::ThinkingDelta(text.to_string())).await;
                    }
                }
            }
        }

        let _ = event_tx.send(QueryEvent::AssistantMessage(assistant_msg.clone())).await;
        messages.push(assistant_msg);

        // ── Abort check: after streaming, before tool execution ─────────
        if config.abort_handle.is_aborted() {
            // Backfill any tool_use blocks that won't get results
            let backfills = abort::backfill_missing_tool_results(messages, "Interrupted by user");
            for msg in backfills {
                messages.push(msg);
            }
            let reason = if config.abort_handle.is_submit_interrupt() {
                StopReason::Completed // submit-interrupt: user queued new input
            } else {
                StopReason::Aborted
            };
            let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
            return Ok(reason);
        }

        // ── Token budget tracking ───────────────────────────────────────
        budget_tracker.record_turn(
            cost_tracker.total_input_tokens() + cost_tracker.total_output_tokens()
        );

        // ── If no tool calls → check stop conditions ────────────────────
        if tool_use_blocks.is_empty() {
            // ── Max output tokens recovery (S8) ────────────────────────
            // If the API hit max_output_tokens, try escalating or injecting
            // a resume message (up to MAX_OUTPUT_TOKENS_RECOVERY_LIMIT times).
            if stream_stop_reason.as_deref() == Some("max_tokens")
                && max_output_tokens_recovery_count < retry::MAX_OUTPUT_TOKENS_RECOVERY_LIMIT
            {
                max_output_tokens_recovery_count += 1;
                if max_output_tokens_recovery_count == 1 {
                    // First attempt: escalate to 64K
                    current_max_output_tokens = retry::ESCALATED_MAX_TOKENS;
                    info!(
                        attempt = max_output_tokens_recovery_count,
                        new_max = current_max_output_tokens,
                        "Max output tokens hit — escalating"
                    );
                } else {
                    // Subsequent: inject resume message
                    messages.push(ConversationMessage::user_text(
                        "Your previous response was cut off because it exceeded the output limit. \
                         Resume directly from where you left off, without repeating any content."
                    ));
                    info!(
                        attempt = max_output_tokens_recovery_count,
                        "Max output tokens hit — injecting resume message"
                    );
                }
                continue; // Retry the API call
            }

            if !config.stop_hooks.is_empty() {
                let hook_result = stop_hooks::execute_stop_hooks(&config.stop_hooks).await;
                if hook_result.prevent_continuation {
                    let reason = StopReason::HookPrevented(
                        hook_result.blocking_errors.join("; ")
                    );
                    let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                    return Ok(reason);
                }
                if !hook_result.blocking_errors.is_empty() {
                    let error_text = format!(
                        "Stop hook errors:\n{}",
                        hook_result.blocking_errors.join("\n")
                    );
                    messages.push(ConversationMessage::user_text(error_text));
                    continue;
                }
            }

            // ── M1: Extract memories at end of turn (fire-and-forget) ────
            // This is the stop hook integration point — when the model
            // produces a final response with no tool calls, extract
            // durable memories from the conversation before returning.
            tokio::spawn({
                let msgs = messages.clone();
                let socket = std::env::var("AGENT_IPC_SOCKET")
                    .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
                async move {
                    if let Ok(mut mem_ipc) = crate::ipc::IpcClient::connect_to(&socket).await {
                        super::memory_integration::extract_memories_fire_and_forget(
                            &mut mem_ipc, &msgs
                        ).await;
                    }
                }
            });

            match budget_tracker.check() {
                BudgetDecision::Stop { reason: budget_reason } => {
                    let reason = StopReason::TokenBudget(budget_reason);
                    let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                    return Ok(reason);
                }
                BudgetDecision::Continue { nudge: Some(nudge_msg), .. } => {
                    // Inject nudge as a user message to encourage efficient behavior,
                    // then loop back for another turn (don't stop yet).
                    debug!("Token budget nudge — injecting and continuing");
                    messages.push(ConversationMessage::user_text(nudge_msg));
                    // Reset recovery count so the new turn starts fresh
                    max_output_tokens_recovery_count = 0;
                    continue;
                }
                BudgetDecision::Continue { nudge: None, .. } => {
                    let reason = StopReason::Completed;
                    let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
                    return Ok(reason);
                }
            }
        }

        // ── Execute tool calls ──────────────────────────────────────────
        info!(count = tool_use_blocks.len(), "Executing tool calls");

        let tool_results = execute_tools(
            &tool_use_blocks,
            registry,
            gate,
            &event_tx,
            config.streaming_tool_execution,
        ).await;

        // ── Track tool calls for session memory state ──────────────────
        session_memory_state.record_tool_calls(tool_use_blocks.len() as u32);

        // ── Repeated-tool detection ─────────────────────────────────────
        // If the same tool is called N times in a row, inject a nudge
        // telling the LLM it appears stuck. Prevents infinite loops where
        // the model keeps calling WebSearch/Grep without making progress.
        {
            let current_tools: Vec<String> = tool_use_blocks.iter()
                .map(|b| b.name.clone())
                .collect();

            let dominated_by_one = current_tools.len() > 0 && {
                let first = &current_tools[0];
                current_tools.iter().all(|t| t == first)
            };

            if dominated_by_one {
                let tool_name = &current_tools[0];
                let count = repeated_tool_counter.entry(tool_name.clone()).or_insert(0);
                *count += 1;
                if *count >= MAX_REPEATED_TOOL_TURNS {
                    warn!(
                        tool = %tool_name, count = *count,
                        "Repeated tool detected — injecting progress nudge"
                    );
                    let nudge = format!(
                        "[System: You have called {tool_name} {} times in a row without \
                         producing a final answer. Please synthesize what you've found so far \
                         and respond to the user. If you truly need more information, explain \
                         what you're looking for and why previous attempts didn't suffice.]",
                        *count
                    );
                    // Inject nudge as a user message before the tool results
                    messages.push(ConversationMessage::user_text(&nudge));
                    // Reset counter so the nudge has a chance to work
                    *count = 0;
                }
            } else {
                // Different tools used — reset all counters
                repeated_tool_counter.clear();
            }
        }

        // ── Inject tool results and loop ────────────────────────────────
        messages.push(ConversationMessage::tool_results(tool_results));

        // ── Tool result budget enforcement ──────────────────────────────
        // Enforce per-tool (50K) and aggregate (200K) size limits so that
        // large tool outputs don't overflow the context window.
        if let Some(last_msg) = messages.last_mut() {
            if let Value::Array(ref mut blocks) = last_msg.content {
                let _ = tool_result_budget::enforce_tool_result_budget(
                    blocks,
                    &mut tool_budget_state,
                    &skip_budget_tools,
                    &session_dir,
                );
            }
        }

        // ── Reset per-turn recovery counters ────────────────────────────
        // Each new turn (after tool results) gets 3 fresh max_tokens recovery
        // attempts. Without this reset, attempts are permanently consumed
        // across turns and recovery stops working after the first 3 hits.
        max_output_tokens_recovery_count = 0;

        // ── Abort check: after tool execution ───────────────────────────
        if config.abort_handle.is_aborted() {
            let reason = if config.abort_handle.is_submit_interrupt() {
                StopReason::Completed
            } else {
                StopReason::Aborted
            };
            let _ = event_tx.send(QueryEvent::Done(reason.clone())).await;
            return Ok(reason);
        }

        debug!(turn = turn_count, "Tool results injected, looping back to API");
    }
}
