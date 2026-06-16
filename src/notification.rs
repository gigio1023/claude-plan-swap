//! macOS notification state.
//!
//! Statusline commands run frequently, so notifications must be idempotent per
//! quota event. The marker file stores only event keys, never account data.

use crate::context::AppContext;
use crate::storage;
use crate::util::escape_applescript;
use anyhow::{Context, Result};
use std::fs;
use std::process::{Command, Stdio};

pub(crate) fn notify_once(ctx: &AppContext, key: &str, title: &str, message: &str) -> Result<()> {
    ctx.ensure_app_dir()?;
    let mut notified = storage::load_notified(ctx)?;
    if notified.contains(key) {
        return Ok(());
    }

    let script = format!(
        "display notification \"{}\" with title \"{}\"",
        escape_applescript(message),
        escape_applescript(title)
    );
    Command::new("osascript")
        .args(["-e", &script])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok();

    notified.insert(key.to_string());
    storage::save_notified(ctx, &notified)
}

pub(crate) fn clear_notified(ctx: &AppContext) -> Result<()> {
    if ctx.notified_path().exists() {
        fs::remove_file(ctx.notified_path()).context("failed to clear notification state")?;
    }
    Ok(())
}
