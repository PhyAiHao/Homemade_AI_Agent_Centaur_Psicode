//! Auth — keychain-based API key storage, OAuth 2.0, AWS STS, and
//! subscription detection.
//!
//! Mirrors `src/utils/auth/` from the original. Handles:
//! - API key storage/retrieval via system keychain
//! - OAuth 2.0 PKCE flow for Claude.ai
//! - AWS STS caller identity for Bedrock
//! - Subscription type detection (API customer vs Claude.ai subscriber)
//! - External API key helper command with TTL caching
//! - Token refresh and expiry handling
#![allow(dead_code)]

use anyhow::{Context, Result};
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime};
use tracing::{debug, info, warn};

const SERVICE_NAME: &str = "centaur-psicode";
const API_KEY_ACCOUNT: &str = "anthropic-api-key";
const OAUTH_TOKEN_ACCOUNT: &str = "oauth-token";
const OAUTH_REFRESH_ACCOUNT: &str = "oauth-refresh-token";

// ─── API Key Storage ────────────────────────────────────────────────────────

/// Store an API key in the system keychain.
pub fn store_api_key(key: &str) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, API_KEY_ACCOUNT)
        .context("Creating keychain entry")?;
    entry.set_password(key)
        .context("Storing API key in keychain")?;
    info!("API key stored in system keychain");
    Ok(())
}

/// Retrieve the API key from the keychain or environment.
pub fn get_api_key() -> Result<Option<String>> {
    // 1. Try keychain
    if let Ok(entry) = Entry::new(SERVICE_NAME, API_KEY_ACCOUNT) {
        if let Ok(key) = entry.get_password() {
            if !key.is_empty() {
                return Ok(Some(key));
            }
        }
    }

    // 2. Try environment variable
    if let Ok(key) = std::env::var("ANTHROPIC_API_KEY") {
        if !key.is_empty() {
            return Ok(Some(key));
        }
    }

    // 3. Try API key helper command
    if let Some(key) = try_api_key_helper()? {
        return Ok(Some(key));
    }

    Ok(None)
}

/// Remove the API key from the keychain.
pub fn remove_api_key() -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, API_KEY_ACCOUNT)
        .context("Creating keychain entry")?;
    match entry.delete_credential() {
        Ok(()) => {
            info!("API key removed from keychain");
            Ok(())
        }
        Err(keyring::Error::NoEntry) => {
            debug!("No API key found in keychain");
            Ok(())
        }
        Err(e) => Err(anyhow::anyhow!("Failed to remove API key: {e}")),
    }
}

// ─── OAuth 2.0 PKCE Flow ────────────────────────────────────────────────────

/// OAuth token pair.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthTokens {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_at: Option<u64>, // unix timestamp ms
    pub token_type: String,
}

impl OAuthTokens {
    /// Check if the access token is expired (with 5-minute buffer).
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            let now = SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            now + 5 * 60 * 1000 > expires_at // 5-minute buffer
        } else {
            false
        }
    }
}

/// Store OAuth tokens in the keychain.
pub fn store_oauth_tokens(tokens: &OAuthTokens) -> Result<()> {
    let entry = Entry::new(SERVICE_NAME, OAUTH_TOKEN_ACCOUNT)?;
    let serialized = serde_json::to_string(tokens)?;
    entry.set_password(&serialized)?;
    info!("OAuth tokens stored in keychain");
    Ok(())
}

/// Retrieve OAuth tokens from the keychain.
pub fn get_oauth_tokens() -> Result<Option<OAuthTokens>> {
    let entry = match Entry::new(SERVICE_NAME, OAUTH_TOKEN_ACCOUNT) {
        Ok(e) => e,
        Err(_) => return Ok(None),
    };
    match entry.get_password() {
        Ok(json) if !json.is_empty() => {
            let tokens: OAuthTokens = serde_json::from_str(&json)?;
            Ok(Some(tokens))
        }
        _ => Ok(None),
    }
}

