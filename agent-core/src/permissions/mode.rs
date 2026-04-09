//! Permission modes — mirrors `src/utils/permissions/PermissionMode.ts`
#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Governs how tool execution requests are approved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    /// Ask the user interactively for each tool invocation (default)
    #[default]
    Default,
    /// Automatically approve all tool invocations without prompting
    AutoApprove,
    /// Only describe what would be done — never execute (plan/dry-run mode)
    PlanOnly,
    /// Skip all permission checks — dangerous, only for trusted automation
    Bypass,
    /// Auto-allow file edits within CWD (SDK mode)
    AcceptEdits,
    /// Convert all 'ask' to 'deny' — headless batch mode
    DontAsk,
}

impl FromStr for PermissionMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "default"     => Ok(Self::Default),
            "autoApprove" => Ok(Self::AutoApprove),
            "planOnly"    => Ok(Self::PlanOnly),
            "bypass"        => Ok(Self::Bypass),
            "acceptEdits"   => Ok(Self::AcceptEdits),
            "dontAsk"       => Ok(Self::DontAsk),
            other           => Err(format!("Unknown permission mode: {other}")),
        }
    }
}

impl fmt::Display for PermissionMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PermissionMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Default     => "default",
            Self::AutoApprove => "autoApprove",
            Self::PlanOnly    => "planOnly",
            Self::Bypass      => "bypass",
            Self::AcceptEdits => "acceptEdits",
            Self::DontAsk     => "dontAsk",
        }
    }

    /// Returns true if tool execution should be auto-approved without prompting.
    pub fn is_auto(&self) -> bool {
        matches!(self, Self::AutoApprove | Self::Bypass | Self::AcceptEdits)
    }

    /// Returns true if this mode completely blocks execution.
    pub fn is_read_only(&self) -> bool {
        matches!(self, Self::PlanOnly)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_str() {
        for mode in [
            PermissionMode::Default,
            PermissionMode::AutoApprove,
            PermissionMode::PlanOnly,
            PermissionMode::Bypass,
            PermissionMode::AcceptEdits,
            PermissionMode::DontAsk,
        ] {
            assert_eq!(mode.as_str().parse::<PermissionMode>().unwrap(), mode);
        }
    }

    #[test]
    fn auto_approve_is_auto() {
        assert!(PermissionMode::AutoApprove.is_auto());
        assert!(PermissionMode::Bypass.is_auto());
        assert!(PermissionMode::AcceptEdits.is_auto());
        assert!(!PermissionMode::Default.is_auto());
        assert!(!PermissionMode::DontAsk.is_auto());
    }
}
