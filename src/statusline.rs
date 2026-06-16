//! Claude Code statusline integration.
//!
//! Claude Code calls this binary repeatedly with JSON on stdin. Rendering must
//! therefore stay lightweight, tolerate malformed input, and avoid noisy output
//! when quota data is absent.

use crate::context::AppContext;
use crate::domain::{
    AccountKind, AccountName, Config, LimitWindow, RateLimitSnapshot, RouteLock, RoutingMode, State,
};
use crate::keychain::Keychain;
use crate::notification;
use crate::routing;
use crate::storage;
use crate::switcher::{self, SwitchOptions};
use crate::time::now_epoch;
use anyhow::{Context, Result};
use serde_json::Value;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, Stdio};

pub(crate) fn handle(ctx: &AppContext, keychain: &Keychain) -> Result<()> {
    let input = read_stdin_to_string()?;
    if input.trim().is_empty() {
        return Ok(());
    }

    let parsed: Value = match serde_json::from_str(&input) {
        Ok(value) => value,
        Err(_) => return Ok(()),
    };

    let inner_output = run_inner_statusline(ctx, &input);
    let state = storage::load_state(ctx)?;
    let config = storage::load_config(ctx)?;
    let router_output = render_router_output(ctx, keychain, &state, &config, &parsed)?;

    match (router_output.as_deref(), inner_output.as_deref()) {
        (Some(a), Some(b)) if !a.is_empty() && !b.is_empty() => println!("{a} | {b}"),
        (Some(a), _) if !a.is_empty() => println!("{a}"),
        (_, Some(b)) if !b.is_empty() => println!("{b}"),
        _ => {}
    }

    Ok(())
}

fn render_router_output(
    ctx: &AppContext,
    keychain: &Keychain,
    state: &State,
    config: &Config,
    input: &Value,
) -> Result<Option<String>> {
    let current_account = match state.current_account.as_deref() {
        Some(value) => value,
        None => return Ok(None),
    };
    let current_kind = match state.accounts.get(current_account) {
        Some(entry) => entry.kind,
        None => return Ok(None),
    };

    match current_kind {
        AccountKind::Team => render_team(ctx, keychain, state, config, current_account, input),
        AccountKind::Enterprise => render_enterprise(ctx, keychain, state, config, current_account),
        AccountKind::Other => Ok(Some(current_account.to_string())),
    }
}

fn render_team(
    ctx: &AppContext,
    keychain: &Keychain,
    state: &State,
    config: &Config,
    current_account: &str,
    input: &Value,
) -> Result<Option<String>> {
    let snapshot = match parse_rate_limits(input) {
        Some(snapshot) => snapshot,
        None => return Ok(None),
    };
    ctx.ensure_app_dir()?;
    // Persist every valid snapshot, including below-threshold usage. If the
    // user routes to an enterprise account early, the enterprise statusline can
    // still count down to the previously observed team reset time.
    storage::save_rate_limits(ctx, &snapshot)?;

    let Some((label, pct)) = max_limit(&snapshot) else {
        return Ok(None);
    };

    if pct < config.alert_at {
        return Ok(None);
    }

    if pct >= 100 {
        if config.mode == RoutingMode::Auto
            && let Some(target) = routing::enterprise_target(state, current_account)
        {
            return auto_route_to_enterprise(ctx, keychain, &target);
        }
        let event_key = format!("team-limit:{current_account}");
        notification::notify_once(
            ctx,
            &event_key,
            "claude-quota-router",
            &format!("{current_account} quota is full. Run claude-quota-router list."),
        )
        .ok();
        return Ok(Some(format!(
            "{current_account} LIMIT({label}) -> claude-quota-router list"
        )));
    }

    let event_key = format!("team-alert:{current_account}");
    notification::notify_once(
        ctx,
        &event_key,
        "claude-quota-router",
        &format!("{current_account} usage is {pct}% ({label}). Prepare to switch."),
    )
    .ok();
    Ok(Some(format!(
        "{current_account} {pct}%({label}) -> claude-quota-router list"
    )))
}

