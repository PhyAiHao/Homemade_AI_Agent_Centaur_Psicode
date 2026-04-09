use anyhow::Result;
use clap::{Parser, Subcommand};
use tracing::info;

mod util;
mod bootstrap;
mod init;
mod cli;
mod config;
mod ipc;
mod query_engine;
mod query;
mod history;
mod session_history;
mod cost_tracker;
mod context;
mod state;
mod tools;
mod permissions;
mod bash_parser;
mod shell;
mod git;
mod file_state_cache;
mod file_history;
mod session_storage;
mod model;
mod input;
mod auth;
mod updater;
mod activity;
mod coordinator;
#[cfg(feature = "swarm")]
mod swarm;
mod tasks;
mod remote;
mod server;
mod proxy;
mod sandbox;
#[cfg(feature = "computer_use")]
mod computer_use;
#[cfg(feature = "telemetry")]
mod telemetry;
mod keybindings;
mod vim;
mod tui;
mod settings;
mod migrations;
mod schemas;
mod commands;
mod mcp_client;
mod ide_integration;
mod bridge_client;
mod dream;

/// Centaur Psicode — AI agent for software engineering tasks
#[derive(Parser, Debug)]
#[command(
    name    = "agent",
    version = env!("CARGO_PKG_VERSION"),
    about   = "Centaur Psicode AI Agent",
    long_about = "A polyglot AI agent CLI — Rust core, Python intelligence, TypeScript integrations."
)]
struct Cli {
    /// Path to working directory (defaults to current directory)
    #[arg(short, long, global = true)]
    dir: Option<std::path::PathBuf>,

    /// Model to use (overrides config and CLAUDE_MODEL env var)
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Log level: error | warn | info | debug | trace
    #[arg(long, global = true, default_value = "warn")]
    log_level: String,

    /// Run without interactive TUI (print-only mode)
    #[arg(long, global = true)]
    bare: bool,

    /// Bypass all permission checks (dangerous — use only in trusted environments)
    #[arg(long, global = true)]
    dangerously_bypass_permissions: bool,

    #[command(subcommand)]
    command: Option<Commands>,

    /// Prompt to send directly (non-interactive mode)
    prompt: Option<String>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check environment health
    Doctor,
    /// Show version information
    Version,
    /// Manage API key authentication
    Login,
    /// Remove stored API key
    Logout,
    /// Run as MCP server
    Mcp,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present (non-fatal) — check parent dir too (monorepo layout)
    let _ = dotenvy::dotenv().or_else(|_| dotenvy::from_filename("../.env"));

    let cli = Cli::parse();

    // Initialize tracing subscriber
    init::setup_tracing(&cli.log_level)?;

    info!("Centaur Psicode agent starting (v{})", env!("CARGO_PKG_VERSION"));

    // Bootstrap: parallel prefetch of config, keychain, IPC
    let boot = bootstrap::Bootstrap::new(&cli).await?;

    // Log auth status resolved during bootstrap
    info!("Auth status: {:?}", boot.auth_status.auth_type);

    // Handle subcommands that don't need the full TUI
    if let Some(cmd) = &cli.command {
        return cli::handle_subcommand(cmd, &boot).await;
    }

    // Non-interactive mode: single prompt then exit
    if let Some(prompt) = &cli.prompt {
        return cli::run_single_prompt(prompt.clone(), &boot).await;
    }

    // Bare mode suppresses the full TUI (e.g. when piped or scripted)
    if boot.bare_mode {
        eprintln!("[bare mode] No prompt supplied — nothing to do.");
        return Ok(());
    }

    // Full interactive TUI
    //
    // Set up IPC connection, shared state, and query engine, then launch the
    // TUI event loop.  The TUI receives QueryEvents via a channel and sends
    // user input back to the engine via another channel.
    let mut ipc_client = ipc::IpcClient::new();
    ipc_client.connect().await?;