/// Remove OAuth tokens from the keychain.
pub fn remove_oauth_tokens() -> Result<()> {
    if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_TOKEN_ACCOUNT) {
        let _ = entry.delete_credential();
    }
    if let Ok(entry) = Entry::new(SERVICE_NAME, OAUTH_REFRESH_ACCOUNT) {
        let _ = entry.delete_credential();
    }
    Ok(())
}

/// Refresh an expired OAuth access token using the refresh token.
pub async fn refresh_oauth_token(
    token_endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<OAuthTokens> {
    let client = reqwest::Client::new();
    let params = [
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ];

    let resp = client.post(token_endpoint)
        .form(&params)
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("Token refresh failed (HTTP {status}): {body}");
    }

    let data: serde_json::Value = resp.json().await?;
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;

    let tokens = OAuthTokens {
        access_token: data["access_token"].as_str().unwrap_or("").to_string(),
        refresh_token: data["refresh_token"].as_str()
            .map(|s| s.to_string())
            .or_else(|| Some(refresh_token.to_string())),
        expires_at: data["expires_in"].as_u64()
            .map(|secs| now_ms + secs * 1000),
        token_type: data["token_type"].as_str().unwrap_or("Bearer").to_string(),
    };

    store_oauth_tokens(&tokens)?;
    info!("OAuth token refreshed");
    Ok(tokens)
}

// ─── AWS STS Authentication ─────────────────────────────────────────────────

/// AWS caller identity for Bedrock access.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwsIdentity {
    pub account_id: String,
    pub arn: String,
    pub user_id: String,
}

/// Check AWS STS caller identity (for Bedrock authentication).
/// Returns None if no AWS credentials are configured.
pub async fn check_aws_sts_identity() -> Result<Option<AwsIdentity>> {
    // Check for AWS credentials
    let has_creds = std::env::var("AWS_ACCESS_KEY_ID").is_ok()
        || std::env::var("AWS_PROFILE").is_ok()
        || std::env::var("AWS_ROLE_ARN").is_ok()
        || std::path::Path::new(&format!(
            "{}/.aws/credentials",
            dirs::home_dir().unwrap_or_default().display()
        )).exists();

    if !has_creds {
        return Ok(None);
    }

    // Call STS GetCallerIdentity via AWS CLI
    let output = tokio::process::Command::new("aws")
        .args(["sts", "get-caller-identity", "--output", "json"])
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => {
            let json: serde_json::Value = serde_json::from_slice(&out.stdout)?;
            Ok(Some(AwsIdentity {
                account_id: json["Account"].as_str().unwrap_or("").to_string(),
                arn: json["Arn"].as_str().unwrap_or("").to_string(),
                user_id: json["UserId"].as_str().unwrap_or("").to_string(),
            }))
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr);
            debug!("AWS STS call failed: {stderr}");
            Ok(None)
        }
        Err(e) => {
            debug!("AWS CLI not available: {e}");
            Ok(None)
        }
    }
}

// ─── Subscription / Billing Detection ───────────────────────────────────────

/// Detected authentication type.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub enum AuthType {
    /// Direct API customer (sk-ant-... key).
    ApiKey,
    /// Claude.ai subscriber (OAuth token).
    ClaudeAiSubscriber,
    /// AWS Bedrock via STS.
    AwsBedrock,
    /// No authentication configured.
    None,
}

/// Subscription info.
#[derive(Debug, Clone, Serialize)]
pub struct AuthStatus {
    pub auth_type: AuthType,
    pub has_api_key: bool,
    pub has_oauth: bool,
    pub has_aws: bool,
    pub api_key_prefix: Option<String>,
    pub oauth_expired: bool,
    pub aws_account: Option<String>,
}

/// Detect the current authentication status.
pub async fn detect_auth_status() -> AuthStatus {
    let api_key = get_api_key().ok().flatten();
    let oauth = get_oauth_tokens().ok().flatten();
    let aws = check_aws_sts_identity().await.ok().flatten();

    let api_key_prefix = api_key.as_ref().map(|k| {
        if k.len() > 10 {
            format!("{}...", &k[..10])
        } else {
            "***".to_string()
        }
    });

    let oauth_expired = oauth.as_ref().map(|t| t.is_expired()).unwrap_or(false);

    let auth_type = if oauth.is_some() && !oauth_expired {
        AuthType::ClaudeAiSubscriber
    } else if api_key.is_some() {
        AuthType::ApiKey
    } else if aws.is_some() {
        AuthType::AwsBedrock
    } else {
        AuthType::None
    };

    AuthStatus {
        auth_type,
        has_api_key: api_key.is_some(),
        has_oauth: oauth.is_some(),
        has_aws: aws.is_some(),
        api_key_prefix,
        oauth_expired,
        aws_account: aws.map(|a| a.account_id),
    }
}