fn render_enterprise(
    ctx: &AppContext,
    keychain: &Keychain,
    state: &State,
    config: &Config,
    current_account: &str,
) -> Result<Option<String>> {
    let Some(snapshot) = storage::load_rate_limits(ctx)? else {
        return Ok(Some(current_account.to_string()));
    };
    let lock = storage::load_route_lock(ctx)?;
    let source_account = lock
        .as_ref()
        .map(|value| value.source_account.as_str())
        .unwrap_or("team account");
    // The first reset to arrive is the first point at which returning to a team
    // account is useful. Showing the earliest window keeps the prompt actionable.
    let Some((label, reset_at)) = earliest_reset(&snapshot) else {
        return Ok(Some(current_account.to_string()));
    };
    let remaining = reset_at - now_epoch() as i64;

    if remaining <= 0 {
        if config.mode == RoutingMode::Auto
            && let Some(target) = team_return_target(ctx, state, current_account, lock.as_ref())
        {
            return auto_switch(ctx, keychain, &target, &format!("auto routed to {target}"));
        }
        let event_key = format!("reset:{source_account}");
        notification::notify_once(
            ctx,
            &event_key,
            "claude-quota-router",
            &format!("{source_account} quota reset is complete. Run claude-quota-router list."),
        )
        .ok();
        return Ok(Some(format!(
            "{source_account} reset done -> claude-quota-router list"
        )));
    }

    if remaining <= 60 {
        let event_key = format!("reset-1min:{source_account}");
        notification::notify_once(
            ctx,
            &event_key,
            "claude-quota-router",
            &format!("{source_account} quota resets within 1 minute."),
        )
        .ok();
        return Ok(Some(format!(
            "{source_account} reset in {remaining}s({label}) -> claude-quota-router list"
        )));
    }
    if remaining <= 300 {
        let minutes = (remaining + 59) / 60;
        let event_key = format!("reset-5min:{source_account}");
        notification::notify_once(
            ctx,
            &event_key,
            "claude-quota-router",
            &format!("{source_account} quota resets in {minutes} minutes."),
        )
        .ok();
        return Ok(Some(format!(
            "{source_account} reset in {minutes}m({label}) -> claude-quota-router list"
        )));
    }

    let minutes = remaining / 60;
    if minutes >= 60 {
        Ok(Some(format!(
            "{current_account} | {source_account} reset in {}h{}m({label})",
            minutes / 60,
            minutes % 60
        )))
    } else {
        Ok(Some(format!(
            "{current_account} | {source_account} reset in {minutes}m({label})"
        )))
    }
}

fn auto_route_to_enterprise(
    ctx: &AppContext,
    keychain: &Keychain,
    target: &str,
) -> Result<Option<String>> {
    auto_switch(ctx, keychain, target, &format!("auto routed to {target}"))?;
    Ok(Some(format!(
        "auto routed to {target}; restart Claude Code"
    )))
}

fn auto_switch(
    ctx: &AppContext,
    keychain: &Keychain,
    target: &str,
    message: &str,
) -> Result<Option<String>> {
    let target = AccountName::parse(target)?;
    switcher::switch_to(
        ctx,
        keychain,
        &target,
        SwitchOptions {
            yes: true,
            emit: false,
        },
    )?;
    Ok(Some(format!("{message}; restart Claude Code")))
}

fn team_return_target(
    ctx: &AppContext,
    state: &State,
    current_account: &str,
    lock: Option<&RouteLock>,
) -> Option<String> {
    if let Some(lock) = lock
        && let Some(target) = routing::locked_team_target(state, current_account, lock)
    {
        return Some(target);
    }
    if ctx.route_lock_path().exists() {
        return routing::previous_team_target(state, current_account);
    }
    None
}