    let shared_state = state::new_shared_state(boot.working_dir.clone());
    let mode = if boot.bypass_permissions {
        permissions::mode::PermissionMode::Bypass
    } else {
        permissions::mode::PermissionMode::Default
    };
    let mut gate = permissions::gate::PermissionGate::new(mode, Vec::new())
        .with_disk_rules();

    // ── Wire TUI permission prompt to the gate ────────────────────────
    // The gate's prompt_fn sends a request to the TUI thread via a channel,
    // then blocks waiting for the response. The TUI shows the interactive
    // permission dialog and sends back the user's choice.
    let (perm_request_tx, perm_request_rx) = std::sync::mpsc::channel::<(String, String)>();
    let (perm_response_tx, perm_response_rx) = std::sync::mpsc::channel::<bool>();

    // Wrap the response receiver in Arc<Mutex> so it's Send+Sync for the closure
    let perm_response_rx = std::sync::Arc::new(std::sync::Mutex::new(perm_response_rx));
    let perm_request_rx = std::sync::Arc::new(std::sync::Mutex::new(perm_request_rx));
    let perm_response_tx_for_tui = perm_response_tx.clone();

    let rx_for_gate = perm_response_rx.clone();
    gate.set_prompt_fn(move |tool_name: &str, input_json: &str| -> bool {
        // Send the permission request to the TUI thread
        let _ = perm_request_tx.send((tool_name.to_string(), input_json.to_string()));
        // Block waiting for the TUI to respond
        rx_for_gate.lock().ok()
            .and_then(|rx| rx.recv().ok())
            .unwrap_or(false)
    });

    // Load the API key from the environment so it can be passed through IPC
    // to the Python brain. This ensures the key is available even when the
    // request comes from the IDE bridge (VSCode) rather than the terminal.
    let api_key = std::env::var("ANTHROPIC_API_KEY").ok();

    let engine_config = query_engine::QueryEngineConfig {
        cwd: boot.working_dir.clone(),
        model: boot.model.clone(),
        api_key,
        ..query_engine::QueryEngineConfig::default()
    };

    // ── Start MCP servers and discover tools ────────────────────────────
    let mut mcp_manager = mcp_client::McpManager::new();
    let mcp_tools = mcp_manager.start_all().await;
    let mut registry = tools::ToolRegistry::default_registry(shared_state.clone());
    let mcp_count = mcp_manager.register_tools(&mut registry, &mcp_tools);
    if mcp_count > 0 {
        info!(count = mcp_count, "MCP tools registered");
    }

    let (submit_tx, mut submit_rx) = tokio::sync::mpsc::channel::<tui::SubmitMessage>(16);

    // ── IDE bridge: detect running VS Code and connect ──────────────
    ide_integration::cleanup_stale_lockfiles();
    let ide_instances = ide_integration::detect_ides(&boot.working_dir);
    let bridge = if let Some(ide) = ide_instances.first() {
        match bridge_client::BridgeConnection::connect(ide).await {
            Ok(conn) => {
                info!(ide = %ide.lockfile.ide_name, "Connected to IDE bridge");
                Some(conn)
            }
            Err(e) => {
                info!("IDE bridge connection failed (running headless): {e}");
                None
            }
        }
    } else {
        None
    };

    // If bridge is connected, fork query events: TUI + bridge both receive them.
    // Also merge user input from both TUI and bridge into the engine.
    let (engine_tx, mut engine_rx) = tokio::sync::mpsc::channel::<query::query_loop::QueryEvent>(256);
    let bridge_event_tx = bridge.as_ref().map(|b| b.event_tx_clone());

    // Fanout task: engine emits to engine_tx → forward to both TUI (query_rx) and bridge
    let tui_query_tx = {
        let (tui_tx, tui_rx) = tokio::sync::mpsc::channel::<query::query_loop::QueryEvent>(256);

        let bridge_tx = bridge_event_tx.clone();
        tokio::spawn(async move {
            while let Some(event) = engine_rx.recv().await {
                // Forward to TUI
                let _ = tui_tx.send(event.clone()).await;
                // Forward to IDE bridge (if connected)
                if let Some(ref btx) = bridge_tx {
                    let _ = btx.send(event).await;
                }
            }
        });

        tui_rx
    };

