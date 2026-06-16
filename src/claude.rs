//! Claude Code credential inspection.
//!
//! Account kind is best read from `claude auth status` because that is the CLI's
//! active view of the account. The credential JSON is a fallback for tests and
//! offline setup flows where the active CLI command is unavailable.

use crate::domain::AccountKind;
use serde_json::Value;
use std::process::Command;

pub(crate) fn detect_account_kind_from_active_credential(credential: &str) -> AccountKind {
    if let Some(kind) = detect_account_kind_from_claude_status() {
        return kind;
    }
    detect_account_kind_from_credential(credential).unwrap_or(AccountKind::Other)
}

fn detect_account_kind_from_claude_status() -> Option<AccountKind> {
    let output = Command::new("claude")
        .args(["auth", "status"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    value
        .get("subscriptionType")
        .and_then(Value::as_str)
        .map(AccountKind::from_subscription)
}

fn detect_account_kind_from_credential(credential: &str) -> Option<AccountKind> {
    let value: Value = serde_json::from_str(credential).ok()?;
    value
        .pointer("/claudeAiOauth/subscriptionType")
        .and_then(Value::as_str)
        .map(AccountKind::from_subscription)
}
