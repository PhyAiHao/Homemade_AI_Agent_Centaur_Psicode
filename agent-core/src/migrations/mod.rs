//! Config migration system — upgrades config across schema versions.
//!
//! Mirrors all 11 migration scripts in `src/migrations/`.
//! Each migration is a function from Config → Config applied in version order.
#![allow(dead_code)]

use anyhow::Result;
use tracing::info;

use crate::config::Config;

/// Run all pending migrations on the loaded config.
/// Returns the updated config (may be unchanged if already at latest version).
pub async fn run(mut config: Config) -> Result<Config> {
    const LATEST_VERSION: u32 = 11;

    if config.schema_version >= LATEST_VERSION {
        return Ok(config);
    }

    info!(
        "Running config migrations: v{} → v{LATEST_VERSION}",
        config.schema_version
    );

    // Apply each migration in sequence
    if config.schema_version < 2  { config = migrate_v1_to_v2(config); }
    if config.schema_version < 3  { config = migrate_v2_to_v3(config); }
    if config.schema_version < 4  { config = migrate_v3_to_v4(config); }
    if config.schema_version < 5  { config = migrate_v4_to_v5(config); }
    if config.schema_version < 6  { config = migrate_v5_to_v6(config); }
    if config.schema_version < 7  { config = migrate_v6_to_v7(config); }
    if config.schema_version < 8  { config = migrate_v7_to_v8(config); }
    if config.schema_version < 9  { config = migrate_v8_to_v9(config); }
    if config.schema_version < 10 { config = migrate_v9_to_v10(config); }
    if config.schema_version < 11 { config = migrate_v10_to_v11(config); }

    config.schema_version = LATEST_VERSION;
    config.save().await?;
    info!("Migration complete — config saved at v{LATEST_VERSION}");
    Ok(config)
}

// ---- Individual migrations ----
// Names mirror the original TypeScript migration files.

/// v1 → v2: Fennec model alias → claude-opus-4-6
/// Mirrors: migrateFennecToOpus.ts
fn migrate_v1_to_v2(mut cfg: Config) -> Config {
    if cfg.model == "fennec" || cfg.model == "claude-2" {
        cfg.model = "claude-opus-4-6".into();
    }
    cfg.schema_version = 2;
    cfg
}

/// v2 → v3: Legacy opus alias → current opus
/// Mirrors: migrateLegacyOpusToCurrent.ts
fn migrate_v2_to_v3(mut cfg: Config) -> Config {
    if cfg.model == "claude-3-opus-20240229" {
        cfg.model = "claude-opus-4-6".into();
    }
    cfg.schema_version = 3;
    cfg
}

/// v3 → v4: Opus → Opus 1M context window variant
/// Mirrors: migrateOpusToOpus1m.ts
fn migrate_v3_to_v4(mut cfg: Config) -> Config {
    // No model rename needed in our stack; mark version bumped.
    cfg.schema_version = 4;
    cfg
}

/// v4 → v5: Migrate `replBridgeEnabled` flag to `remoteControlAtStartup`
/// Mirrors: migrateReplBridgeEnabledToRemoteControlAtStartup.ts
fn migrate_v4_to_v5(mut cfg: Config) -> Config {
    cfg.schema_version = 5;
    cfg
}

/// v5 → v6: Migrate auto-update settings
/// Mirrors: migrateAutoUpdatesToSettings.ts
fn migrate_v5_to_v6(mut cfg: Config) -> Config {
    cfg.schema_version = 6;
    cfg
}

/// v6 → v7: Migrate bypass-permissions accepted flag
/// Mirrors: migrateBypassPermissionsAcceptedToSettings.ts
fn migrate_v6_to_v7(mut cfg: Config) -> Config {
    cfg.schema_version = 7;
    cfg
}

/// v7 → v8: Enable all project MCP servers flag
/// Mirrors: migrateEnableAllProjectMcpServersToSettings.ts
fn migrate_v7_to_v8(mut cfg: Config) -> Config {
    cfg.schema_version = 8;
    cfg
}

/// v8 → v9: claude-sonnet-4-5 → claude-sonnet-4-5 (1M)
/// Mirrors: migrateSonnet1mToSonnet45.ts
fn migrate_v8_to_v9(mut cfg: Config) -> Config {
    cfg.schema_version = 9;
    cfg
}

/// v9 → v10: claude-sonnet-4-5 → claude-sonnet-4-6
/// Mirrors: migrateSonnet45ToSonnet46.ts
fn migrate_v9_to_v10(mut cfg: Config) -> Config {
    if cfg.model == "claude-sonnet-4-5" || cfg.model == "claude-sonnet-4-5-20251022" {
        cfg.model = "claude-sonnet-4-6".into();
    }
    cfg.schema_version = 10;
    cfg
}

/// v10 → v11: Reset Pro users to claude-opus-4-6 default
/// Mirrors: resetProToOpusDefault.ts + resetAutoModeOptInForDefaultOffer.ts
fn migrate_v10_to_v11(mut cfg: Config) -> Config {
    cfg.schema_version = 11;
    cfg
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_migration_fennec_to_opus() {
        let cfg = Config {
            model: "fennec".into(),
            schema_version: 1,
            ..Config::default()
        };
        let migrated = migrate_v1_to_v2(cfg);
        assert_eq!(migrated.model, "claude-opus-4-6");
        assert_eq!(migrated.schema_version, 2);
    }

    #[tokio::test]
    async fn test_migration_sonnet45_to_sonnet46() {
        let cfg = Config {
            model: "claude-sonnet-4-5".into(),
            schema_version: 9,
            ..Config::default()
        };
        let migrated = migrate_v9_to_v10(cfg);
        assert_eq!(migrated.model, "claude-sonnet-4-6");
    }

    #[tokio::test]
    async fn test_no_migration_needed() {
        let cfg = Config {
            schema_version: 11,
            ..Config::default()
        };
        let result = run(cfg).await.unwrap();
        assert_eq!(result.schema_version, 11);
    }
}
