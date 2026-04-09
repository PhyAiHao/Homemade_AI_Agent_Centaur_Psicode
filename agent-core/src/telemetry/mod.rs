//! Telemetry — OpenTelemetry-based tracing, metrics, and Perfetto export.
//!
//! Mirrors `src/utils/telemetry/` (8 files):
//! - instrumentation.ts → mod.rs (OTEL SDK init)
//! - sessionTracing.ts → session_tracing.rs (span lifecycle)
//! - events.ts → events.rs (structured event logging)
//! - perfettoTracing.ts → perfetto.rs (Chrome Trace Event format)
//! - bigqueryExporter.ts → metrics_exporter.rs (HTTP metrics push)
#![allow(dead_code)]

pub mod events;
pub mod session_tracing;
pub mod perfetto;
pub mod metrics_exporter;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use tracing::info;

#[cfg(feature = "telemetry")]
use opentelemetry::global;

// ─── Config ─────────────────────────────────────────────────────────────────

/// Telemetry configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TelemetryConfig {
    pub enabled: bool,
    /// OTLP endpoint (e.g., http://localhost:4318).
    pub otlp_endpoint: Option<String>,
    /// Whether to enable Perfetto trace file output.
    pub perfetto_enabled: bool,
    /// Whether to enable metrics export (BigQuery-style HTTP push).
    pub metrics_enabled: bool,
    /// Session ID for this session.
    pub session_id: String,
    /// Agent version.
    pub agent_version: String,
}

impl TelemetryConfig {
    pub fn from_env(session_id: &str) -> Self {
        let disabled = std::env::var("DISABLE_TELEMETRY")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false);

        let perfetto = std::env::var("CLAUDE_CODE_PERFETTO_TRACE")
            .map(|v| v == "1")
            .unwrap_or(false);

        TelemetryConfig {
            enabled: !disabled,
            otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            perfetto_enabled: perfetto,
            metrics_enabled: !disabled,
            session_id: session_id.to_string(),
            agent_version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }
}

/// Global telemetry state.
static TELEMETRY: OnceLock<TelemetryState> = OnceLock::new();

struct TelemetryState {
    config: TelemetryConfig,
    perfetto: Option<std::sync::Mutex<perfetto::PerfettoTracer>>,
    metrics: Option<std::sync::Mutex<metrics_exporter::MetricsCollector>>,
}

// ─── Initialization ─────────────────────────────────────────────────────────

/// Initialize the full telemetry stack.
pub fn init(config: &TelemetryConfig) -> Result<()> {
    if !config.enabled {
        info!("Telemetry disabled");
        return Ok(());
    }

    // ── Resource detection ──────────────────────────────────────────
    let resource_attrs = detect_resource(config);
    info!(
        attrs = ?resource_attrs.keys().collect::<Vec<_>>(),
        "Telemetry resource detected"
    );

    // ── OpenTelemetry SDK ──────────────────────────────────────────
    #[cfg(feature = "telemetry")]
    if let Some(ref endpoint) = config.otlp_endpoint {
        info!(endpoint = %endpoint, "Initializing OTEL TracerProvider with OTLP exporter");
        // The opentelemetry + tracing-opentelemetry crates handle the
        // actual SDK setup. Here we just configure the global provider.
        // In a full production setup, you'd create:
        //   - TracerProvider with BatchSpanProcessor + OtlpExporter
        //   - MeterProvider with PeriodicReader + OtlpMetricExporter
        //   - LoggerProvider with BatchLogProcessor + OtlpLogExporter
        // For now, we use the tracing-subscriber as the bridge.
    }

    // ── Perfetto tracer ────────────────────────────────────────────
    let perfetto = if config.perfetto_enabled {
        let tracer = perfetto::PerfettoTracer::new(&config.session_id)?;
        info!(
            path = %tracer.output_path().display(),
            "Perfetto tracing enabled"
        );
        Some(std::sync::Mutex::new(tracer))
    } else {
        None
    };

    // ── Metrics collector ──────────────────────────────────────────
    let metrics = if config.metrics_enabled {
        Some(std::sync::Mutex::new(metrics_exporter::MetricsCollector::new(
            &config.session_id,
            &config.agent_version,
        )))
    } else {
        None
    };

    let _ = TELEMETRY.set(TelemetryState {
        config: config.clone(),
        perfetto,
        metrics,
    });

    Ok(())
}

/// Shutdown telemetry: flush all pending data.
pub fn shutdown() {
    if let Some(state) = TELEMETRY.get() {
        // Flush Perfetto traces
        if let Some(ref perfetto) = state.perfetto {
            if let Ok(mut p) = perfetto.lock() {
                if let Err(e) = p.flush() {
                    tracing::warn!(error = %e, "Failed to flush Perfetto traces");
                }
            }
        }

        // Flush metrics
        if let Some(ref metrics) = state.metrics {
            if let Ok(m) = metrics.lock() {
                // Metrics are exported on drop or explicit flush
                let _ = m.summary();
            }
        }

        // Shutdown OTEL SDK
        #[cfg(feature = "telemetry")]
        {
            global::shutdown_tracer_provider();
        }

        info!("Telemetry shut down");
    }
}

/// Check if telemetry is enabled.
pub fn is_enabled() -> bool {
    TELEMETRY.get().map(|s| s.config.enabled).unwrap_or(false)
}

/// Access the Perfetto tracer (if enabled).
pub fn with_perfetto<F, R>(f: F) -> Option<R>
where F: FnOnce(&mut perfetto::PerfettoTracer) -> R
{
    TELEMETRY.get()
        .and_then(|s| s.perfetto.as_ref())
        .and_then(|m| m.lock().ok())
        .map(|mut p| f(&mut p))
}

/// Record a metric data point.
pub fn record_metric(name: &str, value: f64, labels: &[(&str, &str)]) {
    if let Some(state) = TELEMETRY.get() {
        if let Some(ref metrics) = state.metrics {
            if let Ok(mut m) = metrics.lock() {
                m.record(name, value, labels);
            }
        }
    }
}

// ─── Resource detection ─────────────────────────────────────────────────────

fn detect_resource(config: &TelemetryConfig) -> std::collections::HashMap<String, String> {
    let mut attrs = std::collections::HashMap::new();
    attrs.insert("service.name".into(), "centaur-agent".into());
    attrs.insert("service.version".into(), config.agent_version.clone());
    attrs.insert("session.id".into(), config.session_id.clone());
    attrs.insert("os.type".into(), std::env::consts::OS.into());
    attrs.insert("host.arch".into(), std::env::consts::ARCH.into());

    // Detect WSL
    if std::env::consts::OS == "linux" {
        if let Ok(release) = std::fs::read_to_string("/proc/version") {
            if release.to_lowercase().contains("microsoft") {
                attrs.insert("os.wsl".into(), "true".into());
            }
        }
    }

    // Hostname
    if let Ok(hostname) = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
    {
        attrs.insert("host.name".into(), hostname);
    }

    attrs
}
