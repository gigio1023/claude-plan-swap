//! Domain types and small invariants.
//!
//! Types in this module describe the durable state model. The module keeps
//! validation close to the values it protects, which makes command handlers
//! smaller and prevents raw strings from crossing security-sensitive code paths.

use anyhow::{Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct AccountName(String);

impl AccountName {
    /// Validate and normalize a user supplied account name.
    ///
    /// The tool stores account credentials under Keychain account names derived
    /// from this value. Restricting the character set keeps those derived names
    /// predictable and avoids shell-looking or path-looking identifiers.
    pub(crate) fn parse(input: &str) -> Result<Self> {
        if input.is_empty() || input.len() > 64 {
            bail!("account name must be 1-64 characters");
        }
        if input.starts_with('-') || input.ends_with('-') || input.contains("--") {
            bail!("account name cannot start/end with '-' or contain '--'");
        }
        if !input
            .chars()
            .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        {
            bail!("account name must use lowercase letters, digits, and '-' only");
        }
        Ok(Self(input.to_string()))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn into_string(self) -> String {
        self.0
    }
}

impl fmt::Display for AccountName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum AccountKind {
    Team,
    Enterprise,
    Other,
}

impl AccountKind {
    pub(crate) fn from_subscription(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "team" => Self::Team,
            "enterprise" => Self::Enterprise,
            _ => Self::Other,
        }
    }

    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Team => "team",
            Self::Enterprise => "enterprise",
            Self::Other => "other",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub(crate) enum RoutingMode {
    Manual,
    Auto,
}

impl RoutingMode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Auto => "auto",
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub(crate) struct Config {
    pub(crate) alert_at: u8,
    pub(crate) mode: RoutingMode,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            alert_at: 95,
            mode: RoutingMode::Manual,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct AccountEntry {
    pub(crate) kind: AccountKind,
    pub(crate) created_at: u64,
    pub(crate) updated_at: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RouteLock {
    pub(crate) source_account: String,
    pub(crate) routed_account: String,
    pub(crate) created_at: u64,
}

#[derive(Debug, Default, Deserialize, Serialize)]
pub(crate) struct State {
    pub(crate) active_account: Option<String>,
    pub(crate) current_account: Option<String>,
    pub(crate) previous_account: Option<String>,
    pub(crate) accounts: BTreeMap<String, AccountEntry>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub(crate) struct LimitWindow {
    pub(crate) used_percentage: Option<u8>,
    pub(crate) resets_at: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct RateLimitSnapshot {
    pub(crate) detected_at: u64,
    pub(crate) five_hour: LimitWindow,
    pub(crate) seven_day: LimitWindow,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_account_names() {
        assert!(AccountName::parse("team").is_ok());
        assert!(AccountName::parse("team-2").is_ok());
        assert!(AccountName::parse("Team").is_err());
        assert!(AccountName::parse("-team").is_err());
        assert!(AccountName::parse("team-").is_err());
        assert!(AccountName::parse("team--2").is_err());
    }
}