// ─── API Key Helper ─────────────────────────────────────────────────────────

/// Cache for the API key helper result.
static API_KEY_HELPER_CACHE: std::sync::OnceLock<std::sync::Mutex<Option<(String, std::time::Instant)>>> =
    std::sync::OnceLock::new();

/// TTL for cached helper results (5 minutes).
const HELPER_CACHE_TTL: Duration = Duration::from_secs(5 * 60);

/// Try to get an API key from an external helper command.
/// Configured via ANTHROPIC_API_KEY_HELPER env var.
fn try_api_key_helper() -> Result<Option<String>> {
    let helper_cmd = match std::env::var("ANTHROPIC_API_KEY_HELPER") {
        Ok(cmd) if !cmd.is_empty() => cmd,
        _ => return Ok(None),
    };

    // Check cache
    let cache = API_KEY_HELPER_CACHE.get_or_init(|| std::sync::Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((ref cached_key, ref cached_at)) = *guard {
            if cached_at.elapsed() < HELPER_CACHE_TTL {
                return Ok(Some(cached_key.clone()));
            }
        }
    }

    // Run the helper command
    debug!("Running API key helper: {helper_cmd}");
    let output = std::process::Command::new("sh")
        .args(["-c", &helper_cmd])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!("API key helper failed: {stderr}");
        return Ok(None);
    }

    let key = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if key.is_empty() {
        return Ok(None);
    }

    // Cache the result
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((key.clone(), std::time::Instant::now()));
    }

    Ok(Some(key))
}

// ─── Interactive Login / Logout ─────────────────────────────────────────────

/// Interactive login flow — prompts for API key and stores it.
pub async fn login_interactive() -> Result<()> {
    println!("Enter your Anthropic API key (starts with sk-ant-):");
    let mut key = String::new();
    std::io::stdin().read_line(&mut key)?;
    let key = key.trim();

    if !key.starts_with("sk-ant-") && !key.starts_with("sk-") {
        anyhow::bail!("Invalid API key format. Expected sk-ant-... or sk-...");
    }

    store_api_key(key)?;
    println!("API key stored successfully.");

    // Verify the key works
    if let Err(e) = verify_api_key(key).await {
        println!("Warning: key verification failed: {e}");
        println!("The key has been stored but may not be valid.");
    } else {
        println!("API key verified successfully.");
    }

    Ok(())
}

/// Verify an API key by making a test request.
pub async fn verify_api_key(key: &str) -> Result<()> {
    let client = reqwest::Client::new();
    let resp = client
        .get("https://api.anthropic.com/v1/models")
        .header("x-api-key", key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await?;

    if resp.status().is_success() {
        Ok(())
    } else {
        anyhow::bail!("API returned status {}", resp.status())
    }
}

/// Logout — remove all stored credentials.
pub fn logout() -> Result<()> {
    remove_api_key()?;
    remove_oauth_tokens()?;
    println!("All credentials removed.");
    Ok(())
}

/// Display current auth status.
pub async fn show_auth_status() {
    let status = detect_auth_status().await;
    println!("Authentication Status:");
    println!("  Type: {:?}", status.auth_type);
    println!("  API Key: {}", if status.has_api_key {
        status.api_key_prefix.as_deref().unwrap_or("present")
    } else { "not set" });
    println!("  OAuth: {}", if status.has_oauth {
        if status.oauth_expired { "expired" } else { "active" }
    } else { "not configured" });
    println!("  AWS: {}", if status.has_aws {
        status.aws_account.as_deref().unwrap_or("configured")
    } else { "not configured" });
}
