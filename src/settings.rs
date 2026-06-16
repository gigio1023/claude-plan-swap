//! Claude Code settings mutation.
//!
//! This module owns `settings.json` edits so the app can install a statusline
//! wrapper without losing a user's existing statusline command.

use crate::context::AppContext;
use crate::util::shell_quote;
use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::path::Path;

pub(crate) fn install_statusline(ctx: &AppContext) -> Result<()> {
    ctx.ensure_app_dir()?;
    ctx.ensure_claude_dir()?;

    let settings_path = ctx.settings_path();
    let mut settings = load_settings(&settings_path)?;
    let command = statusline_command()?;

    if let Some(existing) = settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        && !is_our_statusline_command(existing)
    {
        fs::write(ctx.inner_statusline_path(), existing)
            .context("failed to save prior statusline command")?;
    }

    settings["statusLine"] = json!({
        "type": "command",
        "command": command,
    });
    save_settings(&settings_path, &settings)?;
    println!("installed Claude Code statusLine wrapper");
    Ok(())
}

pub(crate) fn uninstall_statusline(ctx: &AppContext) -> Result<()> {
    let settings_path = ctx.settings_path();
    let mut settings = load_settings(&settings_path)?;
    let current = settings
        .get("statusLine")
        .and_then(|value| value.get("command"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    if !is_our_statusline_command(&current) {
        println!("statusLine wrapper is not installed");
        return Ok(());
    }

    let inner_path = ctx.inner_statusline_path();
    if inner_path.exists() {
        let inner = fs::read_to_string(&inner_path)
            .context("failed to read saved prior statusline command")?;
        settings["statusLine"] = json!({
            "type": "command",
            "command": inner.trim(),
        });
    } else if let Some(object) = settings.as_object_mut() {
        object.remove("statusLine");
    }

    save_settings(&settings_path, &settings)?;
    println!("uninstalled Claude Code statusLine wrapper");
    Ok(())
}

fn statusline_command() -> Result<String> {
    let exe = env::current_exe().context("failed to find current executable")?;
    Ok(format!("{} statusline", shell_quote(&exe)))
}

fn is_our_statusline_command(command: &str) -> bool {
    command.contains("claude-plan-swap") && command.contains("statusline")
}

fn load_settings(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(json!({}));
    }
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("failed to parse {}", path.display()))
}

fn save_settings(path: &Path, settings: &Value) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    fs::write(
        path,
        format!("{}\n", serde_json::to_string_pretty(settings)?),
    )
    .with_context(|| format!("failed to write {}", path.display()))
}
