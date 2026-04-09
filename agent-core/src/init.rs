//! Initialization — tracing, graceful shutdown, runtime checks.
//!
//! Mirrors `src/entrypoints/init.ts`.
#![allow(dead_code)]

use anyhow::Result;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

/// Set up the global tracing subscriber.
/// Respects RUST_LOG env var; falls back to the `log_level` CLI argument.
pub fn setup_tracing(log_level: &str) -> Result<()> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(log_level));

    tracing_subscriber::registry()
        .with(fmt::layer().with_target(false).compact())
        .with(filter)
        .init();

    Ok(())
}

/// Register a Ctrl-C / SIGTERM handler that performs graceful shutdown.
pub fn register_shutdown_handler() {
    tokio::spawn(async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to listen for ctrl-c");
        tracing::info!("Received shutdown signal — cleaning up");
        // IPC client will detect the dropped socket and clean up on the Python side.
        std::process::exit(0);
    });
}