fn parse_rate_limits(input: &Value) -> Option<RateLimitSnapshot> {
    let rate_limits = input.get("rate_limits")?;
    Some(RateLimitSnapshot {
        detected_at: now_epoch(),
        five_hour: LimitWindow {
            used_percentage: value_to_u8(rate_limits.pointer("/five_hour/used_percentage")),
            resets_at: value_to_i64(rate_limits.pointer("/five_hour/resets_at")),
        },
        seven_day: LimitWindow {
            used_percentage: value_to_u8(rate_limits.pointer("/seven_day/used_percentage")),
            resets_at: value_to_i64(rate_limits.pointer("/seven_day/resets_at")),
        },
    })
}

fn max_limit(snapshot: &RateLimitSnapshot) -> Option<(&'static str, u8)> {
    let mut best: Option<(&'static str, u8)> = None;
    if let Some(pct) = snapshot.five_hour.used_percentage {
        best = Some(("5h", pct));
    }
    if let Some(pct) = snapshot.seven_day.used_percentage
        && best.is_none_or(|(_, current)| pct > current)
    {
        best = Some(("7d", pct));
    }
    best
}

fn earliest_reset(snapshot: &RateLimitSnapshot) -> Option<(&'static str, i64)> {
    let mut best: Option<(&'static str, i64)> = None;
    if let Some(reset) = snapshot.five_hour.resets_at {
        best = Some(("5h", reset));
    }
    if let Some(reset) = snapshot.seven_day.resets_at
        && best.is_none_or(|(_, current)| reset < current)
    {
        best = Some(("7d", reset));
    }
    best
}

fn value_to_u8(value: Option<&Value>) -> Option<u8> {
    match value? {
        Value::Number(number) => number.as_u64().and_then(|value| u8::try_from(value).ok()),
        Value::String(text) => text.parse::<u8>().ok(),
        _ => None,
    }
}

fn value_to_i64(value: Option<&Value>) -> Option<i64> {
    match value? {
        Value::Number(number) => number.as_i64(),
        Value::String(text) => text.parse::<i64>().ok(),
        _ => None,
    }
}

fn read_stdin_to_string() -> Result<String> {
    io::read_to_string(io::stdin()).context("failed to read stdin")
}

fn run_inner_statusline(ctx: &AppContext, input: &str) -> Option<String> {
    let command = fs::read_to_string(ctx.inner_statusline_path()).ok()?;
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    // Existing statusline commands are intentionally executed as the shell
    // command Claude Code already accepted in settings.json. The wrapper only
    // preserves compatibility; it does not reinterpret the user's command.
    let mut child = Command::new("/bin/sh")
        .arg("-lc")
        .arg(command)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(input.as_bytes()).ok()?;
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_rate_limits_from_statusline_json() {
        let input = json!({
            "rate_limits": {
                "five_hour": { "used_percentage": 91, "resets_at": 1000 },
                "seven_day": { "used_percentage": "12", "resets_at": "2000" }
            }
        });
        let parsed = parse_rate_limits(&input).unwrap();
        assert_eq!(parsed.five_hour.used_percentage, Some(91));
        assert_eq!(parsed.five_hour.resets_at, Some(1000));
        assert_eq!(parsed.seven_day.used_percentage, Some(12));
        assert_eq!(parsed.seven_day.resets_at, Some(2000));
    }

    #[test]
    fn picks_max_usage_and_earliest_reset() {
        let snapshot = RateLimitSnapshot {
            detected_at: 1,
            five_hour: LimitWindow {
                used_percentage: Some(80),
                resets_at: Some(500),
            },
            seven_day: LimitWindow {
                used_percentage: Some(95),
                resets_at: Some(1000),
            },
        };
        assert_eq!(max_limit(&snapshot), Some(("7d", 95)));
        assert_eq!(earliest_reset(&snapshot), Some(("5h", 500)));
    }
}