    // Merge user input from bridge into the submit channel.
    // Bridge uses a shared model/provider that the engine updates on each query,
    // so it always uses the latest wizard choice — not the stale boot default.
    let bridge_model = std::sync::Arc::new(std::sync::Mutex::new(boot.model.clone()));
    let bridge_provider = std::sync::Arc::new(std::sync::Mutex::new(boot.config.provider.clone()));
    let bridge_model_for_engine = bridge_model.clone();
    let bridge_provider_for_engine = bridge_provider.clone();

    let submit_tx_for_bridge = submit_tx.clone();
    if let Some(mut bridge_conn) = bridge {
        let bm = bridge_model.clone();
        let bp = bridge_provider.clone();
        tokio::spawn(async move {
            while let Some(msg) = bridge_conn.user_message_rx.recv().await {
                info!("User message from IDE bridge");
                let model = bm.lock().unwrap().clone();
                let provider = bp.lock().unwrap().clone();
                let _ = submit_tx_for_bridge.send(tui::SubmitMessage {
                    prompt: msg,
                    model,
                    provider,
                }).await;
            }
        });
    }

    // Spawn the engine task — it processes user input and emits QueryEvents.
    // Also manages the dream (auto memory consolidation) system:
    //   - After each turn: check dream gates, fire if eligible (non-blocking)
    //   - While idle: check every 5 minutes if dream should run
    let engine_state = shared_state.clone();
    let memory_dir = crate::config::agent_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("memory");
    let transcript_dir = crate::config::agent_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."))
        .join("sessions");

    tokio::spawn(async move {
        let mut engine = match query_engine::QueryEngine::with_registry(
            engine_config, ipc_client, engine_state, gate, registry,
        ).await {
            Ok(e) => e,
            Err(e) => {
                eprintln!("Failed to initialize query engine: {e}");
                return;
            }
        };

        // Dream state: checks gates and runs consolidation in the background
        let mut dream_state = dream::DreamState::new(
            memory_dir, transcript_dir,
        );

        // Idle dream timer: check every 5 minutes when no user input
        let idle_dream_interval = tokio::time::Duration::from_secs(5 * 60);

        loop {
            tokio::select! {
                // User submitted a prompt
                Some(submit) = submit_rx.recv() => {
                    // Apply the TUI's current model/provider choice before each query
                    engine.set_model(&submit.model);
                    engine.set_provider(&submit.provider);
                    // Keep bridge in sync with the latest model/provider
                    *bridge_model_for_engine.lock().unwrap() = submit.model.clone();
                    *bridge_provider_for_engine.lock().unwrap() = submit.provider.clone();
                    if let Err(e) = engine.submit_message(&submit.prompt, engine_tx.clone()).await {
                        eprintln!("Query error: {e}");
                    }

                    // Post-turn hook: check if dream should run (non-blocking).
                    // This is the "stop hook" — fires after every completed turn.
                    // The dream runs via IPC so it doesn't block the user.
                    if let Ok(mut dream_ipc) = crate::ipc::IpcClient::new_connected().await {
                        dream_state.maybe_dream(&mut dream_ipc).await;
                    }
                }
                // Idle timeout: periodically check dream gates even when user is away
                _ = tokio::time::sleep(idle_dream_interval) => {
                    if !dream_state.is_running() {
                        if let Ok(mut dream_ipc) = crate::ipc::IpcClient::new_connected().await {
                            dream_state.maybe_dream(&mut dream_ipc).await;
                        }
                    }
                }
            }
        }
    });

    let session_id = uuid::Uuid::new_v4().to_string();
    tui::run_tui(
        &boot.config, &session_id, tui_query_tx, submit_tx,
        perm_request_rx, perm_response_tx_for_tui,
    ).await
}
