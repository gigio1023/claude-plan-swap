//! JSON persistence for non-secret state.
//!
//! Secrets never pass through this module. Plan credentials live in Keychain;
//! this module stores only metadata, user configuration, notification markers,
//! and the quota cache used by the statusline countdown.

use crate::context::AppContext;
use crate::domain::{Config, RateLimitSnapshot, State};
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

pub(crate) fn load_state(ctx: &AppContext) -> Result<State> {
    load_json_or_default(&ctx.state_path())
}

pub(crate) fn save_state(ctx: &AppContext, state: &State) -> Result<()> {
    ctx.ensure_app_dir()?;
    save_json(&ctx.state_path(), state)
}

pub(crate) fn load_config(ctx: &AppContext) -> Result<Config> {
    load_json_or_default(&ctx.config_path())
}

pub(crate) fn save_config(ctx: &AppContext, config: &Config) -> Result<()> {
    ctx.ensure_app_dir()?;
    save_json(&ctx.config_path(), config)
}

pub(crate) fn load_rate_limits(ctx: &AppContext) -> Result<Option<RateLimitSnapshot>> {
    let path = ctx.rate_limits_path();
    if !path.exists() {
        return Ok(None);
    }
    let snapshot = fs::read_to_string(&path)
        .context("failed to read rate limit snapshot")
        .and_then(|text| {
            serde_json::from_str(&text).context("failed to parse rate limit snapshot")
        })?;
    Ok(Some(snapshot))
}

pub(crate) fn save_rate_limits(ctx: &AppContext, snapshot: &RateLimitSnapshot) -> Result<()> {
    save_json(&ctx.rate_limits_path(), snapshot)
}

pub(crate) fn load_notified(ctx: &AppContext) -> Result<BTreeSet<String>> {
    load_json_or_default(&ctx.notified_path())
}

pub(crate) fn save_notified(ctx: &AppContext, notified: &BTreeSet<String>) -> Result<()> {
    save_json(&ctx.notified_path(), notified)
}

fn load_json_or_default<T>(path: &Path) -> Result<T>
where
    T: for<'de> Deserialize<'de> + Default,
{
    if !path.exists() {
        return Ok(T::default());
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_json<T>(path: &Path, value: &T) -> Result<()>
where
    T: Serialize,
{
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let text = serde_json::to_string_pretty(value)?;
    fs::write(path, format!("{text}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}
