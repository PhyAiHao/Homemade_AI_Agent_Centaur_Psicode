//! CLI subcommand handlers and single-prompt (non-interactive) mode.
//!
//! Mirrors `src/entrypoints/cli.tsx`.

use anyhow::Result;
use tracing::info;

use crate::bootstrap::Bootstrap;
use crate::Commands;

/// Handle a CLI subcommand that does not require the full TUI.
pub async fn handle_subcommand(cmd: &Commands, boot: &Bootstrap) -> Result<()> {
    match cmd {
        Commands::Doctor  => run_doctor(boot).await,
        Commands::Version => {
            println!("Centaur Psicode agent v{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Commands::Login  => crate::auth::login_interactive().await,
        Commands::Logout => {
            crate::auth::logout()?;
            Ok(())
        }
        Commands::Mcp    => {
            info!("Starting MCP server mode (delegates to agent-integrations)");
            // MCP server is handled by the TypeScript layer; launch it here.
            let status = tokio::process::Command::new("node")
                .arg("../agent-integrations/dist/mcp/entrypoint.js")
                .status()
                .await?;
            std::process::exit(status.code().unwrap_or(1));
        }
    }
}

/// Run a single prompt and print the result (no TUI).
pub async fn run_single_prompt(prompt: String, boot: &Bootstrap) -> Result<()> {
    use crate::query_engine::{QueryEngine, QueryEngineConfig};
    use crate::permissions::gate::PermissionGate;
    use crate::permissions::mode::PermissionMode;
    use crate::ipc::IpcClient;
    use crate::state;

    let mode = if boot.bypass_permissions {
        PermissionMode::Bypass
    } else {
        PermissionMode::AutoApprove
    };

    let mut ipc = IpcClient::new();
    ipc.connect().await?;

    let shared_state = state::new_shared_state(boot.working_dir.clone());
    let gate = PermissionGate::new(mode, Vec::new());

    let config = QueryEngineConfig {
        cwd: boot.working_dir.clone(),
        model: boot.model.clone(),
        ..QueryEngineConfig::default()
    };

    let _result = QueryEngine::run_single(config, &prompt, ipc, shared_state, gate).await?;
    Ok(())
}

/// /doctor subcommand — print environment diagnostics to stdout.
async fn run_doctor(boot: &Bootstrap) -> Result<()> {
    println!("=== Centaur Psicode Doctor ===\n");

    // API key
    match &boot.api_key {
        Some(_) => println!("  ✓ API key present"),
        None    => println!("  ✗ API key missing — run: agent login"),
    }

    // IPC server
    let socket_path = std::env::var("AGENT_IPC_SOCKET")
        .unwrap_or_else(|_| "/tmp/agent-ipc.sock".into());
    if std::path::Path::new(&socket_path).exists() {
        println!("  ✓ IPC server socket present ({socket_path})");
    } else {
        println!("  ✗ IPC server not running — run: make dev-python");
    }

    // Git
    match std::process::Command::new("git").arg("--version").output() {
        Ok(o) if o.status.success() => {
            let v = String::from_utf8_lossy(&o.stdout);
            println!("  ✓ {}", v.trim());
        }
        _ => println!("  ✗ git not found"),
    }

    // Working directory
    println!("  ✓ cwd: {}", boot.working_dir.display());
    println!("  ✓ model: {}", boot.model);

    Ok(())
}
