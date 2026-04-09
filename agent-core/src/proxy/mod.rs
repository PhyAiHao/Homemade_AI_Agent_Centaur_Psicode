//! Upstream proxy — MITM proxy for container environments.
//!
//! Mirrors `src/upstreamproxy/` (2 files). Handles CA certificate
//! injection and request relaying for containerized deployments.
#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Upstream proxy URL (e.g., http://proxy:8080).
    pub upstream_url: Option<String>,
    /// Path to CA certificate bundle.
    pub ca_cert_path: Option<String>,
    /// Whether to relay through the proxy.
    pub enabled: bool,
}

impl ProxyConfig {
    /// Load proxy config from environment variables.
    pub fn from_env() -> Self {
        ProxyConfig {
            upstream_url: std::env::var("HTTPS_PROXY")
                .or_else(|_| std::env::var("https_proxy"))
                .ok(),
            ca_cert_path: std::env::var("SSL_CERT_FILE")
                .or_else(|_| std::env::var("NODE_EXTRA_CA_CERTS"))
                .ok(),
            enabled: std::env::var("HTTPS_PROXY").is_ok()
                || std::env::var("https_proxy").is_ok(),
        }
    }

    /// Check if proxy is configured and enabled.
    pub fn is_active(&self) -> bool {
        self.enabled && self.upstream_url.is_some()
    }
}

/// Configure a reqwest client with proxy settings.
pub fn configure_client(config: &ProxyConfig) -> Result<reqwest::Client> {
    let mut builder = reqwest::Client::builder();

    if let Some(ref proxy_url) = config.upstream_url {
        if config.enabled {
            let proxy = reqwest::Proxy::all(proxy_url)?;
            builder = builder.proxy(proxy);
        }
    }

    if let Some(ref ca_path) = config.ca_cert_path {
        let cert_pem = std::fs::read(ca_path)?;
        let cert = reqwest::Certificate::from_pem(&cert_pem)?;
        builder = builder.add_root_certificate(cert);
    }

    Ok(builder.build()?)
}
