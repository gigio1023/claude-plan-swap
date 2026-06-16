//! macOS Keychain adapter.
//!
//! This is the only module that talks to the `security` command. Keeping the
//! boundary narrow makes it clear where secret material can enter process memory
//! and keeps persistence code from accidentally writing credentials to disk.

use crate::domain::PlanName;
use anyhow::{Context, Result, bail};
use std::process::Command;

const ACTIVE_SERVICE: &str = "Claude Code-credentials";
const PLAN_SERVICE: &str = "claude-plan-swap";

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Keychain;

impl Keychain {
    /// Read the credential Claude Code currently uses.
    ///
    /// The returned string is secret material. Callers should pass it directly
    /// to another Keychain operation or short-lived in-memory comparison.
    pub(crate) fn read_active(&self, account: &str) -> Result<String> {
        self.read(ACTIVE_SERVICE, account)
    }

    /// Replace the active Claude Code credential with a previously saved plan.
    pub(crate) fn upsert_active(&self, account: &str, credential: &str) -> Result<()> {
        self.upsert(ACTIVE_SERVICE, account, credential)
    }

    /// Read a saved plan credential from the app-owned Keychain service.
    pub(crate) fn read_plan(&self, name: &PlanName) -> Result<String> {
        self.read(PLAN_SERVICE, &plan_account(name))
    }

    /// Store a plan credential without writing it to the filesystem.
    pub(crate) fn upsert_plan(&self, name: &PlanName, credential: &str) -> Result<()> {
        self.upsert(PLAN_SERVICE, &plan_account(name), credential)
    }

    pub(crate) fn delete_plan(&self, name: &PlanName) -> Result<()> {
        self.delete(PLAN_SERVICE, &plan_account(name))
    }

    pub(crate) fn detect_active_account(&self) -> Result<String> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", ACTIVE_SERVICE])
            .output()
            .context("failed to run security to detect active Claude Code account")?;
        if !output.status.success() {
            bail!("Claude Code credential not found in Keychain; log in with claude first");
        }
        let text = String::from_utf8_lossy(&output.stdout);
        parse_keychain_account(&text)
            .ok_or_else(|| anyhow::anyhow!("failed to parse active Keychain account"))
    }

    fn read(&self, service: &str, account: &str) -> Result<String> {
        let output = Command::new("security")
            .args(["find-generic-password", "-s", service, "-a", account, "-w"])
            .output()
            .with_context(|| format!("failed to run security for service {service}"))?;
        if !output.status.success() {
            bail!("security find-generic-password failed for service {service} account {account}");
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .trim_end_matches('\n')
            .to_string())
    }

    fn upsert(&self, service: &str, account: &str, password: &str) -> Result<()> {
        let output = Command::new("security")
            .args([
                "add-generic-password",
                "-U",
                "-s",
                service,
                "-a",
                account,
                "-w",
            ])
            .arg(password)
            .output()
            .with_context(|| format!("failed to run security for service {service}"))?;
        if !output.status.success() {
            bail!(
                "security add-generic-password failed for service {service} account {account}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(())
    }

    fn delete(&self, service: &str, account: &str) -> Result<()> {
        let output = Command::new("security")
            .args(["delete-generic-password", "-s", service, "-a", account])
            .output()
            .with_context(|| format!("failed to run security for service {service}"))?;
        if !output.status.success() {
            bail!("security delete-generic-password failed");
        }
        Ok(())
    }
}

fn plan_account(name: &PlanName) -> String {
    format!("plan:{name}")
}

fn parse_keychain_account(output: &str) -> Option<String> {
    for line in output.lines() {
        if !line.contains("\"acct\"") {
            continue;
        }
        let (_, value) = line.split_once("=\"")?;
        return Some(value.trim_end_matches('"').to_string());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_keychain_account() {
        let text = r#"
keychain: "/Users/me/Library/Keychains/login.keychain-db"
class: "genp"
attributes:
    "acct"<blob>="me@example.com"
    "svce"<blob>="Claude Code-credentials"
"#;
        assert_eq!(
            parse_keychain_account(text).as_deref(),
            Some("me@example.com")
        );
    }
}
